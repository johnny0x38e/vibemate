//! Fault-injection tests for central definitions, with temporary DBs and fake credentials.
mod grok_preflight_tests;
mod grok_preview_tests;
mod pi_preflight_tests;
mod pi_preview_tests;

use super::repository::lock;
use super::validation::{field_name, normalize_url, validate};
use super::*;
use crate::credentials::Secret;
use crate::credentials::{CredentialError, CredentialStore};
use crate::storage::Storage;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    path: PathBuf,
    storage: Storage,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "vibemate-mcp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let storage = Storage::open_in_directory(&path).unwrap();
        Self { path, storage }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
#[derive(Default)]
struct FakeStore {
    values: RefCell<BTreeMap<String, String>>,
    saves: Cell<usize>,
    fail_save_at: Cell<Option<usize>>,
    fail_delete: Cell<bool>,
}
impl CredentialStore for FakeStore {
    fn save(&self, reference: &str, secret: &Secret) -> Result<(), CredentialError> {
        let call = self.saves.get() + 1;
        self.saves.set(call);
        if self.fail_save_at.get() == Some(call) {
            return Err(CredentialError::OperationFailed);
        }
        self.values
            .borrow_mut()
            .insert(reference.to_string(), secret.expose().to_string());
        Ok(())
    }
    fn load(&self, _: &str) -> Result<Option<Secret>, CredentialError> {
        panic!("metadata operations must not read secrets")
    }
    fn delete(&self, reference: &str) -> Result<(), CredentialError> {
        if self.fail_delete.get() {
            return Err(CredentialError::OperationFailed);
        }
        self.values.borrow_mut().remove(reference);
        Ok(())
    }
}
fn request() -> SaveMcpRequest {
    SaveMcpRequest {
        id: None,
        expected_revision: None,
        display_name: "工具箱".into(),
        server_name: "tools".into(),
        enabled: true,
        connection: ConnectionInput::Stdio {
            command: "nonexistent-vibemate-test-server".into(),
            args: vec!["--stdio".into()],
            cwd: Some("/nonexistent/work".into()),
            env: vec![],
        },
    }
}
fn secret(name: &str, value: Option<&str>) -> SecretInput {
    SecretInput {
        name: name.into(),
        value: value.map(|v| Secret::new(v.into())),
    }
}
fn with_env(fields: Vec<SecretInput>) -> SaveMcpRequest {
    let mut input = request();
    if let ConnectionInput::Stdio { env, .. } = &mut input.connection {
        *env = fields;
    }
    input
}
fn edit(row: &McpRecord, fields: Vec<SecretInput>) -> SaveMcpRequest {
    let mut input = with_env(fields);
    input.id = Some(row.id.clone());
    input.expected_revision = Some(row.revision);
    input
}
#[test]
fn mcp_save_restart_and_disable_never_execute_the_command() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let row = save_definition(&fixture.storage, &store, request(), 10).unwrap();
    assert!(row.enabled);
    assert_eq!(row.revision, 1);
    assert_eq!(store.saves.get(), 0);
    let reopened = Storage::open_in_directory(&fixture.path).unwrap();
    assert_eq!(
        get_definition(&reopened, &row.id).unwrap().server_name,
        "tools"
    );
    let mut input = edit(&row, vec![]);
    input.enabled = false;
    let disabled = save_definition(&reopened, &store, input, 5).unwrap();
    assert!(!disabled.enabled);
    assert_eq!(disabled.id, row.id);
    assert_eq!(disabled.revision, 2);
    assert_eq!(disabled.updated_at_ms, 10);
}
#[test]
fn mcp_secret_values_never_enter_db_responses_or_debug_output() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let synthetic = "mcp-synthetic-do-not-persist-123";
    let input = with_env(vec![secret("API_TOKEN", Some(synthetic))]);
    assert!(!format!("{input:?}").contains(synthetic));
    let row = save_definition(&fixture.storage, &store, input, 10).unwrap();
    assert!(!serde_json::to_string(&row).unwrap().contains(synthetic));
    assert!(!format!("{row:?}").contains(synthetic));
    assert_eq!(store.values.borrow().values().next().unwrap(), synthetic);
    let bytes = std::fs::read(fixture.path.join("vibemate.sqlite3")).unwrap();
    assert!(
        !bytes
            .windows(synthetic.len())
            .any(|w| w == synthetic.as_bytes())
    );
    assert_eq!(
        list_definitions(&fixture.storage, None, 10)
            .unwrap()
            .items
            .len(),
        1
    );
}
#[test]
fn mcp_edit_retains_then_replaces_and_removes_secret_fields() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let row = save_definition(
        &fixture.storage,
        &store,
        with_env(vec![secret("TOKEN", Some("old synthetic"))]),
        1,
    )
    .unwrap();
    let old_ref = store.values.borrow().keys().next().unwrap().clone();
    let row = save_definition(
        &fixture.storage,
        &store,
        edit(&row, vec![secret("TOKEN", None)]),
        2,
    )
    .unwrap();
    assert_eq!(store.saves.get(), 1);
    assert!(store.values.borrow().contains_key(&old_ref));
    let row = save_definition(
        &fixture.storage,
        &store,
        edit(&row, vec![secret("TOKEN", Some("new synthetic"))]),
        3,
    )
    .unwrap();
    assert_eq!(store.values.borrow().len(), 1);
    assert!(!store.values.borrow().contains_key(&old_ref));
    save_definition(&fixture.storage, &store, edit(&row, vec![]), 4).unwrap();
    assert!(store.values.borrow().is_empty());
}
#[test]
fn mcp_partial_credential_failure_compensates_without_a_definition() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    store.fail_save_at.set(Some(2));
    assert_eq!(
        save_definition(
            &fixture.storage,
            &store,
            with_env(vec![secret("A", Some("one")), secret("B", Some("two"))]),
            1
        )
        .unwrap_err(),
        McpError::CredentialStoreFailed
    );
    assert!(store.values.borrow().is_empty());
    assert!(
        list_definitions(&fixture.storage, None, 10)
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        list_definitions(&fixture.storage, None, 10).unwrap().total,
        0
    );
}
#[test]
fn mcp_failed_db_write_restores_old_metadata_and_keeps_old_secret() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let row = save_definition(
        &fixture.storage,
        &store,
        with_env(vec![secret("TOKEN", Some("old"))]),
        1,
    )
    .unwrap();
    lock(&fixture.storage).unwrap().execute_batch("CREATE TEMP TRIGGER fail_mcp BEFORE UPDATE ON mcp_definition BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert_eq!(
        save_definition(
            &fixture.storage,
            &store,
            edit(&row, vec![secret("TOKEN", Some("new"))]),
            2
        )
        .unwrap_err(),
        McpError::WriteFailed
    );
    assert_eq!(
        get_definition(&fixture.storage, &row.id).unwrap().revision,
        1
    );
    assert_eq!(
        store.values.borrow().values().cloned().collect::<Vec<_>>(),
        vec!["old"]
    );
}
#[test]
fn mcp_failed_compensation_reports_unknown_outcome() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    store.fail_delete.set(true);
    lock(&fixture.storage).unwrap().execute_batch("CREATE TEMP TRIGGER fail_mcp BEFORE INSERT ON mcp_definition BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert_eq!(
        save_definition(
            &fixture.storage,
            &store,
            with_env(vec![secret("TOKEN", Some("synthetic"))]),
            1
        )
        .unwrap_err(),
        McpError::OutcomeUnknown
    );
}
#[test]
fn mcp_cleanup_failure_is_recorded_and_retry_does_not_delete_current_values() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let row = save_definition(
        &fixture.storage,
        &store,
        with_env(vec![secret("TOKEN", Some("old"))]),
        1,
    )
    .unwrap();
    store.fail_delete.set(true);
    let row = save_definition(
        &fixture.storage,
        &store,
        edit(&row, vec![secret("TOKEN", Some("new"))]),
        2,
    )
    .unwrap();
    assert!(row.cleanup_pending);
    assert_eq!(store.values.borrow().len(), 2);
    store.fail_delete.set(false);
    assert!(cleanup_credentials(&fixture.storage, &store, &row.id).unwrap());
    assert_eq!(
        store.values.borrow().values().cloned().collect::<Vec<_>>(),
        vec!["new"]
    );
    assert!(
        !get_definition(&fixture.storage, &row.id)
            .unwrap()
            .cleanup_pending
    );
}
#[test]
fn mcp_stale_revisions_and_namespace_collisions_preserve_existing_values() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let mut input = request();
    input.server_name = "my-tools".into();
    let row = save_definition(&fixture.storage, &store, input, 1).unwrap();
    let mut clash = request();
    clash.server_name = "my_tools".into();
    assert_eq!(
        save_definition(&fixture.storage, &store, clash, 2).unwrap_err(),
        McpError::ServerNameTaken
    );
    save_definition(&fixture.storage, &store, edit(&row, vec![]), 2).unwrap();
    assert_eq!(
        save_definition(
            &fixture.storage,
            &store,
            edit(&row, vec![secret("TOKEN", Some("not written"))]),
            3
        )
        .unwrap_err(),
        McpError::RevisionConflict
    );
    assert_eq!(store.saves.get(), 0);
}
#[test]
fn mcp_input_validation_happens_before_credentials() {
    for (name, expected) in [
        ("bad name", McpError::ServerNameInvalid),
        ("", McpError::ServerNameInvalid),
    ] {
        let fixture = Fixture::new();
        let store = FakeStore::default();
        let mut input = with_env(vec![secret("TOKEN", Some("synthetic"))]);
        input.server_name = name.into();
        assert_eq!(
            save_definition(&fixture.storage, &store, input, 1).unwrap_err(),
            expected
        );
        assert_eq!(store.saves.get(), 0);
    }
    for fields in [
        vec![secret("BAD-NAME", Some("value"))],
        vec![secret("A", Some("1")), secret("A", Some("2"))],
    ] {
        assert_eq!(
            validate(with_env(fields)).err().unwrap(),
            McpError::FieldNameInvalid
        );
    }
    assert_eq!(
        validate(with_env(vec![secret("TOKEN", None)]))
            .unwrap()
            .3
            .fields
            .len(),
        1
    );
    let fixture = Fixture::new();
    assert_eq!(
        save_definition(
            &fixture.storage,
            &FakeStore::default(),
            with_env(vec![secret("TOKEN", None)]),
            1
        )
        .unwrap_err(),
        McpError::SecretRequired
    );
    for value in ["", "!run-command", "Bearer ${TOKEN}", "bad\r\nvalue"] {
        assert_eq!(
            validate(with_env(vec![secret("A", Some(value))]))
                .err()
                .unwrap(),
            McpError::SecretInvalid
        );
    }
}
#[test]
fn mcp_http_validation_and_case_insensitive_header_names() {
    for url in [
        "https://example.com/mcp",
        "http://localhost:9000/mcp",
        "http://[::1]:9000/mcp",
        "http://127.0.0.1:9000/mcp",
    ] {
        assert!(normalize_url(url).is_ok());
    }
    for url in [
        "http://example.com/mcp",
        "file:///tmp/mcp",
        "https://user:secret@example.com/mcp",
        "https://example.com/mcp?token=secret",
        "https://example.com/mcp#fragment",
    ] {
        assert_eq!(normalize_url(url), Err(McpError::UrlInvalid));
    }
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let mut input = request();
    input.connection = ConnectionInput::Http {
        url: "https://example.com/mcp".into(),
        headers: vec![secret("Authorization", Some("Bearer synthetic"))],
    };
    let row = save_definition(&fixture.storage, &store, input, 1).unwrap();
    assert!(
        matches!(row.connection,ConnectionRecord::Http{headers,..} if headers==vec!["authorization"])
    );
    assert_eq!(
        field_name("MCP-Session-Id", "header"),
        Err(McpError::FieldNameInvalid)
    );
}
#[test]
fn mcp_pages_are_bounded_stable_and_cursors_are_validated() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    for index in 0..5 {
        let mut input = request();
        input.server_name = format!("server{index}");
        save_definition(&fixture.storage, &store, input, index).unwrap();
    }
    let first = list_definitions(&fixture.storage, None, 2).unwrap();
    // Each page reports the library total, not the rows it returned.
    assert_eq!(first.total, 5);
    let second = list_definitions(&fixture.storage, first.next_cursor.as_deref(), 2).unwrap();
    assert_eq!(second.total, 5);
    let third = list_definitions(&fixture.storage, second.next_cursor.as_deref(), 2).unwrap();
    assert_eq!(third.total, 5);
    let ids: Vec<_> = first
        .items
        .iter()
        .chain(&second.items)
        .chain(&third.items)
        .map(|row| row.id.clone())
        .collect();
    assert_eq!(ids.len(), 5);
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 5);
    assert!(ids.windows(2).all(|w| w[0] < w[1]));
    assert!(third.next_cursor.is_none());
    assert_eq!(
        list_definitions(&fixture.storage, None, 101).unwrap_err(),
        McpError::InvalidRequest
    );
    assert_eq!(
        list_definitions(&fixture.storage, Some("bad"), 1).unwrap_err(),
        McpError::InvalidRequest
    );
}
#[test]
fn mcp_corrupt_metadata_is_reported_without_credential_access() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let row = save_definition(&fixture.storage, &store, request(), 1).unwrap();
    lock(&fixture.storage)
        .unwrap()
        .execute(
            "UPDATE mcp_definition SET connection='{}' WHERE id=?1",
            [&row.id],
        )
        .unwrap();
    assert_eq!(
        get_definition(&fixture.storage, &row.id).unwrap_err(),
        McpError::InvalidStoredDefinition
    );
}

