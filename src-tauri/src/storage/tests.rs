//! Behavior tests for the parent module, kept separate from runtime code.

use super::*;
use std::path::PathBuf;
use std::sync::Barrier;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A unique folder under the system temp directory, removed when the test ends.
struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    /// Create a fresh folder. The process id and a counter keep parallel tests apart.
    fn new(name: &str) -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "vibemate-storage-{name}-{}-{id}",
            std::process::id()
        ));
        // Remove leftovers from an interrupted run so every test starts empty.
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create test folder");
        Self { path }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        // Cleanup is best effort so a removal problem cannot hide the test result.
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn effort_preference_migration_preserves_models_and_starts_without_an_inferred_choice() {
    let directory = TestDirectory::new("effort-preference-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    {
        let old = Storage::open_file(&path, &MIGRATIONS[..25]).unwrap();
        old.lock().unwrap().execute_batch("INSERT INTO provider_instance VALUES('0123456789abcdef0123456789abcdef','deepseek','Work','https://api.deepseek.com','chat_completions',1,1,1);
            INSERT INTO provider_model(provider_id,model_id,source,selected,thinking_effort,created_at,updated_at)
            VALUES('0123456789abcdef0123456789abcdef','deepseek-flash','manual',1,'{\"kind\":\"listed\",\"levels\":[\"high\"],\"defaultLevel\":\"high\"}',1,1);").unwrap();
    }
    let storage = Storage::open_in_directory(&directory.path).unwrap();
    assert_eq!(storage.schema_version().unwrap(), latest_schema_version());
    let connection = storage.lock().unwrap();
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM claude_effort_preference", [], |row| {
                row.get::<_, u32>(0)
            })
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row("SELECT thinking_effort FROM provider_model", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        r#"{"kind":"listed","levels":["high"],"defaultLevel":"high"}"#
    );
    assert!(connection.execute("INSERT INTO claude_effort_preference VALUES('0123456789abcdef0123456789abcdef','missing-model','0123456789abcdef0123456789abcdef','high')", []).is_err());
    assert!(connection.execute("INSERT INTO claude_effort_preference VALUES('0123456789abcdef0123456789abcdef','deepseek-flash','0123456789abcdef0123456789abcdef','none')", []).is_err());
}

#[test]
fn version_migration_keeps_old_journals_and_does_not_invent_snapshot_bytes() {
    let directory = TestDirectory::new("independent-versions-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let id = "0123456789abcdef0123456789abcdef";
    {
        let old = Storage::open_file(&path, &MIGRATIONS[..24]).unwrap();
        old.lock()
            .unwrap()
            .execute(
                "INSERT INTO config_operation(id, kind, state, created_at, updated_at)
             VALUES (?1, 'claude_code_settings', 'applied', 1, 1)",
                [id],
            )
            .unwrap();
    }
    let upgraded = Storage::open_in_directory(&directory.path).unwrap();
    let connection = upgraded.lock().unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT state FROM config_version WHERE id = ?1",
                [id],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "legacy"
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM config_version_file", [], |row| row
                .get::<_, u32>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT state FROM config_operation WHERE id = ?1",
                [id],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "applied"
    );
    assert!(connection.execute("INSERT INTO config_version_file VALUES (?1, 0, '/target', 'invalid-digest', 'copy', '/copy')", [id]).is_err());
}

