//! End to end: the engine API over a real library folder and a scripted HTTP
//! client, the way the app uses it.

use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use paper_engine::{
    Engine, EngineObserver, EngineOptions, Identification, ItemFields, ItemKind, ItemQuery,
    LookupStatus, MetadataPatch, PaperError, PdfStatus, PdfStep, Scope,
};
use paper_fetch::testing::FakeHttp;
use pollster::block_on;

const CROSSREF_DEEP_LEARNING: &str = "https://api.crossref.org/works/10.1038/nature14539";
const CROSSREF_ATTENTION: &str = "https://api.crossref.org/works/10.48550/arxiv.1706.03762";
const DOI_ORG_ATTENTION: &str = "https://doi.org/10.48550/arxiv.1706.03762";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../paper-fetch/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Items(Vec<String>),
    Collections,
    Tags,
    Lookup(String, LookupStatus),
    Pdf(String, PdfStatus),
    Step(String, PdfStep),
}

/// Records every event with the thread it arrived on.
#[derive(Default)]
struct Recorder(Mutex<Vec<(Event, ThreadId)>>);

impl Recorder {
    fn take(&self) -> Vec<Event> {
        self.take_with_threads()
            .into_iter()
            .map(|(event, _)| event)
            .collect()
    }

    fn take_with_threads(&self) -> Vec<(Event, ThreadId)> {
        std::mem::take(&mut self.0.lock().unwrap())
    }

    fn push(&self, event: Event) {
        self.0.lock().unwrap().push((event, thread::current().id()));
    }
}

impl EngineObserver for Recorder {
    fn items_changed(&self, ids: Vec<String>) {
        self.push(Event::Items(ids));
    }
    fn collections_changed(&self) {
        self.push(Event::Collections);
    }
    fn tags_changed(&self) {
        self.push(Event::Tags);
    }
    fn lookup_progress(&self, item_id: String, status: LookupStatus) {
        self.push(Event::Lookup(item_id, status));
    }
    fn pdf_progress(&self, item_id: String, status: PdfStatus) {
        self.push(Event::Pdf(item_id, status));
    }
    fn pdf_step(&self, item_id: String, step: PdfStep) {
        self.push(Event::Step(item_id, step));
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    engine: Engine,
    http: Arc<FakeHttp>,
    events: Arc<Recorder>,
}

fn harness() -> Harness {
    harness_with(true)
}

/// `manual_pdf_search: false` is what the app runs with: every added paper
/// starts a PDF search on its own.
fn harness_with(manual_pdf_search: bool) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::new());
    http.route(
        CROSSREF_DEEP_LEARNING,
        200,
        fixture("crossref_nature14539.json"),
    );
    http.route(CROSSREF_ATTENTION, 404, b"Resource not found.".to_vec());
    http.route(DOI_ORG_ATTENTION, 200, fixture("csl_arxiv_1706_03762.json"));
    let engine = Engine::create_library_with(
        dir.path().join("Library"),
        EngineOptions {
            settings_dir: Some(dir.path().join("settings")),
            http: Some(http.clone()),
            manual_pdf_search,
        },
    )
    .unwrap();
    let events = Arc::new(Recorder::default());
    engine.set_observer(events.clone());
    Harness {
        _dir: dir,
        engine,
        http,
        events,
    }
}

#[test]
fn adding_by_doi_shows_a_placeholder_then_the_looked_up_metadata() {
    let h = harness();
    let row = block_on(
        h.engine
            .add_by_identifier(" https://doi.org/10.1038/NATURE14539 "),
    )
    .unwrap();

    assert_eq!(row.title, "Deep learning");
    assert_eq!(row.authors, "LeCun et al.");
    assert_eq!(row.journal.as_deref(), Some("Nature"));
    assert_eq!(row.year, Some(2015));
    assert_eq!(row.doi.as_deref(), Some("10.1038/nature14539"));
    assert_eq!(row.kind, ItemKind::JournalArticle);
    assert_eq!(row.lookup_status, LookupStatus::Done);

    let events = h.events.take_with_threads();
    assert_eq!(
        events
            .iter()
            .map(|(event, _)| event.clone())
            .collect::<Vec<_>>(),
        vec![
            Event::Items(vec![row.id.clone()]),
            Event::Lookup(row.id.clone(), LookupStatus::Pending),
            Event::Lookup(row.id.clone(), LookupStatus::Done),
            Event::Items(vec![row.id.clone()]),
        ],
        "the app hears about the placeholder before the lookup runs"
    );
    let caller = thread::current().id();
    assert_eq!(
        events[0].1, caller,
        "the placeholder is announced before the call returns"
    );
    assert!(
        events[1..].iter().all(|(_, thread)| *thread != caller),
        "everything the lookup does happens on an engine thread, not where the future is polled"
    );
    let detail = h.engine.item_detail(&row.id).unwrap();
    assert_eq!(detail.fields.creators.len(), 3);
    assert_eq!(detail.fields.volume.as_deref(), Some("521"));
    assert_eq!(h.engine.list_items(&ItemQuery::default()).unwrap().len(), 1);

    let requests = h.http.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].headers["User-Agent"].starts_with("Paper/"),
        "{:?}",
        requests[0].headers
    );
}

#[test]
fn arxiv_ids_fall_back_to_doi_org_and_keep_the_arxiv_id() {
    let h = harness();
    let row = block_on(h.engine.add_by_identifier("arXiv:1706.03762v7")).unwrap();
    assert_eq!(row.title, "Attention Is All You Need");
    assert_eq!(row.kind, ItemKind::Preprint);
    let detail = h.engine.item_detail(&row.id).unwrap();
    assert_eq!(detail.fields.arxiv_id.as_deref(), Some("1706.03762v7"));
    assert_eq!(
        detail.fields.doi.as_deref(),
        Some("10.48550/arxiv.1706.03762")
    );
    assert_eq!(h.http.requests().len(), 2);
}