#[test]
fn mcp_uncertain_commit_retains_new_secret_and_requires_reload() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    lock(&fixture.storage).unwrap().execute_batch("CREATE TABLE mcp_test_deferred(id TEXT REFERENCES mcp_definition(id) DEFERRABLE INITIALLY DEFERRED); CREATE TEMP TRIGGER fail_commit AFTER INSERT ON mcp_definition BEGIN INSERT INTO mcp_test_deferred VALUES('missing'); END;").unwrap();
    assert_eq!(
        save_definition(
            &fixture.storage,
            &store,
            with_env(vec![secret("TOKEN", Some("synthetic-retained"))]),
            1
        )
        .unwrap_err(),
        McpError::OutcomeUnknown
    );
    assert_eq!(store.values.borrow().len(), 1);
}

#[test]
fn mcp_revision_changed_during_credential_write_cannot_replace_current_metadata() {
    struct ConcurrentStore<'a> {
        storage: &'a Storage,
        id: String,
        values: RefCell<BTreeMap<String, String>>,
    }
    impl CredentialStore for ConcurrentStore<'_> {
        fn save(&self, reference: &str, secret: &Secret) -> Result<(), CredentialError> {
            self.values
                .borrow_mut()
                .insert(reference.into(), secret.expose().into());
            lock(self.storage)
                .unwrap()
                .execute(
                    "UPDATE mcp_definition SET revision=revision+1 WHERE id=?1",
                    [&self.id],
                )
                .unwrap();
            Ok(())
        }
        fn load(&self, _: &str) -> Result<Option<Secret>, CredentialError> {
            panic!("no credential reads")
        }
        fn delete(&self, reference: &str) -> Result<(), CredentialError> {
            self.values.borrow_mut().remove(reference);
            Ok(())
        }
    }
    let fixture = Fixture::new();
    let initial = save_definition(&fixture.storage, &FakeStore::default(), request(), 1).unwrap();
    let store = ConcurrentStore {
        storage: &fixture.storage,
        id: initial.id.clone(),
        values: RefCell::new(BTreeMap::new()),
    };
    assert_eq!(
        save_definition(
            &fixture.storage,
            &store,
            edit(&initial, vec![secret("TOKEN", Some("new"))]),
            2
        )
        .unwrap_err(),
        McpError::RevisionConflict
    );
    assert!(store.values.borrow().is_empty());
    assert_eq!(
        get_definition(&fixture.storage, &initial.id)
            .unwrap()
            .revision,
        2
    );
}

