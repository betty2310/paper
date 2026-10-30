//! Small text clean ups for API fields.

/// Removes XML tags (Crossref abstracts are JATS), decodes the common
/// entities, and collapses whitespace. A leading "Abstract" heading, which
/// JATS puts in its own `<jats:title>`, is dropped.
pub fn strip_jats(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for c in text.chars() {
        match (c, in_tag) {
            ('<', false) => {
                in_tag = true;
                tag.clear();
            }
            ('>', true) => {
                in_tag = false;
                // Block level JATS elements separate paragraphs.
                let name = tag.trim_start_matches('/').to_ascii_lowercase();
                if name.starts_with("jats:p") || name.starts_with("jats:title") || name == "p" {
                    out.push(' ');
                }
            }
            (c, true) => tag.push(c),
            (c, false) => out.push(c),
        }
    }
    let decoded = decode_entities(&out);
    let collapsed = collapse_whitespace(&decoded);
    collapsed
        .strip_prefix("Abstract ")
        .or_else(|| collapsed.strip_prefix("ABSTRACT "))
        .or_else(|| collapsed.strip_prefix("Summary "))
        .unwrap_or(&collapsed)
        .to_string()
}

pub fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jats_abstracts_become_plain_text() {
        let jats = "<jats:title>ABSTRACT</jats:title>\n   <jats:p>An outbreak of\n the virus &amp; its hosts.</jats:p><jats:p>Second <jats:italic>paragraph</jats:italic>.</jats:p>";
        assert_eq!(
            strip_jats(jats),
            "An outbreak of the virus & its hosts. Second paragraph."
        );
        assert_eq!(strip_jats("Plain &lt;text&gt;"), "Plain <text>");
        assert_eq!(strip_jats(""), "");
    }
}