#[test]
fn mcp_migration_preserves_provider_models_and_preferences() {
    let directory = TestDirectory::new("mcp-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    {
        let old = Storage::open_file(&path, &MIGRATIONS[..7]).expect("v7");
        old.lock().unwrap().execute_batch("INSERT INTO locale_preference VALUES(1,'zh-CN');
            INSERT INTO provider_instance VALUES('0123456789abcdef0123456789abcdef','deepseek','Work','https://api.deepseek.com','chat_completions',1,1,1);
            INSERT INTO provider_model(provider_id,model_id,source,selected,created_at,updated_at) VALUES('0123456789abcdef0123456789abcdef','vendor/model','manual',1,1,1);").unwrap();
    }
    let upgraded = Storage::open_in_directory(&directory.path).unwrap();
    assert_eq!(upgraded.schema_version().unwrap(), latest_schema_version());
    let connection = upgraded.lock().unwrap();
    assert_eq!(
        connection
            .query_row("SELECT preference FROM locale_preference", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "zh-CN"
    );
    assert_eq!(
        connection
            .query_row("SELECT model_id FROM provider_model", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "vendor/model"
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM mcp_definition", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn notion_migration_preserves_existing_preferences_and_supports_restart() {
    use crate::appearance::{Appearance, AppearancePreference, Theme};
    for theme in [
        Theme::Forest,
        Theme::Graphite,
        Theme::Linen,
        Theme::Iris,
        Theme::Ocean,
    ] {
        let directory = TestDirectory::new("notion-upgrade");
        let path = directory.path.join(DATABASE_FILE_NAME);
        let old = Storage::open_file(&path, &MIGRATIONS[..3]).expect("v3 database");
        let previous = AppearancePreference {
            appearance: Appearance::Dark,
            theme,
        };
        crate::appearance::save_appearance_preference(&old, previous).expect("save old pair");
        crate::settings::save_locale_preference(&old, crate::settings::LocalePreference::English)
            .expect("save language");
        old.lock()
            .expect("lock")
            .execute_batch("CREATE TABLE sample (value TEXT); INSERT INTO sample VALUES ('kept');")
            .expect("unrelated row");
        drop(old);
        let upgraded = Storage::open_in_directory(&directory.path).expect("upgrade");
        assert_eq!(
            upgraded.schema_version().expect("schema"),
            latest_version(MIGRATIONS)
        );
        assert_eq!(
            crate::appearance::load_appearance_preference(&upgraded),
            Ok(previous)
        );
        assert_eq!(
            crate::settings::load_locale_preference(&upgraded),
            Ok(crate::settings::LocalePreference::English)
        );
        assert_eq!(read_sample_value(&upgraded), "kept");
        let choice = AppearancePreference {
            appearance: Appearance::Dark,
            theme: Theme::Notion,
        };
        crate::appearance::save_appearance_preference(&upgraded, choice).expect("save Notion");
        assert!(
            upgraded
                .lock()
                .expect("lock")
                .execute("UPDATE appearance_preference SET theme = 'unknown'", [])
                .is_err()
        );
        drop(upgraded);
        let reopened = Storage::open_in_directory(&directory.path).expect("restart");
        assert_eq!(
            crate::appearance::load_appearance_preference(&reopened),
            Ok(choice)
        );
    }
}

#[test]
fn system_themes_migration_preserves_existing_preferences_and_supports_restart() {
    use crate::appearance::{Appearance, AppearancePreference, Theme};
    for theme in [
        Theme::Forest,
        Theme::Graphite,
        Theme::Linen,
        Theme::Iris,
        Theme::Ocean,
        Theme::Notion,
    ] {
        let directory = TestDirectory::new("system-themes-upgrade");
        let path = directory.path.join(DATABASE_FILE_NAME);
        let old = Storage::open_file(&path, &MIGRATIONS[..8]).expect("v8 database");
        let previous = AppearancePreference {
            appearance: Appearance::Light,
            theme,
        };
        crate::appearance::save_appearance_preference(&old, previous).expect("save old pair");
        crate::settings::save_locale_preference(&old, crate::settings::LocalePreference::English)
            .expect("save language");
        old.lock()
            .expect("lock")
            .execute_batch("CREATE TABLE sample (value TEXT); INSERT INTO sample VALUES ('kept');")
            .expect("unrelated row");
        drop(old);
        let upgraded = Storage::open_in_directory(&directory.path).expect("upgrade");
        assert_eq!(
            upgraded.schema_version().expect("schema"),
            latest_version(MIGRATIONS)
        );
        assert_eq!(
            crate::appearance::load_appearance_preference(&upgraded),
            Ok(previous)
        );
        assert_eq!(
            crate::settings::load_locale_preference(&upgraded),
            Ok(crate::settings::LocalePreference::English)
        );
        assert_eq!(read_sample_value(&upgraded), "kept");
        let choice = AppearancePreference {
            appearance: Appearance::Dark,
            theme: Theme::Macos,
        };
        crate::appearance::save_appearance_preference(&upgraded, choice).expect("save macos");
        let windows = AppearancePreference {
            appearance: Appearance::System,
            theme: Theme::Windows,
        };
        crate::appearance::save_appearance_preference(&upgraded, windows).expect("save windows");
        assert!(
            upgraded
                .lock()
                .expect("lock")
                .execute("UPDATE appearance_preference SET theme = 'unknown'", [])
                .is_err()
        );
        drop(upgraded);
        let reopened = Storage::open_in_directory(&directory.path).expect("restart");
        assert_eq!(
            crate::appearance::load_appearance_preference(&reopened),
            Ok(windows)
        );
    }
}

#[test]
fn capability_metadata_migration_keeps_saved_models_unknown_by_default() {
    let directory = TestDirectory::new("capability-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let old = Storage::open_file(&path, &MIGRATIONS[..11]).expect("v11 database");
    old.lock()
        .expect("lock")
        .execute_batch(
            "INSERT INTO provider_instance VALUES ('0123456789abcdef0123456789abcdef', \
             'deepseek', 'Work', 'https://api.deepseek.com', 'chat_completions', 1, 1, 1); \
             INSERT INTO provider_model(provider_id, model_id, source, selected, alias, \
             created_at, updated_at, last_seen_at) VALUES \
             ('0123456789abcdef0123456789abcdef', 'deepseek-flash', 'fetched', 1, 'Mine', 1, 1, 1);",
        )
        .expect("v11 row");
    drop(old);

    let upgraded = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert_eq!(
        upgraded.schema_version().expect("version"),
        latest_schema_version()
    );
    let capabilities: String = upgraded
        .lock()
        .expect("lock")
        .query_row(
            "SELECT capabilities FROM provider_model WHERE model_id = 'deepseek-flash'",
            [],
            |row| row.get(0),
        )
        .expect("kept row");
    let parsed: serde_json::Value = serde_json::from_str(&capabilities).expect("valid JSON");
    assert_eq!(parsed["thinking"]["state"], "unknown");
    assert_eq!(parsed["imageInput"]["state"], "unknown");
    assert_eq!(parsed["toolCalls"]["state"], "unknown");
    // A definitive state without a source is JSON-valid at the storage layer, but
    // the model reader rejects it as `invalid_stored_model` instead of trusting it.
    upgraded
        .lock()
        .expect("lock")
        .execute(
            "UPDATE provider_model SET capabilities = '{\"thinking\":{\"state\":\"supported\",\"source\":null},\"imageInput\":{\"state\":\"unknown\",\"source\":null},\"toolCalls\":{\"state\":\"unknown\",\"source\":null}}'",
            [],
        )
        .expect("paired CHECK stays at JSON validity");
    let read = crate::models::list_provider_models(
        &upgraded,
        &crate::models::ListProviderModelsRequest {
            provider_id: "0123456789abcdef0123456789abcdef".to_owned(),
            after: None,
            limit: 10,
            filter: "all".to_owned(),
            query: None,
        },
    );
    assert_eq!(read, Err(crate::models::ModelError::InvalidStoredModel));
}

#[test]
fn capability_migration_runs_on_a_database_already_at_schema_12() {
    let directory = TestDirectory::new("capability-shared-v12");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let old = Storage::open_file(&path, &MIGRATIONS[..11]).expect("v11 database");
    // Reproduce a database the Agent auto-probe branch already upgraded to 12:
    // it holds the v12 preference table and has no capabilities column. The
    // capabilities migration must still run for it.
    old.lock()
        .expect("lock")
        .execute_batch(
            "INSERT INTO provider_instance VALUES ('0123456789abcdef0123456789abcdef', \
             'deepseek', 'Work', 'https://api.deepseek.com', 'chat_completions', 1, 1, 1); \
             INSERT INTO provider_model(provider_id, model_id, source, selected, \
             created_at, updated_at) VALUES \
             ('0123456789abcdef0123456789abcdef', 'deepseek-flash', 'fetched', 1, 1, 1); \
             CREATE TABLE agent_version_preference (\
                 id INTEGER PRIMARY KEY CHECK (id = 1),\
                 auto_probe INTEGER NOT NULL CHECK (auto_probe IN (0, 1))\
             ) STRICT; \
             PRAGMA user_version = 12;",
        )
        .expect("v12 database from the Agent auto-probe branch");
    drop(old);

    let upgraded = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert_eq!(
        upgraded.schema_version().expect("version"),
        latest_schema_version()
    );
    let capabilities: String = upgraded
        .lock()
        .expect("lock")
        .query_row(
            "SELECT capabilities FROM provider_model WHERE model_id = 'deepseek-flash'",
            [],
            |row| row.get(0),
        )
        .expect("capabilities column added");
    let parsed: serde_json::Value = serde_json::from_str(&capabilities).expect("valid JSON");
    assert_eq!(parsed["thinking"]["state"], "unknown");
    // The other worktree's table survives the upgrade.
    let version_rows: i64 = upgraded
        .lock()
        .expect("lock")
        .query_row("SELECT COUNT(*) FROM agent_version_preference", [], |row| {
            row.get(0)
        })
        .expect("other worktree's table");
    assert_eq!(version_rows, 0);
}

#[test]
fn capability_override_migration_keeps_rows_and_starts_without_declarations() {
    let directory = TestDirectory::new("capability-overrides-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let old = Storage::open_file(&path, &MIGRATIONS[..13]).expect("v13 database");
    old.lock()
        .expect("lock")
        .execute_batch(
            "INSERT INTO provider_instance VALUES ('0123456789abcdef0123456789abcdef', \
             'deepseek', 'Work', 'https://api.deepseek.com', 'chat_completions', 1, 1, 1); \
             INSERT INTO provider_model(provider_id, model_id, source, selected, alias, \
             created_at, updated_at, last_seen_at) VALUES \
             ('0123456789abcdef0123456789abcdef', 'deepseek-flash', 'fetched', 1, 'Mine', 1, 1, 1);",
        )
        .expect("v13 rows");
    drop(old);

    let upgraded = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert_eq!(
        upgraded.schema_version().expect("version"),
        latest_schema_version()
    );
    // Existing rows keep their user labels and have no capability declarations.
    let (alias, overrides): (String, Option<String>) = upgraded
        .lock()
        .expect("lock")
        .query_row(
            "SELECT alias, capability_overrides FROM provider_model \
             WHERE model_id = 'deepseek-flash'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("kept row");
    assert_eq!(alias, "Mine");
    assert_eq!(overrides, None);

    // The CHECK constraint rejects broken JSON at the storage layer...
    let broken = upgraded.lock().expect("lock").execute(
        "UPDATE provider_model SET capability_overrides = 'not json' \
         WHERE model_id = 'deepseek-flash'",
        [],
    );
    assert!(broken.is_err());

    // ...while JSON-valid text with an unexpected shape is the reader's problem:
    // it rejects the row as `invalid_stored_model` instead of trusting it.
    upgraded
        .lock()
        .expect("lock")
        .execute(
            "UPDATE provider_model SET capability_overrides = '{\"thinking\":\"sometimes\"}' \
             WHERE model_id = 'deepseek-flash'",
            [],
        )
        .expect("JSON-valid shape");
    let read = crate::models::list_provider_models(
        &upgraded,
        &crate::models::ListProviderModelsRequest {
            provider_id: "0123456789abcdef0123456789abcdef".to_owned(),
            after: None,
            limit: 10,
            filter: "all".to_owned(),
            query: None,
        },
    );
    assert_eq!(read, Err(crate::models::ModelError::InvalidStoredModel));
}

#[test]
fn raising_skill_limits_keeps_rows_and_accepts_larger_skills() {
    let directory = TestDirectory::new("skill-limits-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let old = Storage::open_file(&path, &MIGRATIONS[..10]).expect("v10 database");
    old.lock()
        .expect("lock")
        .execute_batch(
            "INSERT INTO skill
             (id, name, description, origin_path, source_url, source_revision,
              content_digest, file_count, byte_size, revision, imported_at, updated_at)
             VALUES ('0123456789abcdef0123456789abcdef', 'demo', 'Demo skill.', \
             '/tmp/demo', NULL, NULL, 'abababababababababababababababababababababababababababababababab', \
             2, 128, 1, 1000, 1000);",
        )
        .expect("v10 row");
    drop(old);

    let upgraded = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert_eq!(
        upgraded.schema_version().expect("version"),
        latest_schema_version()
    );
    let connection = upgraded.lock().expect("lock");
    // The existing row survives the rebuild unchanged.
    let row: (String, i64, i64) = connection
        .query_row(
            "SELECT name, file_count, byte_size FROM skill WHERE id = '0123456789abcdef0123456789abcdef'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("kept row");
    assert_eq!(row, ("demo".to_string(), 2, 128));
    // A skill far larger than the original 8 MiB bound is accepted now, and the
    // old bound still rejects what remains outside the new limits.
    connection
        .execute(
            "INSERT INTO skill
             (id, name, description, origin_path, source_url, source_revision,
              content_digest, file_count, byte_size, revision, imported_at, updated_at)
             VALUES ('fedcba9876543210fedcba9876543210', 'large', 'Large skill.', \
             '/tmp/large', NULL, NULL, 'cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd', \
             315, 11534336, 1, 1000, 1000);",
            [],
        )
        .expect("larger row");
    assert!(
        connection
            .execute(
                "INSERT INTO skill
                 (id, name, description, origin_path, source_url, source_revision,
                  content_digest, file_count, byte_size, revision, imported_at, updated_at)
                 VALUES ('00112233445566778899001122334455', 'huge', 'Too large.', \
                 '/tmp/huge', NULL, NULL, 'efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef', \
                 1, 67108865, 1, 1000, 1000);",
                [],
            )
            .is_err()
    );
}

#[test]
fn failed_system_themes_migration_keeps_the_v8_table_and_data() {
    let directory = TestDirectory::new("system-themes-failed-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let old = Storage::open_file(&path, &MIGRATIONS[..8]).expect("v8 database");
    old.lock()
        .expect("lock")
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON; \
             INSERT INTO appearance_preference VALUES (1, 'dark', 'unknown');",
        )
        .expect("corrupt v8 fixture");
    drop(old);
    assert!(matches!(
        Storage::open_in_directory(&directory.path),
        Err(StorageError::MigrationFailed { version: 9, .. })
    ));
    let raw = Connection::open(&path).expect("original database");
    assert_eq!(read_schema_version(&raw).expect("schema"), 8);
    let theme: String = raw
        .query_row("SELECT theme FROM appearance_preference", [], |row| {
            row.get(0)
        })
        .expect("original row");
    assert_eq!(theme, "unknown");
    let temporary_tables: u32 = raw
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'appearance_preference_v9'",
            [],
            |row| row.get(0),
        )
        .expect("rolled back table");
    assert_eq!(temporary_tables, 0);
}

#[test]
fn failed_notion_migration_keeps_the_v3_table_and_data() {
    let directory = TestDirectory::new("notion-failed-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let old = Storage::open_file(&path, &MIGRATIONS[..3]).expect("v3 database");
    old.lock().expect("lock").execute_batch("PRAGMA ignore_check_constraints = ON; INSERT INTO appearance_preference VALUES (1, 'dark', 'unknown');").expect("corrupt v3 fixture");
    drop(old);
    assert!(matches!(
        Storage::open_in_directory(&directory.path),
        Err(StorageError::MigrationFailed { version: 4, .. })
    ));
    let raw = Connection::open(&path).expect("original database");
    assert_eq!(read_schema_version(&raw).expect("schema"), 3);
    let theme: String = raw
        .query_row("SELECT theme FROM appearance_preference", [], |row| {
            row.get(0)
        })
        .expect("original row");
    assert_eq!(theme, "unknown");
    let temporary_tables: u32 = raw
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'appearance_preference_v4'",
            [],
            |row| row.get(0),
        )
        .expect("rolled back table");
    assert_eq!(temporary_tables, 0);
}

#[test]
fn providers_migration_from_v4_keeps_preferences_and_adds_an_empty_table() {
    use crate::appearance::{Appearance, AppearancePreference, Theme};
    let directory = TestDirectory::new("providers-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let old = Storage::open_file(&path, &MIGRATIONS[..4]).expect("v4 database");
    let appearance = AppearancePreference {
        appearance: Appearance::Light,
        theme: Theme::Notion,
    };
    crate::appearance::save_appearance_preference(&old, appearance).expect("save appearance");
    crate::settings::save_locale_preference(
        &old,
        crate::settings::LocalePreference::SimplifiedChinese,
    )
    .expect("save language");
    drop(old);

    // Stop at v5 so this test keeps checking exactly the v4 -> v5 step.
    let upgraded = Storage::open_file(&path, &MIGRATIONS[..5]).expect("upgrade to v5");
    assert_eq!(upgraded.schema_version().expect("schema"), 5);
    assert_eq!(
        crate::appearance::load_appearance_preference(&upgraded),
        Ok(appearance)
    );
    assert_eq!(
        crate::settings::load_locale_preference(&upgraded),
        Ok(crate::settings::LocalePreference::SimplifiedChinese)
    );
    drop(upgraded);
    // Selected-model counts need the v6 `provider_model` table; finish migrating before listing.
    let current = Storage::open_file(&path, MIGRATIONS).expect("upgrade to current");
    let page = crate::providers::list_providers(
        &current,
        &crate::providers::ListProvidersRequest {
            after: None,
            limit: 10,
        },
    )
    .expect("list after upgrade");
    assert!(page.items.is_empty());
}

/// Insert a provider row directly, as a P10 build without keys would have saved it.
fn insert_keyless_provider(connection: &Connection, id: &str) {
    connection
        .execute(
            "INSERT INTO provider_instance
                 (id, kind, display_name, base_url, protocol, revision, created_at, updated_at)
             VALUES (?1, 'deepseek', 'Legacy', 'https://api.deepseek.com', 'chat_completions', 1, 10, 10)",
            [id],
        )
        .expect("insert provider");
}

#[test]
fn credential_migration_from_v5_keeps_providers_and_preferences() {
    let directory = TestDirectory::new("credential-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let old = Storage::open_file(&path, &MIGRATIONS[..5]).expect("v5 database");
    crate::settings::save_locale_preference(&old, crate::settings::LocalePreference::English)
        .expect("save language");
    let legacy_id = "a".repeat(32);
    insert_keyless_provider(&old.lock().expect("lock"), &legacy_id);
    drop(old);

    let upgraded = Storage::open_file(&path, &MIGRATIONS[..6]).expect("upgrade to v6");
    assert_eq!(upgraded.schema_version().expect("schema"), 6);
    assert_eq!(
        crate::settings::load_locale_preference(&upgraded),
        Ok(crate::settings::LocalePreference::English)
    );
    let connection = upgraded.lock().expect("lock");
    let providers: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM provider_instance WHERE id = ?1",
            [&legacy_id],
            |row| row.get(0),
        )
        .expect("count providers");
    assert_eq!(providers, 1, "the keyless legacy row is kept");
    let credentials: u32 = connection
        .query_row("SELECT COUNT(*) FROM provider_credential", [], |row| {
            row.get(0)
        })
        .expect("count credentials");
    assert_eq!(credentials, 0, "no reference is invented for it");
}

#[test]
fn credential_references_are_tied_to_existing_provider_ids() {
    let directory = TestDirectory::new("credential-constraints");
    let storage = Storage::open_in_directory(&directory.path).expect("open");
    let connection = storage.lock().expect("lock");
    let id = "b".repeat(32);
    let other = "c".repeat(32);
    insert_keyless_provider(&connection, &id);
    insert_keyless_provider(&connection, &other);
    let insert = |provider_id: &str, reference: &str| {
        connection.execute(
            "INSERT INTO provider_credential (provider_id, credential_ref, updated_at)
             VALUES (?1, ?2, 20)",
            [provider_id, reference],
        )
    };

    // A reference must be exactly `provider-<id>`: not a name, not another ID.
    assert!(insert(&id, "Legacy").is_err());
    assert!(insert(&id, &format!("provider-{other}")).is_err());
    // The provider must exist (foreign key).
    let missing = "d".repeat(32);
    assert!(insert(&missing, &format!("provider-{missing}")).is_err());
    assert!(insert(&id, &format!("provider-{id}")).is_ok());
    // One reference per provider.
    assert!(insert(&id, &format!("provider-{id}")).is_err());

    // Deleting the provider removes its reference with it.
    connection
        .execute("DELETE FROM provider_instance WHERE id = ?1", [&id])
        .expect("delete provider");
    let remaining: u32 = connection
        .query_row("SELECT COUNT(*) FROM provider_credential", [], |row| {
            row.get(0)
        })
        .expect("count credentials");
    assert_eq!(remaining, 0);
}

#[test]
fn model_migration_from_v6_keeps_providers_and_key_references() {
    let directory = TestDirectory::new("model-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let old = Storage::open_file(&path, &MIGRATIONS[..6]).expect("v6 database");
    let id = "e".repeat(32);
    {
        let connection = old.lock().expect("lock");
        insert_keyless_provider(&connection, &id);
        connection
            .execute(
                "INSERT INTO provider_credential (provider_id, credential_ref, updated_at)
                 VALUES (?1, 'provider-' || ?1, 20)",
                [&id],
            )
            .expect("insert reference");
    }
    drop(old);

    let upgraded = Storage::open_file(&path, &MIGRATIONS[..7]).expect("upgrade to v7");
    assert_eq!(upgraded.schema_version().expect("schema"), 7);
    let connection = upgraded.lock().expect("lock");
    let count = |table: &str| -> u32 {
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count rows")
    };
    assert_eq!(count("provider_instance"), 1);
    assert_eq!(count("provider_credential"), 1);
    assert_eq!(count("provider_model"), 0);
    assert_eq!(count("provider_model_fetch"), 0);
}

#[test]
fn model_rows_are_constrained_and_follow_their_provider() {
    let directory = TestDirectory::new("model-constraints");
    let storage = Storage::open_in_directory(&directory.path).expect("open");
    let connection = storage.lock().expect("lock");
    let id = "f".repeat(32);
    insert_keyless_provider(&connection, &id);
    let insert = |provider_id: &str, model_id: &str, source: &str, endpoints: Option<&str>| {
        connection.execute(
            "INSERT INTO provider_model
                 (provider_id, model_id, source, selected, supported_endpoints,
                  created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4, 10, 10)",
            rusqlite::params![provider_id, model_id, source, endpoints],
        )
    };

    // IDs keep `/`, `.` and `:`; the JSON column must hold valid JSON.
    assert!(
        insert(
            &id,
            "mistral/mistral-large-4",
            "fetched",
            Some(r#"["/messages"]"#)
        )
        .is_ok()
    );
    assert!(insert(&id, "ling-3.1-flash:free", "manual", None).is_ok());
    assert!(insert(&id, "bad-json", "fetched", Some("[")).is_err());
    assert!(insert(&id, "bad-source", "imported", None).is_err());
    assert!(insert(&id, "", "manual", None).is_err());
    assert!(insert(&id, &"m".repeat(257), "manual", None).is_err());
    // One row per (provider, model); the provider must exist.
    assert!(insert(&id, "ling-3.1-flash:free", "fetched", None).is_err());
    assert!(insert(&"0".repeat(32), "other", "manual", None).is_err());
    // A model cannot be missing without having been seen.
    assert!(
        connection
            .execute(
                "INSERT INTO provider_model
                     (provider_id, model_id, source, selected, created_at, updated_at,
                      missing_since)
                 VALUES (?1, 'never-seen', 'fetched', 1, 10, 10, 20)",
                [&id],
            )
            .is_err()
    );
    connection
        .execute(
            "INSERT INTO provider_model_fetch (provider_id, fetched_at, complete, listed_count)
             VALUES (?1, 30, 1, 2)",
            [&id],
        )
        .expect("insert fetch record");

    connection
        .execute("DELETE FROM provider_instance WHERE id = ?1", [&id])
        .expect("delete provider");
    for table in ["provider_model", "provider_model_fetch"] {
        let remaining: u32 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count rows");
        assert_eq!(remaining, 0, "{table} follows its provider");
    }
}

#[test]
fn resolution_migration_preserves_v20_snapshots_without_inventing_resolutions() {
    use crate::config_apply::{
        OperationKind, OperationState, WriteTarget, begin_operation, finish_operation, operation,
        record_step_applied,
    };
    let directory = TestDirectory::new("resolution-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let old = Storage::open_file(&path, &MIGRATIONS[..20]).unwrap();
    let target = directory.path.join("settings.json");
    std::fs::write(&target, b"original").unwrap();
    // The current writer knows the v22 `direct` column, so this pre-v22 fixture
    // records the operation and its snapshot reference with the old shape.
    let operation_id = begin_operation(
        &old,
        OperationKind::PiSettings,
        None,
        &[WriteTarget {
            path: target.clone(),
            expected_digest: Some(crate::config_plan::digest_hex(b"original")),
        }],
    )
    .unwrap();
    record_step_applied(
        &old,
        &operation_id,
        0,
        None,
        &crate::config_plan::digest_hex(b"changed"),
        true,
    )
    .unwrap();
    finish_operation(&old, &operation_id, OperationState::Applied).unwrap();
    old.lock()
        .unwrap()
        .execute(
            "INSERT INTO config_file_revision
             (id, batch_id, operation_id, ordinal, intent, target_path,
              previous_digest, next_digest, backup_path, state, error_code,
              created_at, updated_at)
             VALUES ('v20-revision', ?1, ?1, 0, 'apply', ?2, ?3, ?4, ?5,
                     'completed', NULL, 1, 1)",
            rusqlite::params![
                operation_id,
                target.to_string_lossy(),
                crate::config_plan::digest_hex(b"original"),
                crate::config_plan::digest_hex(b"changed"),
                directory
                    .path
                    .join("backups/v20-revision/settings.json")
                    .to_string_lossy()
            ],
        )
        .unwrap();
    std::fs::write(&target, b"changed").unwrap();
    let before = operation(&old, &operation_id).unwrap().unwrap();
    drop(old);
    let upgraded = Storage::open_in_directory(&directory.path).unwrap();
    assert_eq!(
        operation(&upgraded, &operation_id).unwrap().unwrap(),
        before
    );
    let connection = upgraded.lock().unwrap();
    let (state, digest): (String, String) = connection
        .query_row(
            "SELECT state, previous_digest FROM config_file_revision",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, "completed");
    assert_eq!(digest, crate::config_plan::digest_hex(b"original"));
    let direct: i64 = connection
        .query_row("SELECT direct FROM config_file_revision", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(direct, 0, "legacy rows are not direct version restores");
    let count: i64 = connection
        .query_row(
            "SELECT count(*) FROM config_revision_resolution",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(std::fs::read(target).unwrap(), b"changed");
}

#[test]
fn revision_migration_preserves_legacy_operations_without_fabricating_snapshots() {
    use crate::config_apply::{
        OperationKind, OperationState, StepState, WriteTarget, begin_operation, finish_operation,
        operation, record_step_applied,
    };
    let directory = TestDirectory::new("revision-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let old = Storage::open_file(&path, &MIGRATIONS[..19]).expect("v19 database");
    let target = WriteTarget {
        path: directory.path.join("settings.json"),
        expected_digest: None,
    };
    let id = begin_operation(&old, OperationKind::ClaudeCodeSettings, None, &[target]).unwrap();
    record_step_applied(&old, &id, 0, None, &"a".repeat(64), false).unwrap();
    finish_operation(&old, &id, OperationState::Applied).unwrap();
    drop(old);

    let upgraded = Storage::open_in_directory(&directory.path).unwrap();
    let record = operation(&upgraded, &id).unwrap().unwrap();
    assert_eq!(record.state, OperationState::Applied);
    assert_eq!(record.steps[0].state, StepState::Applied);
    assert_eq!(record.steps[0].previous_existed, Some(false));
    assert_eq!(record.steps[0].restore_state, None);
    let count: i64 = upgraded
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM config_file_revision", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    drop(upgraded);
    let reopened = Storage::open_in_directory(&directory.path).unwrap();
    assert_eq!(operation(&reopened, &id).unwrap().unwrap(), record);
}

#[test]
fn operation_log_migration_keeps_earlier_rows_and_starts_empty() {
    let directory = TestDirectory::new("operation-log-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let old = Storage::open_file(&path, &MIGRATIONS[..15]).expect("v15 database");
    old.lock()
        .expect("lock")
        .execute_batch(
            "INSERT INTO provider_instance VALUES \
             ('0123456789abcdef0123456789abcdef', 'deepseek', 'Work', \
              'https://api.deepseek.com', 'chat_completions', 1, 1, 1);",
        )
        .expect("v15 row");
    drop(old);

    let upgraded = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert_eq!(
        upgraded.schema_version().expect("version"),
        latest_schema_version()
    );
    let providers: i64 = upgraded
        .lock()
        .expect("lock")
        .query_row("SELECT COUNT(*) FROM provider_instance", [], |row| {
            row.get(0)
        })
        .expect("kept row");
    assert_eq!(providers, 1);
    // A freshly migrated database has no unfinished write to report.
    assert!(
        crate::config_apply::unfinished_operations(&upgraded)
            .expect("read log")
            .is_empty()
    );
}

#[test]
fn restore_columns_migrate_an_older_operation_log_without_guessing() {
    let directory = TestDirectory::new("operation-restore-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let old = Storage::open_file(&path, &MIGRATIONS[..16]).expect("v16 database");
    let operation_id = "c".repeat(32);
    // The paths come from this test's own directory because the log reader only
    // accepts a path that is absolute on the platform running the test: the
    // same paths are produced by the writer. A hardcoded "/tmp/..." literal has
    // no prefix on Windows and would be refused as a malformed row.
    let settings = directory.path.join("settings.json");
    let models = directory.path.join("models.json");
    let connection = old.lock().expect("lock");
    connection
        .execute(
            "INSERT INTO config_operation (id, kind, state, created_at, updated_at)
             VALUES (?1, 'claude_code_settings', 'planned', 10, 10)",
            [&operation_id],
        )
        .expect("v16 operation");
    connection
        .execute(
            "INSERT INTO config_operation_step
                 (operation_id, ordinal, target_path, state, applied_digest, updated_at)
             VALUES (?1, 0, ?2, 'applied',
                 'abababababababababababababababababababababababababababababababab', 10)",
            rusqlite::params![
                operation_id,
                settings.to_str().expect("readable settings path")
            ],
        )
        .expect("v16 applied step");
    connection
        .execute(
            "INSERT INTO config_operation_step
                 (operation_id, ordinal, target_path, state, updated_at)
             VALUES (?1, 1, ?2, 'pending', 10)",
            rusqlite::params![operation_id, models.to_str().expect("readable models path")],
        )
        .expect("v16 pending step");
    drop(connection);
    drop(old);

    let upgraded = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert_eq!(
        upgraded.schema_version().expect("version"),
        latest_schema_version()
    );
    // An older write has no owner, no rollback result and no statement about
    // whether its file existed: NULL means "not recorded", so the reader reports
    // exactly that instead of inventing a value.
    let record = crate::config_apply::operation(&upgraded, &operation_id)
        .expect("read log")
        .expect("kept operation");
    assert_eq!(record.owner_ref, None);
    assert_eq!(record.steps.len(), 2);
    assert_eq!(
        record.steps[0].state,
        crate::config_apply::StepState::Applied
    );
    assert_eq!(record.steps[0].restore_state, None);
    assert_eq!(record.steps[0].previous_existed, None);
    assert_eq!(
        record.steps[1].state,
        crate::config_apply::StepState::Pending
    );
    assert_eq!(record.steps[1].restore_state, None);
    assert_eq!(record.steps[1].previous_existed, None);
}

#[test]
fn grok_binding_migration_keeps_pi_aliases_and_scopes_each_root() {
    let directory = TestDirectory::new("grok-binding-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let provider = "0123456789abcdef0123456789abcdef";
    let old = Storage::open_file(&path, &MIGRATIONS[..18]).expect("v18 database");
    old.lock()
        .expect("lock")
        .execute(
            "INSERT INTO pi_provider_binding(config_root,provider_key,provider_id) VALUES (?1,?2,?3)",
            rusqlite::params!["/test/pi", "vibemate-DeepSeek", provider],
        )
        .expect("v18 Pi alias");
    drop(old);

    let upgraded = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert_eq!(
        upgraded.schema_version().expect("version"),
        latest_schema_version()
    );
    let connection = upgraded.lock().expect("lock");
    let kept: i64 = connection
        .query_row("SELECT COUNT(*) FROM pi_provider_binding", [], |row| {
            row.get(0)
        })
        .expect("Pi binding");
    assert_eq!(kept, 1);
    // A freshly migrated database owns no Grok alias: an entry in the user's
    // `config.toml` is theirs until a recorded write says otherwise.
    let empty: i64 = connection
        .query_row("SELECT COUNT(*) FROM grok_model_binding", [], |row| {
            row.get(0)
        })
        .expect("Grok bindings");
    assert_eq!(empty, 0);

    let insert = |root: &str, alias: &str, id: &str, model: &str| {
        connection.execute(
            "INSERT INTO grok_model_binding(config_root,model_alias,provider_id,model_id) VALUES (?1,?2,?3,?4)",
            rusqlite::params![root, alias, id, model],
        )
    };
    insert(
        "/test/grok",
        "vibemate-deepseek-flash",
        provider,
        "deepseek",
    )
    .expect("recorded alias");
    // Ownership is per configuration root, because `GROK_HOME` can point another
    // install at a different file with the same alias inside it.
    insert(
        "/other/grok",
        "vibemate-deepseek-flash",
        provider,
        "deepseek",
    )
    .expect("same alias in another root");
    assert!(
        insert(
            "/test/grok",
            "vibemate-deepseek-flash",
            provider,
            "other-model"
        )
        .is_err(),
        "one alias cannot name two saved models"
    );
    assert!(insert("/test/grok", "vibemate-x", "not-an-id", "m").is_err());
    assert!(insert("/test/grok", "", provider, "m").is_err());
    assert!(insert("/test/grok", "vibemate-x", provider, "").is_err());
}

#[test]
fn operation_rows_are_constrained_and_follow_their_operation() {
    let directory = TestDirectory::new("operation-constraints");
    let storage = Storage::open_in_directory(&directory.path).expect("open");
    let connection = storage.lock().expect("lock");
    let id = "a".repeat(32);
    connection
        .execute(
            "INSERT INTO config_operation (id, kind, state, created_at, updated_at)
             VALUES (?1, 'claude_code_settings', 'planned', 10, 10)",
            [&id],
        )
        .expect("operation row");
    let step = |operation_id: &str, ordinal: i64, state: &str| {
        connection.execute(
            "INSERT INTO config_operation_step
                 (operation_id, ordinal, target_path, state, updated_at)
             VALUES (?1, ?2, '/tmp/settings.json', ?3, 10)",
            rusqlite::params![operation_id, ordinal, state],
        )
    };
    assert!(step(&id, 0, "pending").is_ok());
    assert!(step(&id, 1, "pending").is_ok());
    // One row per ordinal, known states only, and the operation must exist.
    assert!(step(&id, 0, "pending").is_err());
    assert!(step(&id, 1, "half-applied").is_err());
    assert!(step(&"b".repeat(32), 0, "pending").is_err());
    // The operation table owns its own closed set of states.
    assert!(
        connection
            .execute(
                "UPDATE config_operation SET state = 'running' WHERE id = ?1",
                [&id]
            )
            .is_err()
    );
    // A step that did not fail cannot carry a failure code, an applied digest
    // belongs to an applied step, and the backup columns appear together.
    let update = |sql: &str| connection.execute(sql, [&id]);
    assert!(
        update(
            "UPDATE config_operation_step SET error_code = 'target_changed' WHERE operation_id = ?1"
        )
        .is_err()
    );
    assert!(
        update(
            "UPDATE config_operation_step SET applied_digest = \
             'abababababababababababababababababababababababababababababababab' \
             WHERE operation_id = ?1"
        )
        .is_err()
    );
    assert!(
        update("UPDATE config_operation_step SET backup_id = '1700000000000-1-0' WHERE operation_id = ?1")
            .is_err()
    );
    // A rollback result belongs to an applied step and is one of the known
    // values; it is stored apart from the failure code above.
    let second_step = "UPDATE config_operation_step SET restore_state = 'restored'
         WHERE operation_id = ?1 AND ordinal = 1";
    assert!(connection.execute(second_step, [&id]).is_err());
    connection
        .execute(
            "UPDATE config_operation_step SET state = 'applied', applied_digest = \
             'abababababababababababababababababababababababababababababababab' \
             WHERE operation_id = ?1 AND ordinal = 0",
            [&id],
        )
        .expect("applied step");
    let first_step = |sql: &str| connection.execute(sql, [&id]);
    assert!(
        first_step(
            "UPDATE config_operation_step SET restore_state = 'restored'\n             WHERE operation_id = ?1 AND ordinal = 0"
        )
        .is_ok()
    );
    assert!(
        first_step(
            "UPDATE config_operation_step SET restore_state = 'half-done'\n             WHERE operation_id = ?1 AND ordinal = 0"
        )
        .is_err()
    );
    assert!(
        first_step(
            "UPDATE config_operation_step SET restore_state = NULL\n             WHERE operation_id = ?1 AND ordinal = 0"
        )
        .is_ok()
    );
    // Whether the file existed before the step is stated the same way: only an
    // applied step may carry it, and only as one of its two values.
    assert!(
        connection
            .execute(
                "UPDATE config_operation_step SET previous_existed = 1\n                 WHERE operation_id = ?1 AND ordinal = 1",
                [&id],
            )
            .is_err()
    );
    let first_existed = |sql: &str| connection.execute(sql, [&id]);
    assert!(
        first_existed(
            "UPDATE config_operation_step SET previous_existed = 1\n             WHERE operation_id = ?1 AND ordinal = 0"
        )
        .is_ok()
    );
    assert!(
        first_existed(
            "UPDATE config_operation_step SET previous_existed = 0\n             WHERE operation_id = ?1 AND ordinal = 0"
        )
        .is_ok()
    );
    assert!(
        first_existed(
            "UPDATE config_operation_step SET previous_existed = 7\n             WHERE operation_id = ?1 AND ordinal = 0"
        )
        .is_err()
    );
    // An operation owner is a bounded printable reference, never a display name
    // that a later rename would rewrite.
    let owner = |sql: &str| connection.execute(sql, [&id]);
    assert!(
        owner("UPDATE config_operation SET owner_ref = '0123456789abcdef' WHERE id = ?1").is_ok()
    );
    assert!(owner("UPDATE config_operation SET owner_ref = '' WHERE id = ?1").is_err());
    assert!(owner("UPDATE config_operation SET owner_ref = char(1) || 'x' WHERE id = ?1").is_err());
    assert!(
        owner("UPDATE config_operation SET owner_ref = 'x' WHERE id = ?1").is_ok(),
        "a short reference is valid"
    );
    // Deleting the operation removes its steps with it.
    connection
        .execute("DELETE FROM config_operation WHERE id = ?1", [&id])
        .expect("delete operation");
    let steps: i64 = connection
        .query_row("SELECT COUNT(*) FROM config_operation_step", [], |row| {
            row.get(0)
        })
        .expect("count steps");
    assert_eq!(steps, 0);
}

/// Read the single row written by the test migrations.
fn read_sample_value(storage: &Storage) -> String {
    let connection = storage.lock().expect("lock test connection");
    connection
        .query_row("SELECT value FROM sample", [], |row| row.get(0))
        .expect("read sample row")
}

#[test]
fn first_open_creates_the_folder_and_database_at_the_latest_version() {
    let directory = TestDirectory::new("first-open");
    let app_data = directory.path.join("app-data");

    let storage = Storage::open_in_directory(&app_data).expect("first open");

    assert!(app_data.join(DATABASE_FILE_NAME).is_file());
    assert_eq!(
        storage.schema_version().expect("read version"),
        latest_version(MIGRATIONS)
    );
}

#[test]
fn reopening_keeps_rows_and_skips_applied_migrations() {
    let directory = TestDirectory::new("reopen");
    let path = directory.path.join(DATABASE_FILE_NAME);
    // Running this SQL a second time would fail, so a repeated run would be visible.
    let migrations = [Migration {
        version: 1,
        sql: "CREATE TABLE sample (value TEXT NOT NULL); INSERT INTO sample (value) VALUES ('kept');",
    }];

    Storage::open_file(&path, &migrations).expect("first open");
    let storage = Storage::open_file(&path, &migrations).expect("second open must skip version 1");

    assert_eq!(storage.schema_version().expect("read version"), 1);
    assert_eq!(read_sample_value(&storage), "kept");
}

#[test]
fn failed_migration_rolls_back_and_keeps_previous_data_readable() {
    let directory = TestDirectory::new("failed-migration");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let first = Migration {
        version: 1,
        sql: "CREATE TABLE sample (value TEXT NOT NULL); INSERT INTO sample (value) VALUES ('kept');",
    };
    // The first statement is valid and the second fails. The rollback must undo both.
    let broken = Migration {
        version: 2,
        sql: "CREATE TABLE partial (id INTEGER); INSERT INTO missing_table (id) VALUES (1);",
    };

    let error = Storage::open_file(&path, &[first, broken])
        .err()
        .expect("broken migration must fail");

    assert!(matches!(
        error,
        StorageError::MigrationFailed { version: 2, .. }
    ));
    // The message is shown to users, so it must not reveal SQL or the file location.
    let message = error.to_string();
    assert!(!message.contains("missing_table"));
    assert!(!message.contains(&path.display().to_string()));

    // Reopening with only the first migration shows that version 1 and its data survived.
    let storage = Storage::open_file(&path, &[first]).expect("previous schema must still open");
    assert_eq!(storage.schema_version().expect("read version"), 1);
    assert_eq!(read_sample_value(&storage), "kept");
    let connection = storage.lock().expect("lock test connection");
    let partial_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'partial'",
            [],
            |row| row.get(0),
        )
        .expect("count partial table");
    assert_eq!(partial_tables, 0);
}

#[test]
fn database_from_a_newer_build_is_rejected_without_changes() {
    let directory = TestDirectory::new("newer-schema");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let known = [Migration {
        version: 1,
        sql: "",
    }];
    {
        let raw = Connection::open(&path).expect("create raw database");
        raw.pragma_update(None, "user_version", 9)
            .expect("simulate a newer schema");
    }

    let error = Storage::open_file(&path, &known)
        .err()
        .expect("newer schema must be rejected");

    assert!(matches!(
        error,
        StorageError::NewerSchema {
            found: 9,
            supported: 1
        }
    ));
    let raw = Connection::open(&path).expect("reopen raw database");
    let version: u32 = raw
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read version");
    assert_eq!(version, 9);
}

#[test]
fn upgrade_applies_each_missing_version_and_keeps_existing_rows() {
    let directory = TestDirectory::new("multi-step");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let first = Migration {
        version: 1,
        sql: "CREATE TABLE sample (value TEXT NOT NULL); INSERT INTO sample (value) VALUES ('kept');",
    };
    let second = Migration {
        version: 2,
        sql: "ALTER TABLE sample ADD COLUMN note TEXT;",
    };
    let third = Migration {
        version: 3,
        sql: "CREATE TABLE later (id INTEGER PRIMARY KEY);",
    };
    Storage::open_file(&path, &[first]).expect("open at version 1");

    let storage = Storage::open_file(&path, &[first, second, third]).expect("upgrade to version 3");

    assert_eq!(storage.schema_version().expect("read version"), 3);
    assert_eq!(read_sample_value(&storage), "kept");
    let connection = storage.lock().expect("lock test connection");
    let later_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'later'",
            [],
            |row| row.get(0),
        )
        .expect("count later table");
    assert_eq!(later_tables, 1);
}

#[test]
fn concurrent_openers_apply_each_migration_exactly_once() {
    let directory = TestDirectory::new("concurrent");
    let path = directory.path.join(DATABASE_FILE_NAME);
    // Applying this twice would fail on CREATE TABLE, and the row count would double.
    let migrations = [Migration {
        version: 1,
        sql: "CREATE TABLE sample (value TEXT NOT NULL); INSERT INTO sample (value) VALUES ('once');",
    }];
    let barrier = Barrier::new(4);

    // Four threads open the same file at the same moment, like four app instances.
    let opened: Vec<bool> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    Storage::open_file(&path, &migrations).is_ok()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("opener thread finished"))
            .collect()
    });

    assert!(opened.iter().all(|succeeded| *succeeded));
    let storage = Storage::open_file(&path, &migrations).expect("reopen after the race");
    assert_eq!(storage.schema_version().expect("read version"), 1);
    let connection = storage.lock().expect("lock test connection");
    let rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM sample", [], |row| row.get(0))
        .expect("count sample rows");
    assert_eq!(rows, 1);
}

#[test]
fn prerenumber_pi_mcp_tables_upgrade_and_allow_claude_code_targets() {
    let directory = TestDirectory::new("pi-mcp-prerenumber");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let skill_id = "0123456789abcdef0123456789abcdef";
    let definition_id = "fedcba9876543210fedcba9876543210";
    let digest = "ab".repeat(32);
    let generation = "cd".repeat(16);
    {
        let old = Storage::open_file(&path, &MIGRATIONS[..30]).expect("v30 database");
        let connection = old.lock().expect("lock v30 connection");
        connection
            .execute_batch(&format!(
                "INSERT INTO skill(
                    id, name, description, origin_path, content_digest,
                    file_count, byte_size, revision, imported_at, updated_at
                 ) VALUES(
                    '{skill_id}', 'demo', 'A test skill.', '/tmp/demo', '{digest}',
                    1, 1, 1, 1, 1
                 );
                 INSERT INTO skill_deployment VALUES(
                    '{skill_id}', 'pi', '/home/test/.pi/agent', 'demo', 'symlink',
                    1, '{digest}', 1
                 );
                 INSERT INTO mcp_definition(
                    id, display_name, server_name, namespace, enabled, revision,
                    connection, created_at, updated_at
                 ) VALUES(
                    '{definition_id}', 'Demo', 'demo', 'demo', 1, 1, '{{}}', 1, 1
                 );
                 CREATE TABLE pi_mcp_deployment (
                    config_root TEXT NOT NULL CHECK(length(config_root) BETWEEN 1 AND 4096),
                    definition_id TEXT NOT NULL REFERENCES mcp_definition(id) ON DELETE RESTRICT,
                    server_name TEXT NOT NULL CHECK(length(server_name) BETWEEN 1 AND 64),
                    namespace TEXT NOT NULL CHECK(length(namespace) BETWEEN 1 AND 64),
                    definition_revision INTEGER NOT NULL CHECK(definition_revision>=1),
                    entry_digest TEXT NOT NULL CHECK(length(entry_digest)=64 AND entry_digest NOT GLOB '*[^0-9a-f]*'),
                    generation TEXT NOT NULL CHECK(length(generation)=32 AND generation NOT GLOB '*[^0-9a-f]*'),
                    enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
                    PRIMARY KEY(config_root,definition_id),
                    UNIQUE(config_root,namespace)
                 ) STRICT, WITHOUT ROWID;
                 CREATE TABLE pi_mcp_change (
                    id TEXT PRIMARY KEY CHECK(length(id)=32 AND id NOT GLOB '*[^0-9a-f]*'),
                    config_root TEXT NOT NULL CHECK(length(config_root) BETWEEN 1 AND 4096),
                    definition_id TEXT NOT NULL CHECK(length(definition_id)=32),
                    server_name TEXT NOT NULL CHECK(length(server_name) BETWEEN 1 AND 64),
                    definition_revision INTEGER NOT NULL CHECK(definition_revision>=1),
                    action TEXT NOT NULL CHECK(action IN ('deploy','enable','disable','revoke','restore')),
                    state TEXT NOT NULL CHECK(state IN ('prepared','completed','aborted')),
                    before_digest TEXT CHECK(before_digest IS NULL OR length(before_digest)=64),
                    intended_digest TEXT CHECK(intended_digest IS NULL OR length(intended_digest)=64),
                    before_owner TEXT CHECK(before_owner IS NULL OR json_valid(before_owner)),
                    after_owner TEXT CHECK(after_owner IS NULL OR json_valid(after_owner)),
                    operation_id TEXT REFERENCES config_operation(id) ON DELETE RESTRICT,
                    created_at INTEGER NOT NULL CHECK(created_at>=0)
                 ) STRICT;
                 CREATE UNIQUE INDEX pi_mcp_one_pending_root ON pi_mcp_change(config_root) WHERE state='prepared';
                 CREATE INDEX pi_mcp_history ON pi_mcp_change(config_root,definition_id,created_at DESC,id DESC);
                 INSERT INTO pi_mcp_deployment(
                    config_root, definition_id, server_name, namespace,
                    definition_revision, entry_digest, generation, enabled
                 ) VALUES(
                    '/home/test/.pi/agent', '{definition_id}', 'demo', 'demo',
                    1, '{digest}', '{generation}', 1
                 );"
            ))
            .expect("seed the pre-merge schema 31 shape");
        // The old build's version 31 was the MCP DDL, not the Claude Code check.
        connection
            .pragma_update(None, "user_version", 31)
            .expect("stamp the pre-merge version");
        assert!(
            connection
                .execute(
                    "INSERT INTO skill_deployment VALUES(
                        ?1, 'claudeCode', '/home/test/.claude', 'demo', 'copy', 1, ?2, 1
                     )",
                    rusqlite::params![skill_id, digest],
                )
                .is_err(),
            "schema 31 from that build still rejects Claude Code"
        );
    }

    let storage = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert_eq!(
        storage.schema_version().expect("schema version"),
        latest_schema_version()
    );
    let connection = storage.lock().expect("lock upgraded connection");
    let kept_target: String = connection
        .query_row(
            "SELECT agent_id FROM skill_deployment WHERE skill_id = ?1 AND agent_id = 'pi'",
            [skill_id],
            |row| row.get(0),
        )
        .expect("existing Pi deployment");
    assert_eq!(kept_target, "pi");
    let kept_mcp: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pi_mcp_deployment WHERE definition_id = ?1",
            [definition_id],
            |row| row.get(0),
        )
        .expect("existing Pi MCP row");
    assert_eq!(kept_mcp, 1);
    connection
        .execute(
            "INSERT INTO skill_deployment VALUES(
                ?1, 'claudeCode', '/home/test/.claude', 'demo', 'copy', 1, ?2, 1
             )",
            rusqlite::params![skill_id, digest],
        )
        .expect("Claude Code target after upgrade");
}

#[test]
fn claude_code_skill_deployment_migration_keeps_existing_targets() {
    let directory = TestDirectory::new("skill-deploy-claude-code");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let skill_id = "0123456789abcdef0123456789abcdef";
    let digest = "ab".repeat(32);
    {
        let old = Storage::open_file(&path, &MIGRATIONS[..30]).expect("v30 database");
        old.lock()
            .unwrap()
            .execute_batch(&format!(
                "INSERT INTO skill(
                    id, name, description, origin_path, content_digest,
                    file_count, byte_size, revision, imported_at, updated_at
                 ) VALUES(
                    '{skill_id}', 'demo', 'A test skill.', '/tmp/demo', '{digest}',
                    1, 1, 1, 1, 1
                 );
                 INSERT INTO skill_deployment VALUES(
                    '{skill_id}', 'pi', '/home/test/.pi/agent', 'demo', 'symlink',
                    1, '{digest}', 1
                 );"
            ))
            .expect("seed v30 rows");
    }

    let storage = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert_eq!(
        storage.schema_version().expect("schema version"),
        latest_schema_version()
    );
    let connection = storage.lock().expect("lock test connection");
    let (agent_id, target_name): (String, String) = connection
        .query_row(
            "SELECT agent_id, target_name FROM skill_deployment",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("existing row");
    assert_eq!(agent_id, "pi");
    assert_eq!(target_name, "demo");

    connection
        .execute(
            "INSERT INTO skill_deployment VALUES(
                ?1, 'claudeCode', '/home/test/.claude', 'demo', 'copy', 1, ?2, 1
             )",
            rusqlite::params![skill_id, digest],
        )
        .expect("Claude Code target");
    assert!(
        connection
            .execute(
                "INSERT INTO skill_deployment VALUES(
                    ?1, 'unknown', '/home/test/other', 'demo', 'copy', 1, ?2, 1
                 )",
                rusqlite::params![skill_id, digest],
            )
            .is_err()
    );
}

#[test]
fn skill_deploy_operation_migration_preserves_v33_rows() {
    let directory = TestDirectory::new("skill-deploy-operation-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let skill_id = "0123456789abcdef0123456789abcdef";
    let digest = "a".repeat(64);
    {
        let old = Storage::open_file(&path, &MIGRATIONS[..33]).expect("v33 database");
        old.lock()
            .unwrap()
            .execute_batch(&format!(
                "INSERT INTO skill(
                    id, name, description, origin_path, content_digest,
                    file_count, byte_size, revision, imported_at, updated_at
                 ) VALUES(
                    '{skill_id}', 'demo', 'A test skill.', '/tmp/demo', '{digest}',
                    1, 1, 1, 1, 1
                 );
                 INSERT INTO skill_deployment VALUES(
                    '{skill_id}', 'pi', '/home/test/.pi/agent', 'demo', 'symlink',
                    1, '{digest}', 1
                 );"
            ))
            .expect("seed v33 rows");
    }

    let storage = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert_eq!(
        storage.schema_version().expect("schema version"),
        latest_schema_version()
    );
    let connection = storage.lock().expect("lock");
    let kept_skill: i64 = connection
        .query_row("SELECT COUNT(*) FROM skill_deployment", [], |row| {
            row.get(0)
        })
        .expect("skill deployment");
    assert_eq!(kept_skill, 1);
    let empty: i64 = connection
        .query_row("SELECT COUNT(*) FROM skill_deploy_operation", [], |row| {
            row.get(0)
        })
        .expect("operations");
    assert_eq!(empty, 0);
    connection
        .execute(
            "INSERT INTO skill_deploy_operation(
                id, kind, state, skill_id, agent_id, config_root, target_name,
                target_path, mode, skill_revision, content_digest, created_at, updated_at
             ) VALUES(
                ?1, 'apply', 'prepared', ?2, 'pi', '/home/test/.pi/agent', 'demo',
                '/home/test/.pi/agent/skills/demo', 'symlink', 1, ?3, 1, 1
             )",
            rusqlite::params![skill_id, skill_id, digest],
        )
        .expect("insert operation");
}

#[test]
fn claude_mcp_migration_preserves_v34_rows_and_restricts_definitions() {
    let directory = TestDirectory::new("claude-mcp-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let definition_id = "fedcba9876543210fedcba9876543210";
    let digest = "ab".repeat(32);
    let generation = "cd".repeat(16);
    {
        let old = Storage::open_file(&path, &MIGRATIONS[..34]).expect("v34 database");
        old.lock()
            .unwrap()
            .execute_batch(&format!(
                "INSERT INTO mcp_definition(
                    id, display_name, server_name, namespace, enabled, revision,
                    connection, created_at, updated_at
                 ) VALUES(
                    '{definition_id}', 'Demo', 'demo', 'demo', 1, 1, '{{}}', 1, 1
                 );
                 INSERT INTO grok_mcp_deployment(
                    config_root, definition_id, server_name, definition_revision,
                    entry_digest, generation, enabled
                 ) VALUES('/home/test/.grok', '{definition_id}', 'demo', 1,
                    '{digest}', '{generation}', 1);"
            ))
            .expect("seed v34 rows");
        assert!(
            old.lock()
                .unwrap()
                .execute("SELECT 1 FROM claude_mcp_deployment", [])
                .is_err(),
            "v34 has no Claude Code MCP tables"
        );
    }

    let storage = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert_eq!(
        storage.schema_version().expect("schema version"),
        latest_schema_version()
    );
    assert!(latest_schema_version() >= 35);
    let connection = storage.lock().expect("lock");
    let kept: i64 = connection
        .query_row("SELECT COUNT(*) FROM grok_mcp_deployment", [], |row| {
            row.get(0)
        })
        .expect("grok rows");
    assert_eq!(kept, 1);
    connection
        .execute(
            "INSERT INTO claude_mcp_deployment(
                config_root, definition_id, server_name, definition_revision,
                entry_digest, generation
             ) VALUES('/home/test', ?1, 'demo', 1, ?2, ?3)",
            rusqlite::params![definition_id, digest, generation],
        )
        .expect("ownership row");
    // Exact name is unique per root, and the definition cannot disappear.
    let other = "1".repeat(32);
    connection
        .execute(
            "INSERT INTO mcp_definition(
                id, display_name, server_name, namespace, enabled, revision,
                connection, created_at, updated_at
             ) VALUES(?1, 'Other', 'other', 'other', 1, 1, '{}', 1, 1)",
            [&other],
        )
        .expect("second definition");
    assert!(
        connection
            .execute(
                "INSERT INTO claude_mcp_deployment VALUES('/home/test', ?1, 'demo', 1, ?2, ?3)",
                rusqlite::params![other, digest, generation],
            )
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM mcp_definition WHERE id=?1", [definition_id])
            .is_err()
    );
    let change = |action: &str| {
        connection.execute(
            "INSERT INTO claude_mcp_change(
                id, config_root, definition_id, server_name, definition_revision,
                action, state, created_at
             ) VALUES(lower(hex(randomblob(16))), '/home/test', ?1, 'demo', 1, ?2, 'completed', 1)",
            rusqlite::params![definition_id, action],
        )
    };
    for action in ["deploy", "revoke", "restore"] {
        change(action).expect("written action");
    }
    for action in ["enable", "disable"] {
        assert!(change(action).is_err(), "{action} is never journaled");
    }
}

#[test]
fn codex_migration_preserves_v35_rows_and_admits_codex_targets() {
    let directory = TestDirectory::new("codex-upgrade");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let skill_id = "0123456789abcdef0123456789abcdef";
    let operation_id = "11111111111111111111111111111111";
    let definition_id = "fedcba9876543210fedcba9876543210";
    let provider_id = "abcdefabcdefabcdefabcdefabcdefab";
    let digest = "a".repeat(64);
    let generation = "cd".repeat(16);
    {
        let old = Storage::open_file(&path, &MIGRATIONS[..35]).expect("v35 database");
        old.lock()
            .unwrap()
            .execute_batch(&format!(
                "INSERT INTO skill(
                    id, name, description, origin_path, content_digest,
                    file_count, byte_size, revision, imported_at, updated_at
                 ) VALUES(
                    '{skill_id}', 'demo', 'A test skill.', '/tmp/demo', '{digest}',
                    1, 1, 1, 1, 1
                 );
                 INSERT INTO skill_deployment VALUES(
                    '{skill_id}', 'claudeCode', '/home/test/.claude', 'demo', 'copy',
                    1, '{digest}', 7
                 );
                 INSERT INTO skill_deploy_operation(
                    id, kind, state, skill_id, agent_id, config_root, target_name,
                    target_path, mode, skill_revision, content_digest, created_at, updated_at
                 ) VALUES(
                    '{operation_id}', 'apply', 'unknown', '{skill_id}', 'grokBuild',
                    '/home/test/.grok', 'demo', '/home/test/.grok/skills/demo', 'copy',
                    1, '{digest}', 3, 4
                 );
                 INSERT INTO mcp_definition(
                    id, display_name, server_name, namespace, enabled, revision,
                    connection, created_at, updated_at
                 ) VALUES(
                    '{definition_id}', 'Demo', 'demo', 'demo', 1, 1, '{{}}', 1, 1
                 );"
            ))
            .expect("seed v35 rows");
        let refused = old.lock().unwrap().execute(
            "INSERT INTO skill_deployment VALUES(?1, 'codex', '/home/test/.codex', 'other', 'copy', 1, ?2, 1)",
            rusqlite::params![skill_id, digest],
        );
        assert!(refused.is_err(), "v35 rejects the Codex target");
    }

    let storage = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert_eq!(
        storage.schema_version().expect("schema version"),
        latest_schema_version()
    );
    assert!(latest_schema_version() >= 36);
    let connection = storage.lock().expect("lock");
    let kept: (String, i64) = connection
        .query_row(
            "SELECT agent_id, deployed_at FROM skill_deployment",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("kept deployment");
    assert_eq!(kept, ("claudeCode".to_owned(), 7));
    let kept_operation: (String, String, i64) = connection
        .query_row(
            "SELECT agent_id, state, updated_at FROM skill_deploy_operation WHERE id=?1",
            [operation_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("kept operation");
    assert_eq!(
        kept_operation,
        ("grokBuild".to_owned(), "unknown".to_owned(), 4)
    );
    // Rebuilt indexes keep their names.
    let indexes: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index'
             AND name IN ('skill_deploy_operation_skill','skill_deploy_operation_list')",
            [],
            |row| row.get(0),
        )
        .expect("indexes");
    assert_eq!(indexes, 2);
    connection
        .execute(
            "INSERT INTO skill_deployment VALUES(?1, 'codex', '/home/test/.codex', 'demo', 'symlink', 1, ?2, 1)",
            rusqlite::params![skill_id, digest],
        )
        .expect("Codex skill target");
    connection
        .execute(
            "INSERT INTO skill_deploy_operation(
                id, kind, state, skill_id, agent_id, config_root, target_name,
                target_path, mode, skill_revision, content_digest, created_at, updated_at
             ) VALUES(
                lower(hex(randomblob(16))), 'disable', 'prepared', ?1, 'codex',
                '/home/test/.codex', 'demo', '/home/test/.codex/skills/demo', 'symlink', 1, ?2, 1, 1
             )",
            rusqlite::params![skill_id, digest],
        )
        .expect("Codex skill journal");
    // Codex MCP ownership restricts central deletion like the other Agents.
    connection
        .execute(
            "INSERT INTO codex_mcp_deployment VALUES('/home/test/.codex', ?1, 'demo', 1, ?2, ?3, 1)",
            rusqlite::params![definition_id, digest, generation],
        )
        .expect("Codex MCP ownership");
    assert!(
        connection
            .execute("DELETE FROM mcp_definition WHERE id=?1", [definition_id])
            .is_err()
    );
    // One provider key per root, and one owned default per root.
    connection
        .execute(
            "INSERT INTO codex_provider_binding VALUES('/home/test/.codex', ?1, 'vibemate-demo', 1, ?2, ?3)",
            rusqlite::params![provider_id, digest, generation],
        )
        .expect("provider binding");
    assert!(
        connection
            .execute(
                "INSERT INTO codex_provider_binding VALUES('/home/test/.codex', ?1, 'vibemate-demo', 1, ?2, ?3)",
                rusqlite::params!["b".repeat(32), digest, generation],
            )
            .is_err()
    );
    connection
        .execute(
            "INSERT INTO codex_default_binding VALUES('/home/test/.codex', ?1, 'm', 'vibemate-demo', ?2, '{}', ?3, 1)",
            rusqlite::params![provider_id, digest, generation],
        )
        .expect("default binding");
    let change = |action: &str, state: &str| {
        connection.execute(
            "INSERT INTO codex_settings_change(
                id, config_root, provider_id, provider_key, action, state,
                before_owner, after_owner, created_at
             ) VALUES(lower(hex(randomblob(16))), '/home/test/.codex', ?1, 'vibemate-demo',
                ?2, ?3, '{}', '{}', 1)",
            rusqlite::params![provider_id, action, state],
        )
    };
    for action in ["deploy", "revoke", "set_default", "clear_default"] {
        change(action, "completed").expect("journaled action");
    }
    assert!(change("restore", "completed").is_err());
    change("deploy", "prepared").expect("one pending change");
    assert!(
        change("revoke", "prepared").is_err(),
        "a root has at most one pending settings change"
    );
}

#[test]
fn shipped_migrations_are_numbered_consecutively_from_one() {
    for (index, migration) in MIGRATIONS.iter().enumerate() {
        let expected = u32::try_from(index + 1).expect("migration count fits in u32");
        assert_eq!(migration.version, expected);
    }
}

#[test]
fn unreadable_database_is_reported_and_left_unchanged() {
    let directory = TestDirectory::new("unreadable");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let original = b"this file is not a SQLite database".to_vec();
    fs::write(&path, &original).expect("write unreadable file");

    // Startup must still produce a status, so the window can open.
    let status = StorageStatus::open_in_directory(&directory.path);

    assert!(matches!(status.storage(), Err(StorageError::Database(_))));
    assert_eq!(fs::read(&path).expect("read file back"), original);
}

#[test]
fn storage_status_can_be_shared_between_threads() {
    // Tauri stores the status as managed state across threads. This stops compiling
    // if the status, or the storage it wraps, loses `Send` or `Sync`.
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<StorageStatus>();
}

#[test]
fn codex_settings_history_migration_backfills_model_ids() {
    let directory = TestDirectory::new("codex-history-model");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let provider = "abcdefabcdefabcdefabcdefabcdefab";
    let digest = "a".repeat(64);
    let generation = "cd".repeat(16);
    let default_owner = format!(
        r#"{{"provider":null,"default":{{"providerId":"{provider}","modelId":"deepseek-flash","providerKey":"vibemate-x","entryDigest":"{digest}","previous":{{"model":null,"modelProvider":null,"modelReasoningEffort":null}},"generation":"{generation}"}}}}"#
    );
    let empty = r#"{"provider":null,"default":null}"#;
    {
        let old = Storage::open_file(&path, &MIGRATIONS[..36]).expect("v36 database");
        let connection = old.lock().unwrap();
        for (id, action, before, after) in [
            ("1", "deploy", empty, empty),
            ("2", "set_default", empty, default_owner.as_str()),
            ("3", "clear_default", default_owner.as_str(), empty),
            ("4", "revoke", empty, empty),
        ] {
            connection
                .execute(
                    "INSERT INTO codex_settings_change(id,config_root,provider_id,provider_key,action,state,before_owner,after_owner,created_at) VALUES(?1,'/home/test/.codex',?2,'vibemate-x',?3,'completed',?4,?5,1)",
                    rusqlite::params![id.repeat(32), provider, action, before, after],
                )
                .expect("seed v36 change");
        }
    }

    let storage = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert!(storage.schema_version().expect("schema version") >= 37);
    let connection = storage.lock().expect("lock");
    let mut statement = connection
        .prepare("SELECT action, model_id FROM codex_settings_change ORDER BY id")
        .unwrap();
    let rows: Vec<(String, Option<String>)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        [
            ("deploy".to_owned(), None),
            ("set_default".to_owned(), Some("deepseek-flash".to_owned())),
            (
                "clear_default".to_owned(),
                Some("deepseek-flash".to_owned())
            ),
            ("revoke".to_owned(), None),
        ]
    );
    let too_long = connection.execute(
        "UPDATE codex_settings_change SET model_id=?1",
        [&"m".repeat(161)],
    );
    assert!(too_long.is_err(), "model ids stay bounded");
}

#[test]
fn codex_default_effort_migration_keeps_existing_defaults_owning_the_effort() {
    let directory = TestDirectory::new("codex-default-effort");
    let path = directory.path.join(DATABASE_FILE_NAME);
    let provider = "abcdefabcdefabcdefabcdefabcdefab";
    {
        let old = Storage::open_file(&path, &MIGRATIONS[..37]).expect("v37 database");
        old.lock()
            .unwrap()
            .execute(
                "INSERT INTO codex_default_binding VALUES('/home/test/.codex', ?1, 'm', 'vibemate-x', ?2, '{}', ?3)",
                rusqlite::params![provider, "a".repeat(64), "cd".repeat(16)],
            )
            .expect("seed v37 default");
    }

    let storage = Storage::open_in_directory(&directory.path).expect("upgrade");
    assert!(storage.schema_version().expect("schema version") >= 38);
    let connection = storage.lock().expect("lock");
    let writes: i64 = connection
        .query_row("SELECT writes_effort FROM codex_default_binding", [], |r| {
            r.get(0)
        })
        .expect("flag");
    assert_eq!(
        writes, 1,
        "earlier defaults always set or removed the effort"
    );
    assert!(
        connection
            .execute("UPDATE codex_default_binding SET writes_effort=2", [])
            .is_err()
    );
}
