//! Behavior tests for provider deletion and its credential compensation.

use super::*;
use crate::credentials::CredentialError;
use crate::credentials::fake::FakeStore;
use crate::providers::{
    ClearProviderReferenceRequest, DeleteProviderRequest, DeletedProvider,
    clear_provider_reference, delete_provider, fail_commits_after_provider_delete,
    provider_deletion_impact,
};
use crate::references::ReferenceState;

const KEY: &str = "vibemate-synthetic-key-7f3a9c";

fn delete_request(id: &str, expected_revision: i64) -> DeleteProviderRequest {
    DeleteProviderRequest {
        id: id.to_string(),
        expected_revision,
    }
}

/// Insert a saved model row for `provider_id`.
fn add_model(storage: &Storage, provider_id: &str, model_id: &str) {
    storage
        .lock()
        .expect("lock")
        .execute(
            "INSERT INTO provider_model
                 (provider_id, model_id, source, selected, created_at, updated_at)
             VALUES (?1, ?2, 'manual', 1, 1, 1)",
            params![provider_id, model_id],
        )
        .expect("insert model");
}

/// Create one provider with the shared `store` at time 100.
fn create_with_store(storage: &Storage, store: &FakeStore, name: &str) -> ProviderRecord {
    create_provider(storage, store, create_request("deepseek", name), 100).expect("create")
}

#[test]
fn delete_removes_the_provider_its_models_and_its_key_only() {
    let directory = TestDirectory::new();
    let storage = directory.open();
    let store = FakeStore::default();
    let removed = create_with_store(&storage, &store, "Removed");
    let kept = create_with_store(&storage, &store, "Kept");
    add_model(&storage, removed.id.as_str(), "vendor/model-a");
    add_model(&storage, removed.id.as_str(), "vendor/model-b");

    let deleted =
        delete_provider(&storage, &store, delete_request(removed.id.as_str(), 1)).expect("delete");
    assert_eq!(
        deleted,
        DeletedProvider {
            id: removed.id.clone(),
            display_name: "Removed".to_string(),
            selected_model_count: 2,
        }
    );
    assert_eq!(table_rows(&storage, "provider_instance"), 1);
    assert_eq!(table_rows(&storage, "provider_model"), 0);
    assert_eq!(table_rows(&storage, "provider_credential"), 1);
    assert_eq!(
        store.stored(&removed.id.credential_reference()),
        None,
        "the deleted provider's key is removed"
    );
    assert_eq!(
        store.stored(&kept.id.credential_reference()).as_deref(),
        Some(KEY),
        "another provider keeps its key"
    );

    // The same ID and an unknown ID report NotFound without touching anything.
    let before = store.call_count();
    assert_eq!(
        delete_provider(&storage, &store, delete_request(removed.id.as_str(), 1)),
        Err(ProviderError::NotFound)
    );
    assert_eq!(
        delete_provider(&storage, &store, delete_request(&"f".repeat(32), 1)),
        Err(ProviderError::NotFound)
    );
    assert_eq!(store.call_count(), before);
    assert_eq!(table_rows(&storage, "provider_instance"), 1);
}

#[test]
fn provider_delete_is_blocked_by_pi_and_grok_bindings_and_keeps_the_key() {
    let directory = TestDirectory::new();
    let storage = directory.open();
    let store = FakeStore::default();
    let record = create_with_store(&storage, &store, "Bound");

    let pi_root = directory.0.join("pi-agent");
    fs::create_dir_all(&pi_root).expect("pi root");
    fs::write(
        pi_root.join("models.json"),
        r#"{"providers":{"vibemate-Pi-test":{"models":[{"id":"deepseek-flash"}]}}}"#,
    )
    .expect("pi models");
    let grok_root = directory.0.join("grok-home");
    fs::create_dir_all(&grok_root).expect("grok root");
    fs::write(
        grok_root.join("config.toml"),
        "[model.vibemate-deepseek-flash]\nmodel = \"deepseek-flash\"\n",
    )
    .expect("grok config");

    storage
        .lock()
        .expect("lock")
        .execute(
            "INSERT INTO pi_provider_binding(config_root, provider_key, provider_id)
             VALUES (?1, 'vibemate-Pi-test', ?2)",
            params![pi_root.to_string_lossy(), record.id.as_str()],
        )
        .expect("pi binding");
    storage
        .lock()
        .expect("lock")
        .execute(
            "INSERT INTO grok_model_binding
                (config_root, model_alias, provider_id, model_id)
             VALUES (?1, 'vibemate-deepseek-flash', ?2, 'deepseek-flash')",
            params![grok_root.to_string_lossy(), record.id.as_str()],
        )
        .expect("grok binding");

    let impact = provider_deletion_impact(&storage, record.id.as_str()).expect("impact");
    assert_eq!(impact.references.len(), 2);
    assert!(!impact.can_delete);
    assert!(
        impact
            .references
            .iter()
            .all(|reference| reference.state == ReferenceState::Active)
    );

    assert_eq!(
        delete_provider(&storage, &store, delete_request(record.id.as_str(), 1)),
        Err(ProviderError::ProviderInUse)
    );
    assert_eq!(table_rows(&storage, "provider_instance"), 1);
    assert_eq!(
        store.stored(&record.id.credential_reference()).as_deref(),
        Some(KEY),
        "the blocked delete never removes the key"
    );
}