#[test]
fn bad_input_and_duplicates_are_reported_without_adding_anything() {
    let h = harness();
    assert_eq!(
        block_on(h.engine.add_by_identifier("attention is all you need")),
        Err(PaperError::InvalidIdentifier {
            text: "attention is all you need".into()
        })
    );
    let first = block_on(h.engine.add_by_identifier("10.1038/nature14539")).unwrap();
    h.events.take();
    assert_eq!(
        block_on(h.engine.add_by_identifier("doi:10.1038/nature14539")),
        Err(PaperError::Duplicate {
            doi: "10.1038/nature14539".into(),
            item_id: first.id.clone()
        }),
        "the app can select the existing item"
    );
    assert!(h.events.take().is_empty());
    assert_eq!(h.engine.list_items(&ItemQuery::default()).unwrap().len(), 1);
    assert_eq!(
        h.http.requests().len(),
        1,
        "a duplicate never hits the network"
    );
}

#[test]
fn a_failed_lookup_keeps_the_item_for_retry() {
    let h = harness();
    h.http.fail_with("offline");
    let error = block_on(h.engine.add_by_identifier("10.1038/nature14539")).unwrap_err();
    let PaperError::Lookup {
        doi,
        item_id,
        message,
    } = error
    else {
        panic!("unexpected error {error:?}");
    };
    assert_eq!(doi, "10.1038/nature14539");
    assert_eq!(message, "offline");

    let rows = h.engine.list_items(&ItemQuery::default()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, item_id);
    assert_eq!(
        rows[0].title, "10.1038/nature14539",
        "the identifier stands in for the title"
    );
    assert_eq!(rows[0].lookup_status, LookupStatus::Failed);
    assert!(
        h.events
            .take()
            .contains(&Event::Lookup(item_id.clone(), LookupStatus::Failed))
    );

    h.http.recover();
    let row = block_on(h.engine.retry_lookup(&item_id)).unwrap();
    assert_eq!(row.id, item_id, "the same item is updated in place");
    assert_eq!(row.title, "Deep learning");
    assert_eq!(row.lookup_status, LookupStatus::Done);
    assert_eq!(h.engine.list_items(&ItemQuery::default()).unwrap().len(), 1);
}

#[test]
fn unknown_dois_are_metadata_not_found() {
    let h = harness();
    h.http.route(
        "https://api.crossref.org/works/10.9999/nothing",
        404,
        Vec::new(),
    );
    h.http
        .route("https://doi.org/10.9999/nothing", 404, Vec::new());
    let error = block_on(h.engine.add_by_identifier("10.9999/nothing")).unwrap_err();
    assert!(
        matches!(error, PaperError::MetadataNotFound { ref doi, .. } if doi == "10.9999/nothing"),
        "{error:?}"
    );
    assert_eq!(
        h.engine.list_items(&ItemQuery::default()).unwrap()[0].lookup_status,
        LookupStatus::Failed
    );
}

#[test]
fn delete_can_be_undone_and_the_app_is_told_what_changed() {
    let h = harness();
    let deep = block_on(h.engine.add_by_identifier("10.1038/nature14539")).unwrap();
    let attention = block_on(h.engine.add_by_identifier("1706.03762")).unwrap();
    let reading = h.engine.create_collection("Reading", None).unwrap();
    h.engine
        .add_to_collection(&reading, std::slice::from_ref(&deep.id))
        .unwrap();
    h.engine.set_tags(&deep.id, &["ml".into()]).unwrap();
    h.events.take();

    assert!(!h.engine.can_undo_delete());
    h.engine
        .delete_items(&[deep.id.clone(), "missing".into()])
        .unwrap();
    assert!(h.engine.can_undo_delete());
    assert_eq!(
        h.events.take(),
        vec![
            Event::Items(vec![deep.id.clone()]),
            Event::Collections,
            Event::Tags
        ]
    );
    let rows = h.engine.list_items(&ItemQuery::default()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, attention.id);
    assert_eq!(h.engine.collection_tree().unwrap()[0].item_count, 0);
    assert!(
        h.engine.tags().unwrap().is_empty(),
        "a tag whose only paper is in the trash leaves the sidebar"
    );

    h.engine.delete_items(&["missing".into()]).unwrap();
    assert!(
        h.events.take().is_empty(),
        "deleting nothing is silent and not undoable"
    );

    assert_eq!(h.engine.undo_delete().unwrap(), vec![deep.id.clone()]);
    assert!(!h.engine.can_undo_delete());
    assert_eq!(
        h.events.take(),
        vec![
            Event::Items(vec![deep.id.clone()]),
            Event::Collections,
            Event::Tags
        ]
    );
    assert_eq!(h.engine.list_items(&ItemQuery::default()).unwrap().len(), 2);
    assert_eq!(h.engine.collection_tree().unwrap()[0].item_count, 1);
    let in_reading = ItemQuery {
        scope: Scope::Collection { id: reading },
        ..ItemQuery::default()
    };
    assert_eq!(h.engine.list_items(&in_reading).unwrap()[0].id, deep.id);
    assert_eq!(h.engine.undo_delete().unwrap(), Vec::<String>::new());
}

