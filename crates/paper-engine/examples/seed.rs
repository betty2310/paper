//! Fills a library folder with generated papers, collections, tags, and notes
//! for screenshots and performance checks of the apps.
//!
//! Usage: cargo run -p paper-engine --example seed -- <library dir> [item count]
//!
//! The first items are real looking papers in every lookup state; the rest are
//! generated so the list can be measured at the 5,000 item target.

use paper_core::{Creator, ItemFields, ItemKind, Library, LookupStatus};

const PAPERS: &[(&str, &[(&str, &str)], &str, &str, i32, &str, ItemKind)] = &[
    (
        "Denoising Diffusion Probabilistic Models",
        &[("Jonathan", "Ho"), ("Ajay", "Jain"), ("Pieter", "Abbeel")],
        "Advances in Neural Information Processing Systems",
        "NeurIPS",
        2020,
        "10.48550/arxiv.2006.11239",
        ItemKind::ConferencePaper,
    ),
    (
        "Attention Is All You Need",
        &[
            ("Ashish", "Vaswani"),
            ("Noam", "Shazeer"),
            ("Niki", "Parmar"),
        ],
        "Advances in Neural Information Processing Systems",
        "NeurIPS",
        2017,
        "10.48550/arxiv.1706.03762",
        ItemKind::ConferencePaper,
    ),
    (
        "Highly accurate protein structure prediction with AlphaFold",
        &[
            ("John", "Jumper"),
            ("Richard", "Evans"),
            ("Alexander", "Pritzel"),
        ],
        "Nature",
        "Nature",
        2021,
        "10.1038/s41586-021-03819-2",
        ItemKind::JournalArticle,
    ),
    (
        "Score-Based Generative Modeling through Stochastic Differential Equations",
        &[
            ("Yang", "Song"),
            ("Jascha", "Sohl-Dickstein"),
            ("Diederik P.", "Kingma"),
        ],
        "International Conference on Learning Representations",
        "ICLR",
        2021,
        "10.48550/arxiv.2011.13456",
        ItemKind::ConferencePaper,
    ),
    (
        "An Image is Worth 16x16 Words: Transformers for Image Recognition at Scale",
        &[
            ("Alexey", "Dosovitskiy"),
            ("Lucas", "Beyer"),
            ("Alexander", "Kolesnikov"),
        ],
        "International Conference on Learning Representations",
        "ICLR",
        2021,
        "10.48550/arxiv.2010.11929",
        ItemKind::ConferencePaper,
    ),
    (
        "High-Resolution Image Synthesis with Latent Diffusion Models",
        &[
            ("Robin", "Rombach"),
            ("Andreas", "Blattmann"),
            ("Dominik", "Lorenz"),
        ],
        "IEEE/CVF Conference on Computer Vision and Pattern Recognition",
        "CVPR",
        2022,
        "10.1109/cvpr52688.2022.01042",
        ItemKind::ConferencePaper,
    ),
    (
        "Deep learning",
        &[
            ("Yann", "LeCun"),
            ("Yoshua", "Bengio"),
            ("Geoffrey", "Hinton"),
        ],
        "Nature",
        "Nature",
        2015,
        "10.1038/nature14539",
        ItemKind::JournalArticle,
    ),
    (
        "Equivariant Graph Neural Networks",
        &[
            ("Victor Garcia", "Satorras"),
            ("Emiel", "Hoogeboom"),
            ("Max", "Welling"),
        ],
        "International Conference on Machine Learning",
        "ICML",
        2021,
        "10.48550/arxiv.2102.09844",
        ItemKind::ConferencePaper,
    ),
    (
        "BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding",
        &[
            ("Jacob", "Devlin"),
            ("Ming-Wei", "Chang"),
            ("Kenton", "Lee"),
        ],
        "Proceedings of NAACL-HLT",
        "NAACL",
        2019,
        "10.18653/v1/n19-1423",
        ItemKind::ConferencePaper,
    ),
    (
        "Neural Ordinary Differential Equations",
        &[
            ("Ricky T. Q.", "Chen"),
            ("Yulia", "Rubanova"),
            ("Jesse", "Bettencourt"),
        ],
        "Advances in Neural Information Processing Systems",
        "NeurIPS",
        2018,
        "10.48550/arxiv.1806.07366",
        ItemKind::ConferencePaper,
    ),
    (
        "Flow Matching for Generative Modeling",
        &[
            ("Yaron", "Lipman"),
            ("Ricky T. Q.", "Chen"),
            ("Heli", "Ben-Hamu"),
        ],
        "International Conference on Learning Representations",
        "ICLR",
        2023,
        "10.48550/arxiv.2210.02747",
        ItemKind::ConferencePaper,
    ),
    (
        "XGBoost: A Scalable Tree Boosting System",
        &[("Tianqi", "Chen"), ("Carlos", "Guestrin")],
        "Proceedings of the 22nd ACM SIGKDD International Conference",
        "KDD",
        2016,
        "10.1145/2939672.2939785",
        ItemKind::ConferencePaper,
    ),
];

const ABSTRACT: &str = "We present a class of latent variable models inspired by considerations \
from nonequilibrium thermodynamics. Our best results are obtained by training on a weighted \
variational bound designed according to a novel connection between diffusion probabilistic \
models and denoising score matching with Langevin dynamics, and our models naturally admit a \
progressive lossy decompression scheme that can be interpreted as a generalization of \
autoregressive decoding.";