#[test]
fn mcp_unavailable_store_reports_the_cause_without_a_plaintext_fallback() {
    struct UnavailableStore;
    impl CredentialStore for UnavailableStore {
        fn save(&self, _: &str, _: &Secret) -> Result<(), CredentialError> {
            Err(CredentialError::Unavailable)
        }
        fn load(&self, _: &str) -> Result<Option<Secret>, CredentialError> {
            panic!("no reads")
        }
        fn delete(&self, _: &str) -> Result<(), CredentialError> {
            Err(CredentialError::Unavailable)
        }
    }
    let fixture = Fixture::new();
    assert_eq!(
        save_definition(
            &fixture.storage,
            &UnavailableStore,
            with_env(vec![secret("TOKEN", Some("synthetic"))]),
            1
        )
        .unwrap_err(),
        McpError::CredentialStoreUnavailable
    );
    assert!(
        list_definitions(&fixture.storage, None, 10)
            .unwrap()
            .items
            .is_empty()
    );
}

fn deletion(record: &McpRecord) -> DeleteMcpRequest {
    DeleteMcpRequest {
        id: record.id.clone(),
        expected_revision: record.revision,
    }
}

#[test]
fn deletion_removes_only_reviewed_definition_and_its_credentials() {
    let f = Fixture::new();
    let store = FakeStore::default();
    let saved = save_definition(
        &f.storage,
        &store,
        with_env(vec![secret("TOKEN", Some("synthetic-delete-test"))]),
        1,
    )
    .unwrap();
    let mut other = with_env(vec![secret("OTHER", Some("synthetic-other-test"))]);
    other.server_name = "other".into();
    let other = save_definition(&f.storage, &store, other, 1).unwrap();
    let result = delete_definition(&f.storage, &store, deletion(&saved)).unwrap();
    assert_eq!(result.id, saved.id);
    assert!(!result.cleanup_pending);
    assert_eq!(
        get_definition(&f.storage, &saved.id).unwrap_err(),
        McpError::NotFound
    );
    assert!(get_definition(&f.storage, &other.id).is_ok());
    assert!(
        store
            .values
            .borrow()
            .keys()
            .all(|key| key.starts_with(&format!("mcp-{}-", other.id)))
    );
    let page = list_definitions(&f.storage, None, 20).unwrap();
    assert_eq!(page.total, 1);
    assert!(!page.deleted_cleanup_pending);
}