#[test]
fn re_adding_a_deleted_doi_restores_the_same_item() {
    let h = harness();
    let deep = block_on(h.engine.add_by_identifier("10.1038/nature14539")).unwrap();
    h.engine.save_note(&deep.id, "keep me").unwrap();
    h.engine
        .delete_items(std::slice::from_ref(&deep.id))
        .unwrap();

    assert!(h.engine.can_undo_delete());

    let again = block_on(h.engine.add_by_identifier("10.1038/nature14539")).unwrap();
    assert_eq!(again.id, deep.id);
    assert_eq!(h.engine.note(&deep.id).unwrap().as_deref(), Some("keep me"));
    assert_eq!(h.engine.list_items(&ItemQuery::default()).unwrap().len(), 1);
    assert_eq!(
        h.http.requests().len(),
        2,
        "the metadata is looked up again"
    );
    assert!(
        !h.engine.can_undo_delete(),
        "the item is back, so there is nothing left to undo"
    );
    assert_eq!(h.engine.undo_delete().unwrap(), Vec::<String>::new());
}

#[test]
fn a_doi_that_resolves_to_a_paper_already_in_the_library_leaves_no_junk_row() {
    let h = harness();
    let deep = block_on(h.engine.add_by_identifier("10.1038/nature14539")).unwrap();
    // Crossref answers an alias with the record of the canonical DOI.
    h.http.route(
        "https://api.crossref.org/works/10.1038/nature14539.alias",
        200,
        fixture("crossref_nature14539.json"),
    );
    h.events.take();

    let error = block_on(h.engine.add_by_identifier("10.1038/nature14539.alias")).unwrap_err();
    assert_eq!(
        error,
        PaperError::Duplicate {
            doi: "10.1038/nature14539".into(),
            item_id: deep.id.clone()
        },
        "the app can select the paper it already has"
    );
    let rows = h.engine.list_items(&ItemQuery::default()).unwrap();
    assert_eq!(rows.len(), 1, "no failed placeholder is left behind");
    assert_eq!(rows[0].id, deep.id);
    let events = h.events.take();
    let Event::Items(placeholder) = &events[0] else {
        panic!("{events:?}");
    };
    assert_ne!(placeholder[0], deep.id);
    assert_eq!(
        events.last(),
        Some(&Event::Items(placeholder.clone())),
        "the table is told to drop the placeholder"
    );
    assert!(
        !events.contains(&Event::Lookup(placeholder[0].clone(), LookupStatus::Failed)),
        "{events:?}"
    );
}

#[test]
fn deleting_an_item_while_its_lookup_runs_keeps_the_result_for_undo() {
    let h = harness();
    h.http.hold();
    let result = thread::scope(|scope| {
        let adding = scope.spawn(|| block_on(h.engine.add_by_identifier("10.1038/nature14539")));
        h.http.wait_for_held_request();
        let placeholder = h
            .engine
            .list_items(&ItemQuery::default())
            .unwrap()
            .remove(0);
        assert_eq!(placeholder.lookup_status, LookupStatus::Pending);
        h.engine
            .delete_items(std::slice::from_ref(&placeholder.id))
            .unwrap();
        h.http.release();
        adding.join().unwrap()
    });

    let row = result.expect("the lookup still completes");
    assert_eq!(row.title, "Deep learning");
    assert!(
        h.engine
            .list_items(&ItemQuery::default())
            .unwrap()
            .is_empty(),
        "the delete stands"
    );
    assert_eq!(h.engine.undo_delete().unwrap(), vec![row.id.clone()]);
    let restored = h
        .engine
        .list_items(&ItemQuery::default())
        .unwrap()
        .remove(0);
    assert_eq!(restored.title, "Deep learning");
    assert_eq!(restored.lookup_status, LookupStatus::Done);
    assert_eq!(
        h.engine
            .item_detail(&row.id)
            .unwrap()
            .fields
            .volume
            .as_deref(),
        Some("521")
    );
}

#[test]
fn closing_does_not_wait_for_the_network_and_silences_the_observer() {
    let h = harness();
    h.http.hold();
    let result = thread::scope(|scope| {
        let adding = scope.spawn(|| block_on(h.engine.add_by_identifier("10.1038/nature14539")));
        h.http.wait_for_held_request();
        h.events.take();

        let started = Instant::now();
        h.engine.close();
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "close blocked for {:?} on a lookup that was waiting on the network",
            started.elapsed()
        );
        assert_eq!(
            h.engine.list_items(&ItemQuery::default()),
            Err(PaperError::Closed)
        );

        h.http.release();
        adding.join().unwrap()
    });
    assert_eq!(
        result.map(|row| row.title),
        Ok("Deep learning".to_string()),
        "the lookup that was already running finishes"
    );
    assert!(
        h.events.take().is_empty(),
        "nothing reaches the observer after close"
    );
}

#[test]
fn a_read_only_settings_directory_does_not_keep_the_library_closed() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_dir = dir.path().join("settings");
    std::fs::write(&not_a_dir, b"a file where the settings folder should be").unwrap();
    let engine = Engine::create_library_with(
        dir.path().join("Library"),
        EngineOptions {
            settings_dir: Some(not_a_dir.clone()),
            http: Some(Arc::new(FakeHttp::new())),
            manual_pdf_search: true,
        },
    )
    .expect("the library opens even if the recent list cannot be written");
    assert_eq!(engine.collection_tree().unwrap(), []);
    assert!(
        paper_engine::recent_libraries_in(&not_a_dir).is_empty(),
        "nothing was written"
    );
    assert!(matches!(
        engine.update_settings(engine.settings()),
        Err(PaperError::Settings { .. }) | Err(PaperError::Io { .. })
    ));
}