#[test]
fn stale_provider_bindings_can_be_cleared_before_deleting() {
    let directory = TestDirectory::new();
    let storage = directory.open();
    let store = FakeStore::default();
    let record = create_with_store(&storage, &store, "Stale");
    let pi_root = directory.0.join("empty-pi");
    fs::create_dir_all(&pi_root).expect("pi root");
    // A saved thinking level is a Pi preference, not a live binding. Deleting
    // the central record must leave it in place rather than rewrite the file.
    let settings = r#"{"modelThinkingLevels":{"vibemate-Pi-test/deepseek-flash":"high"}}"#;
    fs::write(pi_root.join("settings.json"), settings).expect("pi settings");

    storage
        .lock()
        .expect("lock")
        .execute(
            "INSERT INTO pi_provider_binding(config_root, provider_key, provider_id)
             VALUES (?1, 'vibemate-Pi-test', ?2)",
            params![pi_root.to_string_lossy(), record.id.as_str()],
        )
        .expect("pi binding");

    let impact = provider_deletion_impact(&storage, record.id.as_str()).expect("impact");
    assert_eq!(impact.references.len(), 1);
    assert_eq!(impact.references[0].state, ReferenceState::Stale);
    assert!(impact.references[0].clearable);
    assert!(!impact.can_delete);

    clear_provider_reference(
        &storage,
        ClearProviderReferenceRequest {
            provider_id: record.id.as_str().to_owned(),
            agent_id: "pi".into(),
            config_root: pi_root.to_string_lossy().into_owned(),
            label: "vibemate-Pi-test".into(),
        },
    )
    .expect("clear stale binding");

    delete_provider(&storage, &store, delete_request(record.id.as_str(), 1)).expect("delete");
    assert_eq!(table_rows(&storage, "provider_instance"), 0);
    assert_eq!(store.stored(&record.id.credential_reference()), None);
    assert_eq!(
        fs::read_to_string(pi_root.join("settings.json")).expect("read settings"),
        settings,
        "central deletion leaves the saved Pi thinking level untouched"
    );
}

#[test]
fn active_provider_bindings_cannot_be_cleared() {
    let directory = TestDirectory::new();
    let storage = directory.open();
    let store = FakeStore::default();
    let record = create_with_store(&storage, &store, "Active");
    let pi_root = directory.0.join("active-pi");
    fs::create_dir_all(&pi_root).expect("pi root");
    fs::write(
        pi_root.join("models.json"),
        r#"{"providers":{"vibemate-Pi-test":{"models":[]}}}"#,
    )
    .expect("pi models");

    storage
        .lock()
        .expect("lock")
        .execute(
            "INSERT INTO pi_provider_binding(config_root, provider_key, provider_id)
             VALUES (?1, 'vibemate-Pi-test', ?2)",
            params![pi_root.to_string_lossy(), record.id.as_str()],
        )
        .expect("pi binding");

    assert_eq!(
        clear_provider_reference(
            &storage,
            ClearProviderReferenceRequest {
                provider_id: record.id.as_str().to_owned(),
                agent_id: "pi".into(),
                config_root: pi_root.to_string_lossy().into_owned(),
                label: "vibemate-Pi-test".into(),
            },
        ),
        Err(ProviderError::ProviderInUse)
    );
    assert_eq!(table_rows(&storage, "pi_provider_binding"), 1);
}

