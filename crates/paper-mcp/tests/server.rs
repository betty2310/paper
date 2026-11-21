//! The `paper-mcp` binary over stdio, the way Claude Code and Codex start it,
//! beside an engine that plays the app.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use paper_engine::{
    AnnotationKind, Creator, Engine, EngineOptions, ItemFields, NewAnnotation, PageRect, ReaderTabs,
};
use pollster::block_on;
use serde_json::{Value, json};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start(library: &std::path::Path) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_paper-mcp"))
            .arg("--library")
            .arg(library)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut client = Client {
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            child,
            next_id: 0,
        };
        let info = client.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1"}
            }),
        );
        assert_eq!(info["serverInfo"]["name"], "paper");
        client.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        client
    }

    fn send(&mut self, message: Value) {
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "the server quit"
            );
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["id"] == id {
                assert!(message["error"].is_null(), "{message}");
                return message["result"].clone();
            }
        }
    }

    /// The tool's text, or the error text the model would see.
    fn call(&mut self, tool: &str, arguments: Value) -> Result<String, String> {
        let result = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        let text = result["content"][0]["text"].as_str().unwrap().to_string();
        if result["isError"] == true {
            Err(text)
        } else {
            Ok(text)
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn an_assistant_reads_the_paper_open_in_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Library");
    let app = Engine::create_library_with(
        &library,
        EngineOptions {
            settings_dir: Some(dir.path().join("settings")),
            manual_pdf_search: true,
            ..EngineOptions::default()
        },
    )
    .unwrap();
    let row = app
        .create_item(&ItemFields {
            title: "Deep Residual Learning for Image Recognition".into(),
            creators: vec![
                Creator::person("Kaiming", "He"),
                Creator::person("Xiangyu", "Zhang"),
            ],
            year: Some(2016),
            doi: Some("10.1109/cvpr.2016.90".into()),
            abstract_text: Some("Deeper neural networks are more difficult to train.".into()),
            ..ItemFields::default()
        })
        .unwrap();
    let markdown: String = (1..=150)
        .map(|n| format!("Paragraph {n} is about residual learning.\n\n"))
        .collect();
    let mut pdf = pdf_oxide::api::PdfBuilder::new()
        .from_markdown(&markdown)
        .unwrap();
    let path = dir.path().join("resnet.pdf");
    std::fs::write(&path, pdf.to_bytes().unwrap()).unwrap();
    let pdf = block_on(app.attach_file(&row.id, path.to_str().unwrap())).unwrap();
    app.create_annotation(&NewAnnotation {
        attachment_id: pdf.id.clone(),
        kind: AnnotationKind::Note,
        page_index: 1,
        quads: vec![],
        rect: Some(PageRect {
            x0: 72.0,
            y0: 700.0,
            x1: 92.0,
            y1: 720.0,
        }),
        color: "#FFD60A".into(),
        selected_text: None,
        comment: Some("Compare with highway networks".into()),
        text_before: None,
        text_after: None,
    })
    .unwrap();
    let key = app.item_detail(&row.id).unwrap().citation_key.unwrap();
    // The user reads page 2 of the PDF in the front tab.
    app.set_reading_position(&pdf.id, 1, 1.0).unwrap();
    app.set_reader_tabs(ReaderTabs {
        attachment_ids: vec![pdf.id.clone()],
        front: Some(pdf.id.clone()),
    })
    .unwrap();

    let mut client = Client::start(&library);
    let tools = client.request("tools/list", json!({}));
    for tool in tools["tools"].as_array().unwrap() {
        assert_eq!(
            tool["annotations"]["readOnlyHint"], true,
            "{}",
            tool["name"]
        );
    }

    // "Read the paper I'm reading and summarize it."
    let current = client.call("current_paper", json!({})).unwrap();
    assert!(
        current.contains("# Deep Residual Learning for Image Recognition"),
        "{current}"
    );
    assert!(current.contains("on page 2 of"), "{current}");
    assert!(
        current.contains(&format!("Citation key: {key}")),
        "{current}"
    );
    assert!(current.contains("Deeper neural networks"), "{current}");

    let first = client.call("read_paper", json!({"max_chars": 1})).unwrap();
    assert!(first.contains("[page 1]\nParagraph 1 is about"), "{first}");
    assert!(
        !first.contains("[page 2]"),
        "one page, even over the budget: {first}"
    );
    assert!(
        first.contains("Call read_paper with first_page 2 to read on."),
        "{first}"
    );
    let rest = client.call("read_paper", json!({"first_page": 2})).unwrap();
    assert!(rest.contains("Paragraph 150 is about"), "{rest}");
    assert!(rest.ends_with("This is the end of the PDF.\n"), "{rest}");

    let notes = client.call("get_annotations", json!({})).unwrap();
    assert!(
        notes.contains("- Page 2, note (#FFD60A)\n  Comment: Compare with highway networks"),
        "{notes}"
    );

    let found = client
        .call("search_papers", json!({"query": "residual"}))
        .unwrap();
    assert!(found.contains(&format!("key: {key}")), "{found}");
    let cited = client
        .call("get_citations", json!({"papers": ["10.1109/CVPR.2016.90"]}))
        .unwrap();
    assert!(cited.contains(&format!("{{{key},")), "{cited}");
    assert!(
        client
            .call("get_paper", json!({"paper": "no such paper"}))
            .is_err()
    );

    // The user quits Paper: nothing is open, and the library still reads.
    app.close();
    let closed = client.call("current_paper", json!({})).unwrap_err();
    assert!(closed.contains("Paper is not running"), "{closed}");
    let detail = client.call("get_paper", json!({"paper": key})).unwrap();
    assert!(detail.contains("Kaiming He, Xiangyu Zhang"), "{detail}");
}
