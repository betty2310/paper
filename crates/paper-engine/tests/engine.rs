//! End to end: the engine API over a real library folder and a scripted HTTP
//! client, the way the app uses it.

use std::sync::{Arc, Mutex};

use paper_engine::{
    Engine, EngineObserver, EngineOptions, ItemKind, ItemQuery, LookupStatus, MetadataPatch,
    PaperError, Scope,
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
}

#[derive(Default)]
struct Recorder(Mutex<Vec<Event>>);

impl Recorder {
    fn take(&self) -> Vec<Event> {
        std::mem::take(&mut self.0.lock().unwrap())
    }
}

impl EngineObserver for Recorder {
    fn items_changed(&self, ids: Vec<String>) {
        self.0.lock().unwrap().push(Event::Items(ids));
    }
    fn collections_changed(&self) {
        self.0.lock().unwrap().push(Event::Collections);
    }
    fn tags_changed(&self) {
        self.0.lock().unwrap().push(Event::Tags);
    }
    fn lookup_progress(&self, item_id: String, status: LookupStatus) {
        self.0.lock().unwrap().push(Event::Lookup(item_id, status));
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    engine: Engine,
    http: Arc<FakeHttp>,
    events: Arc<Recorder>,
}

fn harness() -> Harness {
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

    let events = h.events.take();
    assert_eq!(
        events,
        vec![
            Event::Items(vec![row.id.clone()]),
            Event::Lookup(row.id.clone(), LookupStatus::Pending),
            Event::Lookup(row.id.clone(), LookupStatus::Done),
            Event::Items(vec![row.id.clone()]),
        ],
        "the app hears about the placeholder before the lookup runs"
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
    assert!(h.engine.tags().unwrap().is_empty() || h.engine.tags().unwrap()[0].item_count == 0);

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

    let again = block_on(h.engine.add_by_identifier("10.1038/nature14539")).unwrap();
    assert_eq!(again.id, deep.id);
    assert_eq!(h.engine.note(&deep.id).unwrap().as_deref(), Some("keep me"));
    assert_eq!(h.engine.list_items(&ItemQuery::default()).unwrap().len(), 1);
    assert_eq!(
        h.http.requests().len(),
        2,
        "the metadata is looked up again"
    );
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
