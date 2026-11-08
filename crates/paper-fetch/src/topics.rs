//! Topic tags: a paper's keywords and abstract from OpenAlex, and Jev, a
//! decision model, judging which entries of the library's topic list the
//! paper is about. The measurements behind the question shape and the
//! threshold are in `docs/research/topic-tags.md`.

use paper_core::{SourceRecord, Topic};
use serde_json::{Map, Value, json};

use crate::FetchError;
use crate::http::Contact;
use crate::pdf::{OPENALEX, openalex_work_url};
use crate::resolver::{Resolver, record};
use crate::text::collapse_whitespace;

/// The source name of Jev's answer in `source_records`.
pub const JEV_SOURCE: &str = "typesafe";
pub const SYSTEM_ONE_URL: &str = "https://api.typesafe.ai/v1/systemone";
pub const JEV_MODEL: &str = "jev-latest";
/// A topic counts when Jev's probability that the paper is about it is at
/// least this. At 0.8 no paper whose words have a second sense (face
/// anti-spoofing, radar jamming, image edge detection) got a wrong tag.
pub const MIN_PROBABILITY: f64 = 0.8;
/// Topics a paper gets at most, so the tags stay useful as filters.
pub const MAX_TOPICS: usize = 3;

/// What Jev reads about a paper.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaperText {
    pub title: String,
    pub venue: Option<String>,
    pub keywords: Vec<String>,
    pub abstract_text: Option<String>,
}

/// A paper's keywords and abstract as OpenAlex has them, and the raw work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAlexWork {
    pub keywords: Vec<String>,
    pub abstract_text: Option<String>,
    pub record: SourceRecord,
}

/// The topics Jev chose, most likely first, and its raw answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicChoice {
    pub names: Vec<String>,
    pub record: SourceRecord,
}

impl Resolver {
    /// Reads a DOI's OpenAlex work. `None` when OpenAlex does not know it.
    pub fn openalex_work(
        &self,
        doi: &str,
        api_key: Option<&str>,
        contact: &Contact,
    ) -> Result<Option<OpenAlexWork>, FetchError> {
        let url = openalex_work_url(doi, api_key);
        let response = self.get(&url, &[("User-Agent", &contact.user_agent())])?;
        if response.status == 404 {
            return Ok(None);
        }
        if !response.is_success() {
            return Err(FetchError::Status {
                status: response.status,
                url,
            });
        }
        let work = response.json(OPENALEX)?;
        let keywords = work["keywords"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|keyword| keyword["display_name"].as_str())
            .map(str::to_string)
            .collect();
        Ok(Some(OpenAlexWork {
            keywords,
            abstract_text: abstract_from_index(&work["abstract_inverted_index"]),
            record: record(OPENALEX, &work),
        }))
    }

    /// Asks Jev, for every topic, whether the paper is about it, in one
    /// request, and keeps up to `MAX_TOPICS` topics at `MIN_PROBABILITY` or
    /// more. Fails with `Status` 401 when the key is wrong.
    pub fn choose_topics(
        &self,
        paper: &PaperText,
        topics: &[Topic],
        api_key: &str,
    ) -> Result<TopicChoice, FetchError> {
        let questions: Map<String, Value> = topics
            .iter()
            .enumerate()
            .map(|(index, topic)| {
                let instructions = format!(
                    "This paper is about the research topic \"{}\": {}",
                    topic.name, topic.description
                );
                (
                    question_key(index),
                    json!({"type": "noul", "instructions": instructions}),
                )
            })
            .collect();
        let body = json!({
            "model": JEV_MODEL,
            "state": {
                "title": paper.title,
                "venue": paper.venue,
                "keywords": paper.keywords,
                "abstract": paper.abstract_text,
            },
            "questions": questions,
        });
        let authorization = format!("Bearer {api_key}");
        let response = self.post(
            SYSTEM_ONE_URL,
            &[
                ("Authorization", &authorization),
                ("Content-Type", "application/json"),
            ],
            body.to_string().as_bytes(),
        )?;
        if !response.is_success() {
            return Err(FetchError::Status {
                status: response.status,
                url: SYSTEM_ONE_URL.to_string(),
            });
        }
        let answer = response.json(JEV_SOURCE)?;
        let mut likely: Vec<(f64, &Topic)> = topics
            .iter()
            .enumerate()
            .filter_map(|(index, topic)| {
                let probability = answer["answers"][question_key(index)]["noul"].as_f64()?;
                (probability >= MIN_PROBABILITY).then_some((probability, topic))
            })
            .collect();
        likely.sort_by(|a, b| b.0.total_cmp(&a.0));
        Ok(TopicChoice {
            names: likely
                .into_iter()
                .take(MAX_TOPICS)
                .map(|(_, topic)| topic.name.clone())
                .collect(),
            record: record(JEV_SOURCE, &answer),
        })
    }
}

/// Question ids are positions, so a topic name never has to be a JSON key.
fn question_key(index: usize) -> String {
    format!("t{index}")
}

