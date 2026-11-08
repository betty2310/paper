# Research: topic tags from a fixed list

Measured 2026-11-08 (Jev added the same day). Decides how Paper tags each paper with a few topics, such as "Large language models" or "GNSS spoofing", so the user can filter the library by topic from the TAGS section of the sidebar.

## Goal

When a paper is added, Paper should give it one to three topic tags without the user typing them. The tags must come from a short list that the user keeps, so the TAGS section stays short. A paper's own keywords rarely use the list's words, so a step has to decide which list entries a paper's keywords and abstract mean.

## Where a paper's keywords come from

The test set is 60 papers found by OpenAlex searches in the user's areas (LLM code generation, LLM agents, SWE agents, unit test generation, GNSS spoofing, GPS jamming, GNSS/INS navigation, edge AI, embodied AI, GPU training systems, CNN image classification, deep RL, microservices) plus two areas outside them (protein structure prediction, soil microbes), four papers per search. Another 10 papers use the same words in a different sense, to test whether a method can tell the senses apart: face anti-spoofing, radar jamming, image edge detection, power transformer faults, and agent-based modeling. I labeled all 70 papers by hand with the topics in the list below, about two per paper. The set and labels are in `.amp/in/artifacts/topic-tag-test-set.json` in the research thread.

| Source | Papers with it | Notes |
|---|---|---|
| OpenAlex `keywords` | 60 of 60 | About 8 per paper, with a score. Close to author keywords, e.g. "GNSS spoofing detection", "LLM agents", "on-device inference". |
| OpenAlex `topics` | 60 of 60 | Three of about 4,500 topics, each with subfield and field. Too coarse for the user's list. For example, "GNSS positioning and interference" covers spoofing, jamming, and positioning alike, and one GNSS spoofing paper has "Smart Grid Security and Resilience" as a topic. |
| OpenAlex abstract | 50 of 60 | Stored as an inverted index of words to positions, which has to be put back in order. |
| Crossref abstract | 16 of 42 | Crossref did not know the 18 arXiv and Zenodo DOIs, and most publishers send no abstract. |
| Crossref `subject` | 0 of 42 | Crossref no longer fills this field. |
| Keywords printed in the PDF | not measured | IEEE prints "Index Terms", Elsevier and Springer print "Keywords" on page 1. `paper-identify` already reads page 1, so the text could be sent to the model when no abstract is known. |

OpenAlex is the useful source. The engine already calls OpenAlex, but only inside `find_pdf`, and only for papers that are not on arXiv, so the topic step needs its own OpenAlex call by DOI with `select=keywords,abstract_inverted_index`. OpenAlex has arXiv DOIs too.

## Why the keywords cannot be the tags

The 60 papers have 385 different OpenAlex keywords, which is more than six new tags for every paper added. Many are near duplicates, e.g. "GPS spoofing", "GNSS spoofing", "GNSS spoofing detection", and "anti-spoofing". Tags made straight from keywords would fill the sidebar after a few dozen papers, so keywords should only be the input for choosing tags from the list.

## The topic list

The list below starts from the user's areas. Each entry has a name, which is the tag name, and a one sentence description, which is what the model reads. The descriptions are where the senses are separated, e.g. "GNSS spoofing" says it is about satellite navigation signals, so face anti-spoofing does not match it. Near duplicates are merged, so "neural networks" is part of "Deep learning" and "anti-spoofing" is part of "GNSS spoofing". Umbrella topics such as "Artificial intelligence" are left out, because they would apply to most of the library and so would not narrow a filter.