#[test]
fn deleted_cleanup_survives_restart_and_retries_without_secret_reads() {
    let f = Fixture::new();
    let store = FakeStore::default();
    let saved = save_definition(
        &f.storage,
        &store,
        with_env(vec![secret("TOKEN", Some("synthetic-delete-test"))]),
        1,
    )
    .unwrap();
    store.fail_delete.set(true);
    assert!(
        delete_definition(&f.storage, &store, deletion(&saved))
            .unwrap()
            .cleanup_pending
    );
    let restarted = Storage::open_in_directory(&f.path).unwrap();
    let page = list_definitions(&restarted, None, 20).unwrap();
    assert_eq!(page.total, 0);
    assert!(page.deleted_cleanup_pending);
    assert!(!cleanup_deleted_credentials(&restarted, &store).unwrap());
    store.fail_delete.set(false);
    assert!(cleanup_deleted_credentials(&restarted, &store).unwrap());
    assert!(store.values.borrow().is_empty());
    assert!(
        !list_definitions(&restarted, None, 20)
            .unwrap()
            .deleted_cleanup_pending
    );
}

#[test]
fn invalid_missing_and_stale_deletions_leave_credentials_untouched() {
    let f = Fixture::new();
    let store = FakeStore::default();
    let saved = save_definition(
        &f.storage,
        &store,
        with_env(vec![secret("TOKEN", Some("synthetic-delete-test"))]),
        1,
    )
    .unwrap();
    let before = store.values.borrow().clone();
    for (id, revision, error) in [
        ("bad".to_string(), 1, McpError::InvalidRequest),
        (saved.id.clone(), 0, McpError::InvalidRequest),
        (saved.id.clone(), 2, McpError::RevisionConflict),
        ("f".repeat(32), 1, McpError::NotFound),
    ] {
        assert_eq!(
            delete_definition(
                &f.storage,
                &store,
                DeleteMcpRequest {
                    id,
                    expected_revision: revision
                }
            )
            .unwrap_err(),
            error
        );
        assert_eq!(*store.values.borrow(), before);
    }
}