#[test]
fn retry_prefers_the_journal_doi_over_the_arxiv_id() {
    let h = harness();
    let attention = block_on(h.engine.add_by_identifier("arXiv:1706.03762v7")).unwrap();
    let again = block_on(h.engine.retry_lookup(&attention.id)).unwrap();
    assert_eq!(
        h.engine
            .item_detail(&again.id)
            .unwrap()
            .fields
            .arxiv_id
            .as_deref(),
        Some("1706.03762v7"),
        "with only the DataCite DOI, the retry goes by the arXiv ID the user typed"
    );

    let published = MetadataPatch {
        doi: Some("10.1038/nature14539".into()),
        ..MetadataPatch::default()
    };
    h.engine.update_metadata(&attention.id, &published).unwrap();
    let row = block_on(h.engine.retry_lookup(&attention.id)).unwrap();
    assert_eq!(row.id, attention.id);
    assert_eq!(
        row.title, "Deep learning",
        "the journal DOI the user set wins"
    );
    assert_eq!(row.doi.as_deref(), Some("10.1038/nature14539"));
}

#[test]
fn arxiv_dois_typed_as_dois_still_make_preprints() {
    let h = harness();
    h.http.fail_with("offline");
    let _ = block_on(h.engine.add_by_identifier("10.48550/arxiv.1706.03762"));
    let placeholder = h
        .engine
        .list_items(&ItemQuery::default())
        .unwrap()
        .remove(0);
    assert_eq!(placeholder.kind, ItemKind::Preprint);
    assert_eq!(placeholder.lookup_status, LookupStatus::Failed);
}

#[test]
fn edited_dois_are_normalized_and_the_rest_of_the_api_round_trips() {
    let h = harness();
    let deep = block_on(h.engine.add_by_identifier("10.1038/nature14539")).unwrap();
    h.events.take();

    let patch = MetadataPatch {
        doi: Some("https://doi.org/10.1038/NATURE14539".into()),
        ..MetadataPatch::default()
    };
    h.engine.update_metadata(&deep.id, &patch).unwrap();
    assert_eq!(
        h.engine
            .item_detail(&deep.id)
            .unwrap()
            .fields
            .doi
            .as_deref(),
        Some("10.1038/nature14539")
    );
    let bad = MetadataPatch {
        doi: Some("not a doi".into()),
        ..MetadataPatch::default()
    };
    assert!(matches!(
        h.engine.update_metadata(&deep.id, &bad),
        Err(PaperError::InvalidIdentifier { .. })
    ));
    let clear = MetadataPatch {
        doi: Some(String::new()),
        ..MetadataPatch::default()
    };
    h.engine.update_metadata(&deep.id, &clear).unwrap();
    assert_eq!(h.engine.item_detail(&deep.id).unwrap().fields.doi, None);

    let arxiv = MetadataPatch {
        arxiv_id: Some("arXiv:1706.03762v2".into()),
        ..MetadataPatch::default()
    };
    h.engine.update_metadata(&deep.id, &arxiv).unwrap();
    assert_eq!(
        h.engine
            .item_detail(&deep.id)
            .unwrap()
            .fields
            .arxiv_id
            .as_deref(),
        Some("1706.03762v2"),
        "an edited arXiv ID is normalized like a typed one"
    );
    let bad_arxiv = MetadataPatch {
        arxiv_id: Some("not an arxiv id".into()),
        ..MetadataPatch::default()
    };
    assert!(matches!(
        h.engine.update_metadata(&deep.id, &bad_arxiv),
        Err(PaperError::InvalidIdentifier { .. })
    ));

    let parent = h.engine.create_collection("Parent", None).unwrap();
    let child = h.engine.create_collection("Child", Some(&parent)).unwrap();
    h.engine.rename_collection(&child, "Kid").unwrap();
    h.engine.move_collection(&child, None, 0).unwrap();
    let names: Vec<String> = h
        .engine
        .collection_tree()
        .unwrap()
        .into_iter()
        .map(|node| node.name)
        .collect();
    assert_eq!(names, ["Kid", "Parent"]);
    h.engine.delete_collection(&parent).unwrap();
    assert_eq!(h.engine.collection_tree().unwrap().len(), 1);

    h.engine.set_tags(&deep.id, &["ml".into()]).unwrap();
    let tag = h.engine.tags().unwrap().remove(0);
    h.engine.set_tag_color(&tag.id, Some("#112233")).unwrap();
    assert_eq!(
        h.engine.tags().unwrap()[0].color.as_deref(),
        Some("#112233")
    );

    let mut settings = h.engine.settings();
    assert_eq!(settings.contact_email, "");
    settings.contact_email = "me@example.org".into();
    h.engine.update_settings(settings).unwrap();
    assert_eq!(h.engine.settings().contact_email, "me@example.org");

    let events = h.events.take();
    assert!(events.contains(&Event::Collections));
    assert!(events.contains(&Event::Tags));
    assert!(events.contains(&Event::Items(vec![deep.id.clone()])));
}

#[test]
fn the_contact_email_from_settings_goes_into_requests() {
    let h = harness();
    let mut settings = h.engine.settings();
    settings.contact_email = "me@example.org".into();
    h.engine.update_settings(settings).unwrap();
    h.http.route(
        "https://api.crossref.org/works/10.1038/nature14539?mailto=me@example.org",
        200,
        fixture("crossref_nature14539.json"),
    );
    block_on(h.engine.add_by_identifier("10.1038/nature14539")).unwrap();
    let request = h.http.requests().remove(0);
    assert!(request.url.ends_with("?mailto=me@example.org"));
    assert!(request.headers["User-Agent"].ends_with("mailto:me@example.org)"));
}