#[test]
fn a_stale_revision_is_rejected_before_the_credential_store_is_touched() {
    let directory = TestDirectory::new();
    let storage = directory.open();
    let store = FakeStore::default();
    let record = create_with_store(&storage, &store, "Work");
    storage
        .lock()
        .expect("lock")
        .execute(
            "UPDATE provider_instance SET revision = 2 WHERE id = ?1",
            [record.id.as_str()],
        )
        .expect("bump revision");

    let before = store.call_count();
    assert_eq!(
        delete_provider(&storage, &store, delete_request(record.id.as_str(), 1)),
        Err(ProviderError::RevisionConflict)
    );
    assert_eq!(store.call_count(), before, "no credential-store access");
    assert_eq!(table_rows(&storage, "provider_instance"), 1);
    assert_eq!(
        store.stored(&record.id.credential_reference()).as_deref(),
        Some(KEY)
    );
}

#[test]
fn a_delete_store_failure_leaves_the_provider_and_its_key_intact() {
    let directory = TestDirectory::new();
    let storage = directory.open();
    let store = FakeStore::default();
    let record = create_with_store(&storage, &store, "Work");
    add_model(&storage, record.id.as_str(), "vendor/model");
    store.fail_deletes(Some(CredentialError::AccessDenied));

    assert_eq!(
        delete_provider(&storage, &store, delete_request(record.id.as_str(), 1)),
        Err(ProviderError::CredentialStoreAccessDenied)
    );
    assert_eq!(table_rows(&storage, "provider_instance"), 1);
    assert_eq!(table_rows(&storage, "provider_model"), 1);
    assert_eq!(
        store.stored(&record.id.credential_reference()).as_deref(),
        Some(KEY)
    );
}

#[test]
fn a_failed_delete_statement_restores_the_key_and_reports_write_failed() {
    let directory = TestDirectory::new();
    let storage = directory.open();
    let store = FakeStore::default();
    let record = create_with_store(&storage, &store, "Work");
    // SQLite rejects the DELETE, so the commit step never reaches COMMIT.
    storage
        .lock()
        .expect("lock")
        .pragma_update(None, "query_only", true)
        .expect("read-only");

    assert_eq!(
        delete_provider(&storage, &store, delete_request(record.id.as_str(), 1)),
        Err(ProviderError::WriteFailed)
    );
    storage
        .lock()
        .expect("lock")
        .pragma_update(None, "query_only", false)
        .expect("writable");
    assert_eq!(table_rows(&storage, "provider_instance"), 1);
    assert_eq!(table_rows(&storage, "provider_credential"), 1);
    assert_eq!(
        store.stored(&record.id.credential_reference()).as_deref(),
        Some(KEY),
        "the key was restored"
    );
}

#[test]
fn an_uncertain_delete_commit_restores_the_key_and_reports_unknown() {
    let directory = TestDirectory::new();
    let storage = directory.open();
    let store = FakeStore::default();
    let record = create_with_store(&storage, &store, "Work");
    fail_commits_after_provider_delete(&storage);

    assert_eq!(
        delete_provider(&storage, &store, delete_request(record.id.as_str(), 1)),
        Err(ProviderError::DeleteOutcomeUnknown)
    );
    // The row may or may not survive; the key is back either way.
    assert_eq!(
        store.stored(&record.id.credential_reference()).as_deref(),
        Some(KEY)
    );
    assert_eq!(table_rows(&storage, "provider_instance"), 1);
}

#[test]
fn an_unreadable_key_does_not_block_deletion() {
    let directory = TestDirectory::new();
    let storage = directory.open();
    let store = FakeStore::default();
    let record = create_with_store(&storage, &store, "Work");
    // Replacing the key is the only repair for an unreadable value; deleting it
    // must still work and must not attempt a restore.
    store.fail_loads(Some(CredentialError::CorruptValue));

    delete_provider(&storage, &store, delete_request(record.id.as_str(), 1)).expect("delete");
    assert_eq!(table_rows(&storage, "provider_instance"), 0);
    assert_eq!(store.stored(&record.id.credential_reference()), None);
}

#[test]
fn deleting_a_legacy_provider_without_a_reference_row_still_removes_its_key() {
    let directory = TestDirectory::new();
    let storage = directory.open();
    let id = "a".repeat(32);
    storage
        .lock()
        .expect("lock")
        .execute(
            "INSERT INTO provider_instance
                 (id, kind, display_name, base_url, protocol, revision, created_at, updated_at)
             VALUES (?1, 'deepseek', 'Legacy', 'https://api.deepseek.com',
                     'chat_completions', 1, 10, 10)",
            [&id],
        )
        .expect("insert legacy provider");
    let store = FakeStore::holding(&format!("provider-{id}"), KEY);

    delete_provider(&storage, &store, delete_request(&id, 1)).expect("delete");
    assert_eq!(table_rows(&storage, "provider_instance"), 0);
    assert_eq!(store.stored(&format!("provider-{id}")), None);
}