const WORDS: &[&str] = &[
    "Scalable",
    "Bayesian",
    "Inference",
    "Sparse",
    "Graph",
    "Learning",
    "Optimal",
    "Transport",
    "Robust",
    "Estimation",
    "Latent",
    "Dynamics",
    "Causal",
    "Discovery",
    "Variational",
    "Contrastive",
    "Representation",
    "Stochastic",
    "Control",
    "Federated",
    "Manifold",
];
const FAMILIES: &[&str] = &[
    "Nguyen", "Kim", "Garcia", "Müller", "Okafor", "Tanaka", "Rossi", "Silva", "Novak", "Haddad",
    "Larsen", "Ivanova",
];
const VENUES: &[(&str, &str)] = &[
    ("Journal of Machine Learning Research", "JMLR"),
    ("Nature Methods", "Nat. Methods"),
    ("Physical Review Letters", "PRL"),
    ("Proceedings of the National Academy of Sciences", "PNAS"),
    (
        "IEEE Transactions on Pattern Analysis and Machine Intelligence",
        "TPAMI",
    ),
];

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("usage: seed <library dir> [item count]");
    let count: usize = args.next().map(|n| n.parse().unwrap()).unwrap_or(40);

    let library = if Library::is_library(&dir) {
        Library::open(&dir).unwrap()
    } else {
        Library::create(&dir).unwrap()
    };

    let diffusion = library.create_collection("Diffusion models", None).unwrap();
    let scores = library
        .create_collection("Score matching", Some(&diffusion))
        .unwrap();
    library
        .create_collection("Sampling", Some(&diffusion))
        .unwrap();
    let proteins = library.create_collection("Protein folding", None).unwrap();
    let thesis = library.create_collection("Thesis reading", None).unwrap();

    let mut ids = Vec::new();
    for (index, (title, authors, venue, short, year, doi, kind)) in PAPERS.iter().enumerate() {
        let fields = ItemFields {
            kind: Some(*kind),
            doi: Some(doi.to_string()),
            arxiv_id: doi.strip_prefix("10.48550/arxiv.").map(str::to_string),
            title: title.to_string(),
            creators: authors
                .iter()
                .map(|(given, family)| Creator::person(given, family))
                .collect(),
            container_title: Some(venue.to_string()),
            container_short: Some(short.to_string()),
            year: Some(*year),
            published_date: Some(format!("{year}-0{}", 1 + index % 9)),
            volume: Some(format!("{}", 30 + index)),
            issue: Some("1".to_string()),
            pages: Some(format!("{}–{}", 100 + index * 12, 111 + index * 12)),
            abstract_text: Some(ABSTRACT.to_string()),
            url: Some(format!("https://doi.org/{doi}")),
            ..ItemFields::default()
        };
        let id = library.insert_item(&fields, LookupStatus::Done).unwrap();
        ids.push(id);
    }

    library
        .add_to_collection(
            &diffusion,
            &[ids[0].clone(), ids[3].clone(), ids[5].clone()],
        )
        .unwrap();
    library
        .add_to_collection(&scores, &[ids[3].clone()])
        .unwrap();
    library
        .add_to_collection(&proteins, &[ids[2].clone()])
        .unwrap();
    library
        .add_to_collection(&thesis, &[ids[0].clone(), ids[1].clone(), ids[6].clone()])
        .unwrap();
    library
        .set_tags(&ids[0], &["to-read".into(), "methods".into()])
        .unwrap();
    library
        .set_tags(&ids[1], &["classic".into(), "methods".into()])
        .unwrap();
    library.set_tags(&ids[6], &["classic".into()]).unwrap();
    library
        .save_note(
            &ids[0],
            "## Key idea\n\nLearn to reverse a noising process using a neural network.\n\nCompare with score matching and continuous-time formulations.",
        )
        .unwrap();

    // One lookup still running and one that failed, for the row states.
    let pending = ItemFields {
        kind: Some(ItemKind::Other),
        doi: Some("10.1000/pending.example".to_string()),
        title: "10.1000/pending.example".to_string(),
        ..ItemFields::default()
    };
    library
        .insert_item(&pending, LookupStatus::Pending)
        .unwrap();
    let failed = ItemFields {
        kind: Some(ItemKind::Other),
        doi: Some("10.1000/failed.example".to_string()),
        title: "10.1000/failed.example".to_string(),
        ..ItemFields::default()
    };
    library.insert_item(&failed, LookupStatus::Failed).unwrap();

    for index in 0..count.saturating_sub(PAPERS.len() + 2) {
        let w = |n: usize| WORDS[(index * 7 + n * 3) % WORDS.len()];
        let title = format!(
            "{} {} {} for {} {}",
            w(0),
            w(1),
            w(2),
            w(3).to_lowercase(),
            w(4).to_lowercase()
        );
        let author_count = 1 + index % 4;
        let creators = (0..author_count)
            .map(|n| Creator::person("A.", FAMILIES[(index + n * 5) % FAMILIES.len()]))
            .collect();
        let (venue, short) = VENUES[index % VENUES.len()];
        let fields = ItemFields {
            kind: Some(ItemKind::JournalArticle),
            doi: Some(format!("10.5555/seed.{index}")),
            title,
            creators,
            container_title: Some(venue.to_string()),
            container_short: Some(short.to_string()),
            year: Some(2000 + (index % 26) as i32),
            abstract_text: Some(ABSTRACT.to_string()),
            ..ItemFields::default()
        };
        library.insert_item(&fields, LookupStatus::Done).unwrap();
    }
    println!("seeded {dir}");
}