#[test]
fn libraries_are_remembered_and_reopen_with_their_data() {
    let dir = tempfile::tempdir().unwrap();
    let settings_dir = dir.path().join("settings");
    let options = || EngineOptions {
        settings_dir: Some(settings_dir.clone()),
        http: None,
        manual_pdf_search: true,
    };
    let a = dir.path().join("A");
    let b = dir.path().join("B");

    assert!(paper_engine::recent_libraries_in(&settings_dir).is_empty());
    assert!(matches!(
        Engine::open_library_with(&a, options()),
        Err(PaperError::Library { .. })
    ));
    let engine = Engine::create_library_with(&a, options()).unwrap();
    let id = engine.create_collection("Kept", None).unwrap();
    engine.close();
    assert_eq!(engine.collection_tree(), Err(PaperError::Closed));
    assert_eq!(
        block_on(engine.add_by_identifier("10.1038/nature14539")),
        Err(PaperError::Closed)
    );
    drop(engine);

    Engine::create_library_with(&b, options()).unwrap().close();
    assert!(
        matches!(
            Engine::create_library_with(&a, options()),
            Err(PaperError::Library { .. })
        ),
        "never overwritten"
    );
    let reopened = Engine::open_library_with(&a, options()).unwrap();
    assert_eq!(reopened.collection_tree().unwrap()[0].id, id);
    assert_eq!(
        reopened.recent_libraries(),
        [
            a.to_string_lossy().into_owned(),
            b.to_string_lossy().into_owned()
        ],
        "most recent first"
    );
    assert!(paper_engine::is_library(&a));
    std::fs::remove_dir_all(&b).unwrap();
    assert_eq!(
        paper_engine::recent_libraries_in(&settings_dir),
        [a.to_string_lossy().into_owned()],
        "missing folders are left out"
    );
}

const ARXIV_PDF_ATTENTION: &str = "https://arxiv.org/pdf/1706.03762v7";
const OPENALEX_DEEP_LEARNING: &str =
    "https://api.openalex.org/works/https://doi.org/10.1038/nature14539";