| Group | Tag | Description for the model |
|---|---|---|
| Software | Software engineering | How software is designed, built, and maintained: architecture, microservices, requirements, code review, refactoring, DevOps. |
| | Software testing | Testing and debugging programs: unit test generation, fuzzing, mutation testing, coverage, fault localization. |
| | Code generation | Models or tools that write, complete, or repair source code. |
| | Distributed systems | Systems spread over many machines: cloud, microservices, serverless, consensus, parameter servers. |
| | Security | Attacks on and defenses of computer systems and software, including prompt injection and malware. |
| AI and ML | Machine learning | Classical or general machine learning when no more specific tag below fits. |
| | Deep learning | Neural network methods: CNNs, transformers, autoencoders, training of deep models. |
| | Large language models | Language models such as GPT or Llama, their training, prompting, and evaluation. |
| | AI agents | LLM based agents that plan and act in a loop, including software engineering agents. Not agent-based simulation, not RL agents in general. |
| | Agent harness | The software around an agent model: tool definitions, skills, context management, sandboxes, the Model Context Protocol, evaluation harnesses. |
| | Reinforcement learning | Learning from rewards: deep RL, policy optimization, RL from feedback. |
| | Computer vision | Understanding images and video: classification, detection, segmentation, face analysis. |
| | ML systems | Systems that run machine learning: distributed training, GPU and accelerator use, inference serving, compilers. |
| | Edge AI | Running models on phones, IoT devices, or embedded hardware, including quantization and energy use for such devices. Not image edge detection. |
| | Physical AI | Embodied AI: models that perceive and act in the physical world through robots, world models, vision language action models. |
| | Robotics | Robots, manipulation, robot control and planning. |
| | AI safety | Alignment, harmful outputs, reward hacking, misuse of AI systems. |
| | Benchmarks | Papers whose main contribution is a benchmark or an evaluation method. |
| Navigation | GNSS | Satellite navigation (GPS, Galileo, BeiDou, GLONASS): receivers, signals, positioning. |
| | GNSS spoofing | Fake satellite navigation signals and their detection and mitigation. Not face or voice anti-spoofing. |
| | GNSS jamming | Jamming and radio interference against satellite navigation receivers. Not radar jamming. |
| | Inertial navigation | INS, IMUs, Kalman filters, GNSS/INS and visual-inertial integration. |
| | Localization | Positioning without GNSS or in addition to it: SLAM, visual odometry, indoor positioning. |
| | UAVs | Drones and unmanned aerial vehicles. |
| | Autonomous vehicles | Self-driving cars and their perception, planning, and safety. |
| | Signal processing | Radio, radar, and array signal processing: direction of arrival, beamforming, detection theory. |
| | Wireless and IoT | Wireless networks, 5G, and Internet of Things devices and protocols. |

The list has 27 entries. The user can add, rename, and remove entries, and the TAGS section only lists tags that a paper has, so the sidebar never shows more entries than the list has. "Agent harness" means the software around an AI agent model, as the user confirmed, and not test harnesses.

## Choosing tags from the list

### A word list, measured

Each entry gets a list of words and phrases, e.g. "GNSS spoofing" matches "spoofing", "spoof", and "anti-spoofing". A paper gets every entry whose words appear in its text. I wrote the word lists after reading the test set, so the numbers below are better than the method would do on new papers.

| Text searched | Precision | Recall | Tags per paper |
|---|---|---|---|
| Title | 0.84 | 0.83 | 1.9 |
| Title and OpenAlex keywords | 0.68 | 0.95 | 2.7 |
| Title, keywords, and abstract | 0.56 | 0.96 | 3.3 |
| Title, keywords, and OpenAlex topics | 0.52 | 0.96 | 3.6 |

The hand labels have 2.0 tags per paper. Precision is the share of the tags it gave that are in the labels, and recall is the share of the labels it found. The title row looks good only because the test papers were found by searching for those same words. Every extra field finds the missing tags and adds more wrong ones.

The word list got all 10 papers with a second sense wrong. The face anti-spoofing papers got "GNSS spoofing", the radar jamming papers got "GNSS jamming", the image edge detection papers got "Edge AI", the agent-based modeling papers got "AI agents", and the power transformer papers got "Deep learning" because of the word "transformer". Adding exceptions for each case is possible, but every new entry in the list brings new ones, and the user would have to write word lists for the entries they add.

