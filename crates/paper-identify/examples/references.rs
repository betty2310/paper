//! Prints what `PdfFile::references` reads from PDFs, one JSON line per file.
//! Usage: cargo run -p paper-identify --example references -- a.pdf b.pdf
fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).expect("read");
        let file = paper_identify::PdfFile::open(bytes).expect("pdf");
        let started = std::time::Instant::now();
        let list = file.references();
        let ms = started.elapsed().as_millis();
        match list {
            None => println!("{path}\tnone\t{ms}ms"),
            Some(list) => {
                let numbered = list.entries.iter().filter(|e| e.number.is_some()).count();
                let with_id = list
                    .entries
                    .iter()
                    .filter(|e| !e.dois.is_empty() || e.arxiv_id.is_some())
                    .count();
                println!(
                    "{path}\tentries {}\tnumbered {numbered}\twith id {with_id}\tcitations {}\t{ms}ms",
                    list.entries.len(),
                    list.citations.len()
                );
                for (i, entry) in list.entries.iter().enumerate() {
                    if std::env::var("REFPARTS").is_ok() {
                        println!("  text {i}: {}", entry.text);
                        println!(
                            "  parts {i}: {:?}",
                            entry
                                .parts
                                .iter()
                                .map(|p| (
                                    p.page_index,
                                    p.rect.x0 as i32,
                                    p.rect.y0 as i32,
                                    p.rect.x1 as i32,
                                    p.rect.y1 as i32
                                ))
                                .collect::<Vec<_>>()
                        );
                    }
                    let cited = list.citations.iter().filter(|c| c.entry == i).count();
                    println!(
                        "  {i}\t{:?}\tcited {cited}\t{:?}\t{:?}\t{}",
                        entry.number,
                        entry.dois.first(),
                        entry.arxiv_id,
                        entry.text.chars().take(90).collect::<String>()
                    );
                }
            }
        }
    }
}