/// Waits until the observer has seen `event`, then returns everything so far.
fn wait_for(events: &Recorder, event: &Event) -> Vec<Event> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    loop {
        seen.extend(events.take());
        if seen.contains(event) {
            return seen;
        }
        assert!(
            Instant::now() < deadline,
            "never saw {event:?}, only {seen:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn pdf_response(body: &[u8]) -> paper_fetch::Response {
    paper_fetch::Response {
        status: 200,
        body: body.to_vec(),
        content_type: Some("application/pdf".into()),
        retry_after: None,
    }
}

#[test]
fn adding_a_paper_downloads_its_open_access_pdf_on_its_own() {
    let h = harness_with(false);
    h.http.route_responses(
        ARXIV_PDF_ATTENTION,
        vec![pdf_response(b"%PDF-1.5 attention")],
    );

    let row = block_on(h.engine.add_by_identifier("arXiv:1706.03762v7")).unwrap();
    assert!(
        !row.has_pdf,
        "add returns with the metadata, before the PDF"
    );

    let events = wait_for(&h.events, &Event::Pdf(row.id.clone(), PdfStatus::Found));
    let pdf_events: Vec<&Event> = events
        .iter()
        .filter(|event| matches!(event, Event::Pdf(..)))
        .collect();
    assert_eq!(
        pdf_events,
        [
            &Event::Pdf(row.id.clone(), PdfStatus::Searching),
            &Event::Pdf(row.id.clone(), PdfStatus::Found)
        ]
    );
    let detail = h.engine.item_detail(&row.id).unwrap();
    assert_eq!(detail.attachments.len(), 1);
    let file = h.engine.attachment_file(&detail.attachments[0].id).unwrap();
    assert!(
        file.path.ends_with("/Attention Is All You Need.pdf"),
        "{}",
        file.path
    );
    assert_eq!(std::fs::read(&file.path).unwrap(), b"%PDF-1.5 attention");
    assert_eq!((file.last_page, file.last_zoom), (None, None));
    assert!(h.engine.list_items(&ItemQuery::default()).unwrap()[0].has_pdf);
    assert_eq!(
        h.engine
            .list_items(&ItemQuery {
                scope: Scope::NoPdf,
                ..ItemQuery::default()
            })
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        block_on(h.engine.find_pdf(&row.id)).unwrap().id,
        file.id,
        "searching again returns the PDF it has"
    );
    assert_eq!(
        h.http
            .requests()
            .iter()
            .filter(|request| request.url == ARXIV_PDF_ATTENTION)
            .count(),
        1
    );
}

fn checking(item_id: &str, source: &str) -> Event {
    Event::Step(
        item_id.to_string(),
        PdfStep::Checking {
            source: source.into(),
        },
    )
}

#[test]
fn closed_papers_report_no_pdf_and_offline_searches_can_be_retried() {
    let h = harness();
    h.http.route(
        OPENALEX_DEEP_LEARNING,
        200,
        br#"{"open_access":{"is_oa":false},"best_oa_location":null,"locations":[]}"#.to_vec(),
    );
    let row = block_on(h.engine.add_by_identifier("10.1038/nature14539")).unwrap();
    h.events.take();

    assert_eq!(
        block_on(h.engine.find_pdf(&row.id)),
        Err(PaperError::PdfNotFound {
            item_id: row.id.clone()
        })
    );
    assert_eq!(
        h.events.take(),
        [
            Event::Pdf(row.id.clone(), PdfStatus::Searching),
            checking(&row.id, "openalex"),
            Event::Pdf(row.id.clone(), PdfStatus::NotFound)
        ]
    );

    h.http.fail_with("offline");
    assert!(matches!(
        block_on(h.engine.find_pdf(&row.id)),
        Err(PaperError::Network { .. })
    ));
    assert_eq!(
        h.events.take().last(),
        Some(&Event::Pdf(row.id.clone(), PdfStatus::Failed))
    );

    h.http.recover();
    h.http.route(
        OPENALEX_DEEP_LEARNING,
        200,
        br#"{"best_oa_location":{"pdf_url":"https://example.org/deep.pdf"},"locations":[]}"#
            .to_vec(),
    );
    h.http.route_responses(
        "https://example.org/deep.pdf",
        vec![pdf_response(b"%PDF-1.3")],
    );
    let attached = block_on(h.engine.find_pdf(&row.id)).unwrap();
    assert_eq!(attached.item_id, row.id);
    assert_eq!(
        h.events.take(),
        [
            Event::Pdf(row.id.clone(), PdfStatus::Searching),
            checking(&row.id, "openalex"),
            Event::Step(
                row.id.clone(),
                PdfStep::Downloading {
                    source: "openalex".into(),
                    received_bytes: 8,
                    total_bytes: Some(8),
                }
            ),
            Event::Pdf(row.id.clone(), PdfStatus::Found),
            Event::Items(vec![row.id.clone()]),
        ]
    );
}

#[test]
fn one_item_has_one_pdf_search_at_a_time() {
    let h = harness();
    h.http.route(
        OPENALEX_DEEP_LEARNING,
        200,
        br#"{"best_oa_location":{"pdf_url":"https://example.org/deep.pdf"},"locations":[]}"#
            .to_vec(),
    );
    h.http.route_responses(
        "https://example.org/deep.pdf",
        vec![pdf_response(b"%PDF-1.3")],
    );
    let row = block_on(h.engine.add_by_identifier("10.1038/nature14539")).unwrap();

    h.http.hold();
    thread::scope(|scope| {
        let first = scope.spawn(|| block_on(h.engine.find_pdf(&row.id)));
        h.http.wait_for_held_request();
        assert!(matches!(
            block_on(h.engine.find_pdf(&row.id)),
            Err(PaperError::Library { .. })
        ));
        h.http.release();
        assert!(first.join().unwrap().is_ok());
    });
    assert_eq!(h.engine.item_detail(&row.id).unwrap().attachments.len(), 1);
}

#[test]
fn attached_files_are_copied_and_reading_positions_are_kept() {
    let h = harness();
    let row = block_on(h.engine.add_by_identifier("10.1038/nature14539")).unwrap();
    let outside = h._dir.path().join("Downloads");
    std::fs::create_dir_all(&outside).unwrap();
    let html = outside.join("page.pdf");
    std::fs::write(&html, b"<!DOCTYPE html>").unwrap();
    let pdf = outside.join("lecun.pdf");
    std::fs::write(&pdf, b"%PDF-1.4 deep").unwrap();
    h.events.take();

    assert_eq!(
        block_on(h.engine.attach_file(&row.id, html.to_str().unwrap())),
        Err(PaperError::NotPdf {
            path: html.display().to_string()
        })
    );
    let attached = block_on(h.engine.attach_file(&row.id, pdf.to_str().unwrap())).unwrap();
    assert_eq!(h.events.take(), [Event::Items(vec![row.id.clone()])]);
    assert!(pdf.exists(), "the original stays where it was");
    let file = h.engine.attachment_file(&attached.id).unwrap();
    assert_ne!(file.path, pdf.display().to_string());
    assert!(
        file.path
            .starts_with(&h.engine.library_path().display().to_string())
    );
    assert_eq!(std::fs::read(&file.path).unwrap(), b"%PDF-1.4 deep");

    let unread = ItemQuery {
        scope: Scope::Unread,
        ..ItemQuery::default()
    };
    assert_eq!(h.engine.list_items(&unread).unwrap().len(), 1);
    h.engine.set_reading_position(&attached.id, 2, 1.5).unwrap();
    assert_eq!(
        h.events.take(),
        [Event::Items(vec![row.id.clone()])],
        "the first open takes the paper out of Unread"
    );
    h.engine.set_reading_position(&attached.id, 3, 1.5).unwrap();
    assert!(h.events.take().is_empty(), "later page turns are quiet");
    assert_eq!(h.engine.list_items(&unread).unwrap().len(), 0);
    let file = h.engine.attachment_file(&attached.id).unwrap();
    assert_eq!((file.last_page, file.last_zoom), (Some(3), Some(1.5)));
}

#[test]
fn a_typed_in_manuscript_needs_only_a_title_and_never_goes_online() {
    // The app's settings: an added paper would search for a PDF on its own.
    let h = harness_with(false);
    let blank = ItemFields {
        title: "  \n ".into(),
        ..ItemFields::default()
    };
    assert!(matches!(
        h.engine.create_item(&blank),
        Err(PaperError::Library { .. })
    ));

    let draft = ItemFields {
        title: "  Diffusion for tiny datasets \n".into(),
        abstract_text: Some("We show it works.".into()),
        ..ItemFields::default()
    };
    let row = h.engine.create_item(&draft).unwrap();
    assert_eq!(row.title, "Diffusion for tiny datasets");
    assert_eq!(row.lookup_status, LookupStatus::Done);
    assert!(!row.has_pdf);
    assert_eq!(h.events.take(), [Event::Items(vec![row.id.clone()])]);
    let detail = h.engine.item_detail(&row.id).unwrap();
    assert_eq!(detail.fields.kind, Some(ItemKind::Other));
    assert_eq!(
        detail.fields.abstract_text.as_deref(),
        Some("We show it works.")
    );
    assert_eq!((detail.fields.doi, detail.fields.arxiv_id), (None, None));
    let no_pdf = ItemQuery {
        scope: Scope::NoPdf,
        ..ItemQuery::default()
    };
    assert_eq!(h.engine.list_items(&no_pdf).unwrap().len(), 1);

    let pdf = h._dir.path().join("draft.pdf");
    std::fs::write(&pdf, b"%PDF-1.7 draft").unwrap();
    block_on(h.engine.attach_file(&row.id, pdf.to_str().unwrap())).unwrap();
    assert!(h.engine.list_items(&no_pdf).unwrap().is_empty());
    assert!(
        h.http.requests().is_empty(),
        "no lookup and no PDF search for a typed-in paper"
    );

    // Identifiers typed in are normalized and checked like edited ones.
    let published = ItemFields {
        title: "Deep learning".into(),
        doi: Some(" https://doi.org/10.1038/NATURE14539 ".into()),
        arxiv_id: Some(String::new()),
        ..ItemFields::default()
    };
    let row = h.engine.create_item(&published).unwrap();
    assert_eq!(row.doi.as_deref(), Some("10.1038/nature14539"));
    assert_eq!(h.engine.item_detail(&row.id).unwrap().fields.arxiv_id, None);
    assert!(matches!(
        h.engine.create_item(&published),
        Err(PaperError::Duplicate { item_id, .. }) if item_id == row.id
    ));
    let bad = ItemFields {
        title: "Bad".into(),
        doi: Some("not a doi".into()),
        ..ItemFields::default()
    };
    assert!(matches!(
        h.engine.create_item(&bad),
        Err(PaperError::InvalidIdentifier { .. })
    ));
    assert!(h.http.requests().is_empty());
}

// ----- import_pdf -----

const CROSSREF_TEN_RULES: &str = "https://api.crossref.org/works/10.1371/journal.pcbi.1005619";

/// Writes a one page PDF from Markdown, the way a paper's first page looks to
/// the text extractor: the `#` heading is the largest text.
fn write_pdf(dir: &std::path::Path, name: &str, markdown: &str) -> std::path::PathBuf {
    let mut pdf = pdf_oxide::api::PdfBuilder::new()
        .from_markdown(markdown)
        .unwrap();
    let path = dir.join(name);
    std::fs::write(&path, pdf.to_bytes().unwrap()).unwrap();
    path
}

fn deep_learning_page(with_doi: bool) -> String {
    format!(
        "# Deep learning\n\nYann LeCun, Yoshua Bengio, Geoffrey Hinton\n\n\
         Deep learning allows computational models that are composed of multiple \
         processing layers to learn representations of data.\n\n{}\
         ## References\n\n1. Hinton, G. A fast learning algorithm. doi:10.1162/neco.2006.18.7.1527\n",
        if with_doi {
            "Nature 521, 436-444 (2015). https://doi.org/10.1038/nature14539\n\n"
        } else {
            ""
        }
    )
}

fn model_answer(doi: &str, title: &str) -> Vec<u8> {
    let answer = serde_json::json!({
        "doi": doi, "arxiv_id": null, "title": title, "first_author_family_name": "LeCun"
    });
    serde_json::json!({"content": [{"type": "text", "text": answer.to_string()}]})
        .to_string()
        .into_bytes()
}

#[test]
fn a_pdf_that_prints_its_doi_is_filed_under_that_paper() {
    let h = harness();
    let path = write_pdf(h._dir.path(), "lecun2015.pdf", &deep_learning_page(true));
    let original = std::fs::read(&path).unwrap();

    let imported = block_on(h.engine.import_pdf(path.to_str().unwrap())).unwrap();

    assert_eq!(imported.identification, Identification::FileText);
    let row = &imported.item;
    assert_eq!(row.title, "Deep learning");
    assert_eq!(row.doi.as_deref(), Some("10.1038/nature14539"));
    assert_eq!(row.lookup_status, LookupStatus::Done);
    assert!(row.has_pdf);
    let detail = h.engine.item_detail(&row.id).unwrap();
    assert_eq!(detail.attachments.len(), 1);
    let file = h.engine.attachment_file(&detail.attachments[0].id).unwrap();
    assert!(file.path.ends_with("/Deep learning.pdf"), "{}", file.path);
    assert_eq!(std::fs::read(&file.path).unwrap(), original);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "the dropped file is untouched"
    );

    assert!(
        h.http.requests().iter().all(|r| r.method == "GET"),
        "no model without a key"
    );
    assert!(
        !h.http.requests().iter().any(|r| r.url.contains("neco")),
        "cited DOIs after the references heading are never looked up"
    );
    let events = h.events.take();
    assert_eq!(
        events.first(),
        Some(&Event::Items(vec![row.id.clone()])),
        "the row shows at once"
    );
    assert!(events.contains(&Event::Lookup(row.id.clone(), LookupStatus::Pending)));
    assert_eq!(
        &events[events.len() - 2..],
        &[
            Event::Lookup(row.id.clone(), LookupStatus::Done),
            Event::Items(vec![row.id.clone()])
        ]
    );
}