### Jev, measured

Jev is TypeSafe's decision model (`POST https://api.typesafe.ai/v1/systemone`, model `jev-latest`, which answered as `jev-1.14.0`). It does not write text. It takes a `state` and named questions and returns a probability for each answer, so it cannot invent a tag. I tried two question shapes and two kinds of state, all 70 papers each, with the descriptions from the topic list.

1. One `choice` question with the 27 entries as its options, keeping the top 1, 2, or 3 entries by probability. This is the "pick the 3 best matches" idea.
2. 27 `noul` (yes or no) questions in one request, one per entry, each "This paper is about the research topic X: description". A paper gets the entries whose probability of yes is at least a threshold, at most 3.

The state was either the OpenAlex keywords alone, or the title, venue, keywords, and abstract.

| Questions | State | Rule | Precision | Recall | Tags per paper | Second sense papers with a wrong tag |
|---|---|---|---|---|---|---|
| choice | keywords | top 3 | 0.57 | 0.87 | 3.0 | 9 of 10 |
| choice | keywords | top 3 with p >= 0.1 | 0.84 | 0.64 | 1.5 | 1 of 10 |
| choice | full | top 3 | 0.53 | 0.81 | 3.0 | 9 of 10 |
| choice | full | top 3 with p >= 0.1 | 0.87 | 0.61 | 1.4 | 2 of 10 |
| noul | keywords | up to 3 with p >= 0.8 | 0.86 | 0.77 | 1.7 | 1 of 10 |
| noul | full | up to 3 with p >= 0.5 | 0.81 | 0.88 | 2.1 | 4 of 10 |
| noul | full | up to 3 with p >= 0.8 | 0.86 | 0.79 | 1.8 | 0 of 10 |
| noul | full | up to 3 with p >= 0.9 | 0.93 | 0.69 | 1.5 | 0 of 10 |
| word list | title and keywords | every match | 0.68 | 0.95 | 2.7 | 10 of 10 |

A `choice` question is the wrong shape for tags. Its probabilities add up to 1 across the options, so a paper about GNSS spoofing on UAVs splits its probability among three right answers, and the second and third options of a paper with one clear topic are whatever is left over. Taking the top 3 always gives 3 tags, and on 9 of the 10 second sense papers one of them was wrong. A threshold removes the wrong ones, but then most papers get only one tag.

The `noul` questions are the right shape, because each entry is judged on its own. With the full state and a threshold of 0.8, Jev gave no wrong tag to any of the 10 second sense papers. The face anti-spoofing papers got "Computer vision" and not "GNSS spoofing", and the radar jamming paper got "Reinforcement learning" and "Signal processing" and not "GNSS jamming". The two soil papers got no tags, which is right, and the protein papers got "Deep learning" only when the abstract made it clear. The keywords alone are almost as good (0.86 and 0.77), so a paper without an abstract can still be tagged.

Most of the tags that count as wrong above are defensible. "Distributed systems" on the microservices papers matches its own description, and "ML systems" on the edge inference papers is close. Most missed tags are papers that I labeled with four topics, where the cap of 3 drops one. The real precision is a little higher than the table says. The two copies of one Zenodo paper in the set got the same probabilities to within 0.01, so the answers are stable.

### Haiku, not measured

The Anthropic key in the orb still returns 401 "invalid x-api-key", so Haiku 5.5 could not be compared on the same set. The Haiku design would send one request with the whole list and force its answer to zero to three entry names with a JSON schema whose items are an `enum` of the names (`output_config.format`).

### Speed and cost

