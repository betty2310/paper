//! Looks up DOIs or arXiv IDs against the real APIs and prints the mapped
//! metadata. Spike 4 of `docs/ROADMAP.md`.
//!
//! ```text
//! PAPER_CONTACT_EMAIL=me@example.org cargo run -p paper-fetch --example lookup -- 10.1038/nature14539 arXiv:1706.03762
//! ```

use std::sync::Arc;

use paper_fetch::{Contact, Resolver, UreqClient};
use paper_ids::Identifier;

fn main() {
    let contact = Contact::new(std::env::var("PAPER_CONTACT_EMAIL").unwrap_or_default());
    let resolver = Resolver::new(Arc::new(UreqClient::new()));
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: lookup <doi or arxiv id>...");
        std::process::exit(2);
    }
    let mut failures = 0;
    for text in &args {
        let Some(identifier) = Identifier::parse(text) else {
            println!("{text}: not a DOI or arXiv ID\n");
            failures += 1;
            continue;
        };
        match resolver.lookup(&identifier, &contact) {
            Ok(lookup) => {
                let f = &lookup.fields;
                println!("{identifier}");
                println!(
                    "  source:    {}",
                    lookup
                        .records
                        .iter()
                        .map(|r| r.source.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                println!("  kind:      {:?}", f.kind.unwrap());
                println!("  title:     {}", f.title);
                println!(
                    "  authors:   {}",
                    f.creators
                        .iter()
                        .map(|c| c.display_name())
                        .collect::<Vec<_>>()
                        .join("; ")
                );
                println!(
                    "  venue:     {} ({})",
                    f.container_title.as_deref().unwrap_or("-"),
                    f.container_short.as_deref().unwrap_or("-")
                );
                println!(
                    "  date:      {} (year {})",
                    f.published_date.as_deref().unwrap_or("-"),
                    f.year.map_or("-".to_string(), |y| y.to_string())
                );
                println!(
                    "  vol/issue: {} / {}  pages {}",
                    f.volume.as_deref().unwrap_or("-"),
                    f.issue.as_deref().unwrap_or("-"),
                    f.pages.as_deref().unwrap_or("-")
                );
                println!("  publisher: {}", f.publisher.as_deref().unwrap_or("-"));
                println!("  url:       {}", f.url.as_deref().unwrap_or("-"));
                println!("  arxiv:     {}", f.arxiv_id.as_deref().unwrap_or("-"));
                println!(
                    "  abstract:  {}",
                    f.abstract_text
                        .as_deref()
                        .map(|a| a.chars().take(120).collect::<String>() + "…")
                        .unwrap_or("-".into())
                );
                println!();
            }
            Err(error) => {
                println!("{identifier}: {error}\n");
                failures += 1;
            }
        }
    }
    if failures > 0 {
        std::process::exit(1);
    }
}