#[test]
fn a_pdf_without_a_doi_is_found_by_its_title() {
    let h = harness();
    let title = "Ten simple rules for structuring papers";
    h.http.route(
        &paper_fetch::title_search_url(title, None),
        200,
        serde_json::json!({"message": {"items": [
            {"DOI": "10.1371/journal.pcbi.1005619.s001", "title": [title], "type": "component"},
            {"DOI": "10.1371/JOURNAL.PCBI.1005619", "title": [title], "type": "journal-article"},
        ]}})
        .to_string()
        .into_bytes(),
    );
    h.http
        .route(CROSSREF_TEN_RULES, 200, fixture("crossref_plos_pcbi.json"));
    let path = write_pdf(
        h._dir.path(),
        "mensh.pdf",
        &format!(
            "# {title}\n\nBrett Mensh, Konrad Kording\n\nGood scientific writing is essential.\n"
        ),
    );

    let imported = block_on(h.engine.import_pdf(path.to_str().unwrap())).unwrap();

    assert_eq!(imported.identification, Identification::TitleSearch);
    assert_eq!(
        imported.item.doi.as_deref(),
        Some("10.1371/journal.pcbi.1005619")
    );
    assert_eq!(imported.item.title, title);
}

#[test]
fn the_model_reads_page_one_only_with_a_key_and_only_when_the_file_does_not_say() {
    let h = harness();
    let path = write_pdf(h._dir.path(), "lecun2015.pdf", &deep_learning_page(false));
    h.http.route(
        paper_fetch::MESSAGES_URL,
        200,
        model_answer("https://doi.org/10.1038/nature14539", "Deep learning"),
    );

    // Without a key nothing leaves the machine but Crossref requests, and the
    // file is kept under its own name for the user to identify.
    let unknown = block_on(h.engine.import_pdf(path.to_str().unwrap())).unwrap();
    assert_eq!(unknown.identification, Identification::NotFound);
    assert_eq!(unknown.item.title, "lecun2015");
    assert_eq!(unknown.item.lookup_status, LookupStatus::Failed);
    assert!(unknown.item.has_pdf);
    assert!(h.http.requests().iter().all(|r| r.method == "GET"));

    let mut settings = h.engine.settings();
    settings.anthropic_api_key = " sk-test ".into();
    h.engine.update_settings(settings).unwrap();
    let found = block_on(h.engine.import_pdf(path.to_str().unwrap())).unwrap();
    assert_eq!(found.identification, Identification::Model);
    assert_eq!(found.item.doi.as_deref(), Some("10.1038/nature14539"));
    assert_eq!(found.item.title, "Deep learning");

    let posts: Vec<_> = h
        .http
        .requests()
        .into_iter()
        .filter(|r| r.method == "POST")
        .collect();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].headers["x-api-key"], "sk-test");
    let sent: serde_json::Value = serde_json::from_slice(&posts[0].body).unwrap();
    let image = sent["messages"][0]["content"][0]["source"]["data"]
        .as_str()
        .unwrap();
    assert!(image.starts_with("iVBORw0KGgo"), "page 1 goes as a PNG");

    // A DOI the model misreads is not trusted: its registered title differs.
    h.http.route(
        paper_fetch::MESSAGES_URL,
        200,
        model_answer("10.1371/journal.pcbi.1005619", "Deep learning"),
    );
    h.http
        .route(CROSSREF_TEN_RULES, 200, fixture("crossref_plos_pcbi.json"));
    let other = write_pdf(h._dir.path(), "copy.pdf", &deep_learning_page(false));
    let misread = block_on(h.engine.import_pdf(other.to_str().unwrap())).unwrap();
    assert_eq!(misread.identification, Identification::NotFound);
    assert_eq!(misread.item.doi, None);
}