| Model | Median time per paper | 90th percentile | Input tokens | Cost per 1,000 papers |
|---|---|---|---|---|
| Jev, 27 noul questions, full state | 0.21 s | 0.26 s | 1,574 | $0.07 |
| Jev, 27 noul questions, keywords only | 0.24 s | 0.31 s | 1,318 | $0.06 |
| Jev, one choice question, full state | 0.22 s | 0.27 s | 1,451 | $0.06 |
| Haiku 5.5, estimated | about 1.2 s | about 2.3 s | about 1,350 | about $0.15 |

Jev costs $0.042 per million input tokens and output tokens are free. The state is billed once per request, so the 27 yes or no questions cost only about 10% more than one choice question. The Haiku row is an estimate from token counts and the PDF identification timings, not a run. Jev answered in a fifth of a second, about six times faster than Haiku reads a PDF page, and at half the estimated cost.

### Rejected approaches

A local embedding model would work offline and cost nothing per paper. The engine would compare each paper's text with each entry's description and keep entries above a threshold. It needs a model file of 100 MB or more and a native inference library such as ONNX Runtime inside the static library that the Swift app links, which `docs/TECH_STACK.md` avoids, and it needs a threshold tuned per entry. Jev with yes or no questions does the same comparison without a threshold per entry.

Mapping OpenAlex topics to the list with a fixed table would need no model. OpenAlex topics do not separate spoofing from jamming, and the topics alone gave the lowest precision of all the inputs, so the table would be wrong for the user's main areas.

## How it fits the engine

The step runs on an engine thread after a lookup or an identified import has saved the paper, next to `find_pdf`. First, it asks OpenAlex for the paper's work by DOI, for its keywords and abstract, and it keeps the response in `source_records` as `openalex`. Second, it sends one Jev request with the title, venue, keywords, and abstract as the state and one `noul` question per topic, and it keeps the response in `source_records` as `typesafe`. Third, it adds the chosen tags to the paper as ordinary tags, then sends `tags_changed` and `topics_added`, and the app shows a toast that names the tags. The key is a "Jev API key" field in Settings next to the OpenAlex and Anthropic keys, and the user gets it from the TypeSafe console.

A paper is tagged automatically once, and the `typesafe` record is what marks it. So when the user removes an automatic tag, a later lookup of the same paper does not add it again, and no extra column or table is needed to remember removed tags. Paper > Tag with Topics asks Jev again for the papers the user picks, one request per paper and several at a time, and it only adds tags.

The topic list is stored per library, because it describes the library's papers, in a `topics` table with the name, the description, and the position (migration 4, which fills in the 27 topics above). The engine API has `topics()`, `set_topics(list)`, and `tag_with_topics(ids)`, in `paper-engine` and `paper-ffi` together. The list is edited in a sheet opened from Settings.

Automatic tags are ordinary tags, so the sidebar filter, search (tags are in `items_fts` already), and tag colors work unchanged.

## Decisions made on 2026-11-08

1. Keep a topic list per library, starting with the 27 topics above, editable from Settings. Automatic tags only ever come from the list.
2. Ask Jev one yes or no question per topic in one request, from the title, venue, abstract, and OpenAlex keywords, and keep up to three topics with a probability of at least 0.8. Do not use a single choice question with the top 3, because it gave a wrong tag on 9 of the 10 second sense papers.
3. Run it once after each lookup and identified import when a Jev API key is set, and again on request from Paper > Tag with Topics. Without a key, add no automatic tags.
4. "Harness" means the software around an AI agent model.
5. Add the tags directly, and show a toast that names them, such as "Tagged GNSS spoofing, GNSS, and Inertial navigation" with the paper's title.
6. Automatic tags are the same as tags the user types, such as "to-read", and share the TAGS group of the sidebar.

## To do

1. Run Haiku 5.5 on the same 70 papers once a valid Anthropic key is set, to finish the comparison.
2. Measure Jev on papers from the user's own library, where the labels are not written by the person who wrote the topic descriptions.
3. Measure how often page 1 of a PDF prints keywords or an abstract when OpenAlex has no abstract.