/// OpenAlex stores an abstract as each word with its positions; this puts
/// the words back in order.
fn abstract_from_index(index: &Value) -> Option<String> {
    let mut words: Vec<(u64, &str)> = index
        .as_object()?
        .iter()
        .flat_map(|(word, positions)| {
            positions
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .map(move |position| (position, word.as_str()))
        })
        .collect();
    words.sort_unstable();
    let text = collapse_whitespace(
        &words
            .into_iter()
            .map(|(_, word)| word)
            .collect::<Vec<_>>()
            .join(" "),
    );
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::testing::FakeHttp;

    fn topic(name: &str, description: &str) -> Topic {
        Topic {
            name: name.into(),
            description: description.into(),
        }
    }

    #[test]
    fn keywords_and_the_abstract_come_back_in_order() {
        let http = Arc::new(FakeHttp::new());
        let url = openalex_work_url("10.1/gnss", Some("k3y"));
        assert_eq!(
            url,
            "https://api.openalex.org/works/https://doi.org/10.1/gnss?api_key=k3y"
        );
        http.route(
            &url,
            200,
            json!({
                "keywords": [{"display_name": "GNSS spoofing", "score": 1.0}, {"display_name": "UAV"}],
                "abstract_inverted_index": {"spoofing": [1, 4], "Detecting": [0], "is": [2], "hard,": [3]},
            })
            .to_string()
            .into_bytes(),
        );
        let work = Resolver::new(http)
            .openalex_work("10.1/gnss", Some("k3y"), &Contact::new("me@example.org"))
            .unwrap()
            .unwrap();
        assert_eq!(work.keywords, ["GNSS spoofing", "UAV"]);
        assert_eq!(
            work.abstract_text.as_deref(),
            Some("Detecting spoofing is hard, spoofing")
        );
        assert_eq!(work.record.source, OPENALEX);
    }

    #[test]
    fn an_unknown_doi_is_no_work() {
        let http = Arc::new(FakeHttp::new());
        let work = Resolver::new(http)
            .openalex_work("10.1/unknown", None, &Contact::new(""))
            .unwrap();
        assert_eq!(work, None);
    }

    #[test]
    fn jev_is_asked_once_per_topic_and_the_likely_ones_are_kept() {
        let http = Arc::new(FakeHttp::new());
        // Five topics, four of them above the threshold: the three most
        // likely are kept, best first, whatever their order in the list.
        http.route(
            SYSTEM_ONE_URL,
            200,
            json!({"model": "jev-1.14.0", "answers": {
                "t0": {"type": "noul", "noul": 0.81},
                "t1": {"type": "noul", "noul": 0.99},
                "t2": {"type": "noul", "noul": 0.79},
                "t3": {"type": "noul", "noul": 0.95},
                "t4": {"type": "noul", "noul": 0.9},
            }, "usage": {"input_tokens": 900, "output_tokens": 60}})
            .to_string()
            .into_bytes(),
        );
        let topics = [
            topic("UAVs", "Drones."),
            topic(
                "GNSS spoofing",
                "Fake satellite signals. Not face anti-spoofing.",
            ),
            topic("Security", "Attacks on computer systems."),
            topic("GNSS", "Satellite navigation."),
            topic("Inertial navigation", "INS and IMUs."),
        ];
        let paper = PaperText {
            title: "Spoofing detection for drones".into(),
            venue: None,
            keywords: vec!["GNSS spoofing".into()],
            abstract_text: Some("We detect spoofing.".into()),
        };
        let choice = Resolver::new(http.clone())
            .choose_topics(&paper, &topics, "ts-key")
            .unwrap();
        assert_eq!(
            choice.names,
            ["GNSS spoofing", "GNSS", "Inertial navigation"]
        );
        assert_eq!(choice.record.source, JEV_SOURCE);

        let request = &http.requests()[0];
        assert_eq!(request.method, "POST");
        assert_eq!(request.headers["Authorization"], "Bearer ts-key");
        let sent: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(sent["model"], JEV_MODEL);
        assert_eq!(sent["state"]["title"], "Spoofing detection for drones");
        assert_eq!(sent["state"]["venue"], Value::Null);
        assert_eq!(sent["questions"].as_object().unwrap().len(), 5);
        assert_eq!(sent["questions"]["t1"]["type"], "noul");
        assert_eq!(
            sent["questions"]["t1"]["instructions"],
            "This paper is about the research topic \"GNSS spoofing\": Fake satellite signals. Not face anti-spoofing."
        );
    }

    #[test]
    fn a_wrong_key_is_a_401() {
        let http = Arc::new(FakeHttp::new());
        http.route(
            SYSTEM_ONE_URL,
            401,
            b"{\"detail\": \"invalid key\"}".to_vec(),
        );
        let error = Resolver::new(http)
            .choose_topics(&PaperText::default(), &[topic("GNSS", "")], "bad")
            .unwrap_err();
        assert_eq!(
            error,
            FetchError::Status {
                status: 401,
                url: SYSTEM_ONE_URL.into()
            }
        );
    }
}