#[test]
fn a_rejected_key_is_reported_and_the_pdf_is_kept() {
    let h = harness();
    let mut settings = h.engine.settings();
    settings.anthropic_api_key = "sk-wrong".into();
    h.engine.update_settings(settings).unwrap();
    h.http.route(
        paper_fetch::MESSAGES_URL,
        401,
        b"{\"type\":\"error\"}".to_vec(),
    );
    let path = write_pdf(h._dir.path(), "lecun2015.pdf", &deep_learning_page(false));

    let imported = block_on(h.engine.import_pdf(path.to_str().unwrap())).unwrap();

    assert_eq!(
        imported.identification,
        Identification::Failed {
            message: "Anthropic did not accept the API key.".into()
        }
    );
    assert!(imported.item.has_pdf);
}

#[test]
fn importing_a_paper_already_in_the_library_gives_that_item_the_pdf() {
    let h = harness();
    let existing = block_on(h.engine.add_by_identifier("10.1038/nature14539")).unwrap();
    assert!(!existing.has_pdf);
    let path = write_pdf(h._dir.path(), "lecun2015.pdf", &deep_learning_page(true));

    let result = block_on(h.engine.import_pdf(path.to_str().unwrap()));

    assert_eq!(
        result.unwrap_err(),
        PaperError::Duplicate {
            doi: "10.1038/nature14539".into(),
            item_id: existing.id.clone()
        }
    );
    let rows = h.engine.list_items(&ItemQuery::default()).unwrap();
    assert_eq!(rows.len(), 1, "the new row is gone");
    assert!(rows[0].has_pdf);

    // A second copy changes nothing: the item keeps its first PDF.
    let again = block_on(h.engine.import_pdf(path.to_str().unwrap()));
    assert!(matches!(again, Err(PaperError::Duplicate { .. })));
    assert_eq!(
        h.engine
            .item_detail(&existing.id)
            .unwrap()
            .attachments
            .len(),
        1
    );
}

#[test]
fn offline_imports_keep_the_file_and_non_pdfs_add_nothing() {
    let h = harness();
    h.http.fail_with("no route to host");
    let path = write_pdf(h._dir.path(), "lecun2015.pdf", &deep_learning_page(true));
    let imported = block_on(h.engine.import_pdf(path.to_str().unwrap())).unwrap();
    assert_eq!(
        imported.identification,
        Identification::Failed {
            message: "no route to host".into()
        }
    );
    assert_eq!(imported.item.title, "lecun2015");
    assert!(imported.item.has_pdf);

    let html = h._dir.path().join("paper.pdf");
    std::fs::write(&html, "<html>Access denied</html>").unwrap();
    assert!(matches!(
        block_on(h.engine.import_pdf(html.to_str().unwrap())),
        Err(PaperError::NotPdf { .. })
    ));
    assert_eq!(h.engine.list_items(&ItemQuery::default()).unwrap().len(), 1);
}