#[test]
fn failed_delete_transaction_rolls_back_queue_and_keeps_current_credentials() {
    let f = Fixture::new();
    let store = FakeStore::default();
    let saved = save_definition(
        &f.storage,
        &store,
        with_env(vec![secret("TOKEN", Some("synthetic-delete-test"))]),
        1,
    )
    .unwrap();
    lock(&f.storage).unwrap().execute_batch("CREATE TRIGGER fail_mcp_delete BEFORE DELETE ON mcp_definition BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
    let before = store.values.borrow().clone();
    assert_eq!(
        delete_definition(&f.storage, &store, deletion(&saved)).unwrap_err(),
        McpError::WriteFailed
    );
    assert_eq!(*store.values.borrow(), before);
    assert!(
        !get_definition(&f.storage, &saved.id)
            .unwrap()
            .cleanup_pending
    );
}

#[test]
fn http_delete_cleans_current_and_previously_obsolete_credentials() {
    let f = Fixture::new();
    let store = FakeStore::default();
    let mut input = request();
    input.connection = ConnectionInput::Http {
        url: "https://example.com/mcp".into(),
        headers: vec![secret("Authorization", Some("synthetic-old"))],
    };
    let first = save_definition(&f.storage, &store, input, 1).unwrap();
    let mut input = request();
    input.id = Some(first.id.clone());
    input.expected_revision = Some(first.revision);
    input.connection = ConnectionInput::Http {
        url: "https://example.com/mcp".into(),
        headers: vec![secret("Authorization", Some("synthetic-new"))],
    };
    store.fail_delete.set(true);
    let saved = save_definition(&f.storage, &store, input, 2).unwrap();
    assert!(saved.cleanup_pending);
    assert_eq!(store.values.borrow().len(), 2);
    store.fail_delete.set(false);
    assert!(
        !delete_definition(&f.storage, &store, deletion(&saved))
            .unwrap()
            .cleanup_pending
    );
    assert!(store.values.borrow().is_empty());
}

#[test]
fn no_secret_delete_needs_no_credential_store() {
    let f = Fixture::new();
    let store = FakeStore::default();
    store.fail_delete.set(true);
    let saved = save_definition(&f.storage, &store, request(), 1).unwrap();
    assert!(
        !delete_definition(&f.storage, &store, deletion(&saved))
            .unwrap()
            .cleanup_pending
    );
}

#[test]
fn uncertain_delete_commit_keeps_credentials_and_requires_reload() {
    let f = Fixture::new();
    let store = FakeStore::default();
    let saved = save_definition(
        &f.storage,
        &store,
        with_env(vec![secret("TOKEN", Some("synthetic-uncertain-delete"))]),
        1,
    )
    .unwrap();
    let before = store.values.borrow().clone();
    lock(&f.storage).unwrap().execute_batch("CREATE TABLE mcp_delete_deferred(id TEXT REFERENCES mcp_definition(id) DEFERRABLE INITIALLY DEFERRED); CREATE TEMP TRIGGER fail_delete_commit AFTER DELETE ON mcp_definition BEGIN INSERT INTO mcp_delete_deferred VALUES('missing'); END;").unwrap();
    assert_eq!(
        delete_definition(&f.storage, &store, deletion(&saved)).unwrap_err(),
        McpError::OutcomeUnknown
    );
    assert_eq!(*store.values.borrow(), before);
    assert!(get_definition(&f.storage, &saved.id).is_ok());
}

#[test]
fn deleted_cleanup_is_bounded_and_preserves_active_definitions() {
    let f = Fixture::new();
    let store = FakeStore::default();
    let active = save_definition(
        &f.storage,
        &store,
        with_env(vec![secret("TOKEN", Some("synthetic-active"))]),
        1,
    )
    .unwrap();
    let before = store.values.borrow().clone();
    {
        let connection = lock(&f.storage).unwrap();
        for index in 1..=101 {
            let id = format!("{index:032x}");
            let reference = format!("mcp-{id}-{:032x}", 1);
            connection
                .execute(
                    "INSERT INTO mcp_cleanup(definition_id,credential_ref) VALUES(?1,?2)",
                    rusqlite::params![id, reference],
                )
                .unwrap();
        }
    }
    assert!(!cleanup_deleted_credentials(&f.storage, &store).unwrap());
    assert_eq!(
        super::repository::deleted_cleanup_ids(&f.storage)
            .unwrap()
            .len(),
        1
    );
    assert!(cleanup_deleted_credentials(&f.storage, &store).unwrap());
    assert_eq!(*store.values.borrow(), before);
    assert!(get_definition(&f.storage, &active.id).is_ok());
}
