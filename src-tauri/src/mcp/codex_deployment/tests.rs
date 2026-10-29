//! End-to-end Codex entry lifecycle with isolated `$CODEX_HOME` roots, real
//! SQLite/private snapshots and synthetic OS credentials only. Native TOML is
//! mutated with toml_edit; comments, unrelated provider tables and profiles
//! stay byte-identical. Nothing here touches the real `~/.codex`.

use super::*;
use crate::credentials::{CredentialError, Secret};
use crate::mcp::{self, ConnectionInput, SaveMcpRequest, SecretInput};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use toml_edit::DocumentMut;

#[derive(Default)]
struct Store {
    values: RefCell<BTreeMap<String, String>>,
    loads: Cell<usize>,
    deny: Cell<bool>,
}
impl CredentialStore for Store {
    fn save(&self, key: &str, value: &Secret) -> Result<(), CredentialError> {
        self.values
            .borrow_mut()
            .insert(key.into(), value.expose().into());
        Ok(())
    }
    fn load(&self, key: &str) -> Result<Option<Secret>, CredentialError> {
        self.loads.set(self.loads.get() + 1);
        if self.deny.get() {
            return Err(CredentialError::AccessDenied);
        }
        Ok(self
            .values
            .borrow()
            .get(key)
            .map(|v| Secret::new(v.clone())))
    }
    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        self.values.borrow_mut().remove(key);
        Ok(())
    }
}
struct Fixture {
    directory: PathBuf,
    root: PathBuf,
    storage: Storage,
    store: Store,
    environment: DiscoveryEnvironment,
    backups: BackupStore,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "vibemate-codex-mcp-flow-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // `$CODEX_HOME` must exist; the canonical path is the ownership key.
        let configured = directory.join("agent");
        std::fs::create_dir_all(&configured).unwrap();
        let storage = Storage::open_in_directory(&directory.join("private")).unwrap();
        let environment = DiscoveryEnvironment {
            home: directory.join("home"),
            codex_directory: Some(configured.into_os_string()),
            ..Default::default()
        };
        let root = crate::agents::codex_config_root(&environment).0.unwrap();
        Self {
            backups: BackupStore::new(directory.join("backups")),
            directory,
            root,
            storage,
            store: Store::default(),
            environment,
        }
    }
    fn definition(&self, http: bool) -> mcp::McpRecord {
        mcp::save_definition(
            &self.storage,
            &self.store,
            SaveMcpRequest {
                id: None,
                expected_revision: None,
                display_name: "Synthetic tools".into(),
                server_name: if http {
                    "remote-tools".into()
                } else {
                    "local-tools".into()
                },
                enabled: true,
                connection: if http {
                    ConnectionInput::Http {
                        url: "https://example.com/mcp".into(),
                        headers: vec![SecretInput {
                            name: "X-Custom-Header".into(),
                            value: Some(Secret::new("synthetic$DOLLAR$!".into())),
                        }],
                    }
                } else {
                    ConnectionInput::Stdio {
                        command: "nonexistent-vibemate-test-server".into(),
                        args: vec!["literal space".into()],
                        cwd: None,
                        env: vec![SecretInput {
                            name: "CUSTOM_VALUE".into(),
                            value: Some(Secret::new("synthetic$DOLLAR$!".into())),
                        }],
                    }
                },
            },
            1,
        )
        .unwrap()
    }
    fn selection(&self, row: &mcp::McpRecord, action: Action) -> Selection {
        Selection {
            definition_id: row.id.clone(),
            expected_revision: row.revision,
            action,
            history_id: None,
        }
    }
    fn perform(&self, selection: &Selection) -> Applied {
        let plan = preview(&self.storage, &self.environment, selection, &self.backups).unwrap();
        apply(
            &self.storage,
            &self.store,
            &self.environment,
            &ApplyRequest {
                selection: selection.clone(),
                approval: plan.approval,
            },
            &self.backups,
        )
        .unwrap()
    }
    fn text(&self) -> String {
        std::fs::read_to_string(self.root.join("config.toml")).unwrap()
    }
    fn document(&self) -> DocumentMut {
        self.text().parse().unwrap()
    }
    fn entry(&self, name: &str) -> Value {
        plan::entry_from_toml(
            self.document()
                .get("mcp_servers")
                .unwrap()
                .as_table_like()
                .unwrap()
                .get(name)
                .unwrap(),
        )
        .unwrap()
    }
    fn write_toml(&self, text: &str) {
        std::fs::create_dir_all(&self.root).unwrap();
        std::fs::write(self.root.join("config.toml"), text).unwrap();
    }
    fn state(&self) -> State {
        read_state(
            &self.storage,
            &self.environment,
            &StateRequest {
                definition_id: None,
                after: None,
                limit: 20,
            },
            &self.backups,
        )
        .unwrap()
    }
    fn sql(&self, sql: &str) {
        self.storage.lock().unwrap().execute_batch(sql).unwrap();
    }
    fn recovery(&self) -> Applied {
        let r = self.state().recovery.unwrap();
        recover(
            &self.storage,
            &self.environment,
            &RecoveryRequest {
                change_id: r.id,
                approval: r.approval.unwrap(),
            },
            &self.backups,
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn complete_stdio_flow_preserves_unowned_toml_and_only_deploy_reads_keys() {
    let f = Fixture::new();
    let row = f.definition(false);
    // Root-level keys must appear before dotted tables; otherwise TOML nests them.
    f.write_toml(
        r#"
# user comment stays
model = "gpt-5" # trailing comment stays

[profiles.strict]
approval_policy = "never"

[mcp_servers.user]
command = "never-run-user-command"
env = { PRIVATE = "unowned-synthetic" }
"#,
    );
    let original = f.text();
    let selection = f.selection(&row, Action::Deploy);
    let plan = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(!serialized.contains("synthetic$DOLLAR"));
    assert!(!serialized.contains("unowned-synthetic"));
    assert_eq!(f.store.loads.get(), 0);
    let deployed = f.perform(&selection);
    assert!(deployed.registered);
    assert_eq!(f.store.loads.get(), 1);
    // Every unrelated byte, including comments and the legacy profile table,
    // stays a prefix of the new file; only the owned table is appended.
    let written = f.text();
    assert!(written.starts_with(&original), "{written}");
    assert!(written.contains("[mcp_servers.local-tools]"));
    assert!(!written.contains("\nprofile"));
    assert!(!written.contains("[mcp_servers]\n"));
    let entry = f.entry(&row.server_name);
    assert_eq!(entry["env"]["CUSTOM_VALUE"], "synthetic$DOLLAR$!");
    assert_eq!(entry["enabled"], true);
    f.store.deny.set(true);
    f.perform(&f.selection(&row, Action::Disable));
    assert_eq!(f.entry(&row.server_name)["enabled"], false);
    f.perform(&f.selection(&row, Action::Enable));
    assert_eq!(f.entry(&row.server_name)["enabled"], true);
    let revoked = f.perform(&f.selection(&row, Action::Revoke));
    assert!(!revoked.registered);
    assert!(
        f.document()
            .get("mcp_servers")
            .and_then(|v| v.as_table_like())
            .is_none_or(|t| !t.contains_key(&row.server_name))
    );
    assert_eq!(f.store.loads.get(), 1);
    assert!(f.state().deployments.is_empty());
    let mut restore = f.selection(&row, Action::Restore);
    restore.history_id = Some(deployed.change_id);
    f.perform(&restore);
    assert_eq!(f.entry(&row.server_name)["enabled"], true);
    assert_eq!(
        f.entry(&row.server_name)["env"]["CUSTOM_VALUE"],
        "synthetic$DOLLAR$!"
    );
    restore.history_id = Some(revoked.change_id);
    f.perform(&restore);
    assert!(f.state().deployments.is_empty());
    // Removing the last owned entry restores the original bytes exactly.
    assert_eq!(f.text(), original);
}

#[test]
fn http_credentials_are_literal_and_never_returned_in_preview() {
    let f = Fixture::new();
    let row = f.definition(true);
    f.perform(&f.selection(&row, Action::Deploy));
    assert_eq!(
        f.entry(&row.server_name)["http_headers"]["x-custom-header"],
        "synthetic$DOLLAR$!"
    );
    let table = f.document()["mcp_servers"][&row.server_name].clone();
    let table = table.as_table_like().unwrap();
    // Codex infers streamable HTTP from `url` and rejects inline bearer tokens.
    for forbidden in [
        "type",
        "bearer_token",
        "headers",
        "env_http_headers",
        "auth",
    ] {
        assert!(table.get(forbidden).is_none(), "{forbidden}");
    }
    let plan = preview(
        &f.storage,
        &f.environment,
        &f.selection(&row, Action::Disable),
        &f.backups,
    )
    .unwrap();
    let text = serde_json::to_string(&plan).unwrap();
    assert!(!text.contains("synthetic"));
    assert!(text.contains("••••••••"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(f.root.join("config.toml"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn codex_cli_charset_is_enforced_for_server_names() {
    for good in [
        "local-tools",
        "a_b",
        "scope:tool",
        "me@host",
        "org/tool",
        "v1.2",
    ] {
        assert!(plan::valid_server_name(good), "{good}");
    }
    for bad in [
        "",
        "has space",
        "emoji\u{1F600}",
        "quote\"",
        &"x".repeat(65),
    ] {
        assert!(!plan::valid_server_name(bad), "{bad}");
    }
}

#[test]
fn symlinked_config_and_missing_root_are_refused() {
    let f = Fixture::new();
    let row = f.definition(false);
    #[cfg(unix)]
    {
        let elsewhere = f.directory.join("elsewhere.toml");
        std::fs::write(&elsewhere, "keep = true\n").unwrap();
        std::os::unix::fs::symlink(&elsewhere, f.root.join("config.toml")).unwrap();
        assert_eq!(
            preview(
                &f.storage,
                &f.environment,
                &f.selection(&row, Action::Deploy),
                &f.backups
            )
            .err(),
            Some(Error::TargetUnavailable)
        );
        assert_eq!(
            std::fs::read_to_string(&elsewhere).unwrap(),
            "keep = true\n"
        );
        std::fs::remove_file(f.root.join("config.toml")).unwrap();
    }
    let missing = DiscoveryEnvironment {
        home: f.directory.join("home"),
        codex_directory: Some(f.directory.join("absent").into_os_string()),
        ..Default::default()
    };
    assert_eq!(
        preview(
            &f.storage,
            &missing,
            &f.selection(&row, Action::Deploy),
            &f.backups
        )
        .err(),
        Some(Error::TargetUnavailable)
    );
}

#[test]
fn codex_reserialization_keeps_ownership_but_hand_added_keys_conflict() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.perform(&f.selection(&row, Action::Deploy));
    // Simulate `codex mcp add other`: Codex rewrites [mcp_servers] with
    // different formatting (dotted env table, no `enabled = true`).
    f.write_toml(
        r#"[mcp_servers.local-tools]
args = ["literal space"]
command = "nonexistent-vibemate-test-server"

[mcp_servers.local-tools.env]
CUSTOM_VALUE = "synthetic$DOLLAR$!"

[mcp_servers.other]
command = "user"
"#,
    );
    assert_eq!(f.state().deployments[0].state, "written");
    f.perform(&f.selection(&row, Action::Disable));
    assert!(
        f.document()["mcp_servers"]
            .as_table()
            .unwrap()
            .contains_key("other")
    );
    // A hand-added key Codex knows but vibemate does not map is an external edit.
    let mut doc = f.document();
    doc["mcp_servers"][&row.server_name]["startup_timeout_sec"] = toml_edit::value(30);
    f.write_toml(&doc.to_string());
    assert_eq!(
        preview(
            &f.storage,
            &f.environment,
            &f.selection(&row, Action::Enable),
            &f.backups
        )
        .err(),
        Some(Error::OwnershipConflict)
    );
    assert_eq!(f.state().deployments[0].state, "modified");
}

#[test]
fn pending_settings_change_blocks_mcp_writes() {
    let f = Fixture::new();
    let row = f.definition(false);
    let operation = repository::random_id(&f.storage).unwrap();
    f.sql(&format!(
        "INSERT INTO codex_settings_change(
            id, config_root, provider_id, provider_key, action, state,
            before_owner, after_owner, created_at
         ) VALUES('{operation}', '{}', '{}', 'vibemate-x', 'deploy', 'prepared',
            'null', 'null', 1)",
        f.root.to_str().unwrap(),
        "a".repeat(32)
    ));
    assert_eq!(
        preview(
            &f.storage,
            &f.environment,
            &f.selection(&row, Action::Deploy),
            &f.backups
        )
        .err(),
        Some(Error::RecoveryRequired)
    );
}

#[test]
fn relative_and_home_cwd_are_refused_before_any_key_read() {
    let f = Fixture::new();
    for (index, cwd) in ["relative/dir", "~/dir"].into_iter().enumerate() {
        let row = mcp::save_definition(
            &f.storage,
            &f.store,
            SaveMcpRequest {
                id: None,
                expected_revision: None,
                display_name: format!("Cwd {index}"),
                server_name: format!("cwd-{index}"),
                enabled: true,
                connection: ConnectionInput::Stdio {
                    command: "never".into(),
                    args: vec![],
                    cwd: Some(cwd.into()),
                    env: vec![],
                },
            },
            1,
        );
        // Central validation may already refuse it; either way nothing is written.
        if let Ok(row) = row {
            assert_eq!(
                preview(
                    &f.storage,
                    &f.environment,
                    &f.selection(&row, Action::Deploy),
                    &f.backups
                )
                .err(),
                Some(Error::PathSemanticsUnverified)
            );
        }
    }
    assert_eq!(f.store.loads.get(), 0);
    assert!(!f.root.join("config.toml").exists());
}

#[test]
fn stale_target_and_central_revision_write_nothing() {
    let f = Fixture::new();
    let row = f.definition(false);
    let selection = f.selection(&row, Action::Deploy);
    let plan = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
    f.write_toml("unrelated = true\n");
    assert_eq!(
        apply(
            &f.storage,
            &f.store,
            &f.environment,
            &ApplyRequest {
                selection: selection.clone(),
                approval: plan.approval
            },
            &f.backups
        )
        .err(),
        Some(Error::PlanStale)
    );
    assert_eq!(f.store.loads.get(), 0);
    f.sql(&format!(
        "UPDATE mcp_definition SET revision=revision+1 WHERE id='{}'",
        row.id
    ));
    assert_eq!(
        preview(&f.storage, &f.environment, &selection, &f.backups).err(),
        Some(Error::DefinitionChanged)
    );
    assert_eq!(f.text(), "unrelated = true\n");
}

#[test]
fn foreign_name_and_modified_owned_entry_are_never_overwritten() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.write_toml(
        r#"
[mcp_servers.local-tools]
command = "foreign"
enabled = false
"#,
    );
    assert_eq!(
        preview(
            &f.storage,
            &f.environment,
            &f.selection(&row, Action::Deploy),
            &f.backups
        )
        .err(),
        Some(Error::NameConflict)
    );
    f.write_toml("");
    f.perform(&f.selection(&row, Action::Deploy));
    let mut doc = f.document();
    doc["mcp_servers"][&row.server_name]["args"] =
        toml_edit::value(toml_edit::Array::from_iter(["user-changed"]));
    f.write_toml(&doc.to_string());
    for action in [Action::Deploy, Action::Disable, Action::Revoke] {
        assert_eq!(
            preview(
                &f.storage,
                &f.environment,
                &f.selection(&row, action),
                &f.backups
            )
            .err(),
            Some(Error::OwnershipConflict)
        );
    }
    assert_eq!(f.state().deployments[0].state, "modified");
}

#[test]
fn deployment_and_pending_intents_block_central_deletion_until_revoke() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.perform(&f.selection(&row, Action::Deploy));
    let delete = || mcp::DeleteMcpRequest {
        id: row.id.clone(),
        expected_revision: row.revision,
    };
    assert_eq!(
        mcp::delete_definition(&f.storage, &f.store, delete()).err(),
        Some(mcp::McpError::McpInUse)
    );
    f.perform(&f.selection(&row, Action::Disable));
    assert_eq!(
        mcp::delete_definition(&f.storage, &f.store, delete()).err(),
        Some(mcp::McpError::McpInUse)
    );
    f.perform(&f.selection(&row, Action::Revoke));
    mcp::delete_definition(&f.storage, &f.store, delete()).unwrap();
}

#[test]
fn restoring_one_entry_keeps_later_user_servers_and_model_keys() {
    let f = Fixture::new();
    let row = f.definition(false);
    let initial = f.perform(&f.selection(&row, Action::Deploy));
    f.perform(&f.selection(&row, Action::Disable));
    let mut doc = f.document();
    doc["mcp_servers"]["later-user"] = toml_edit::table();
    doc["mcp_servers"]["later-user"]["url"] = toml_edit::value("https://example.com/another");
    doc["model"] = toml_edit::value("later-model");
    f.write_toml(&doc.to_string());
    let mut selection = f.selection(&row, Action::Restore);
    selection.history_id = Some(initial.change_id.clone());
    f.perform(&selection);
    let after = f.document();
    assert_eq!(after["model"].as_str(), Some("later-model"));
    assert_eq!(
        after["mcp_servers"]["later-user"]["url"].as_str(),
        Some("https://example.com/another")
    );
    assert_eq!(f.entry(&row.server_name)["enabled"], true);
}

#[test]
fn pending_before_observation_can_be_abandoned_without_replay() {
    let f = Fixture::new();
    let row = f.definition(false);
    let selection = f.selection(&row, Action::Deploy);
    let plan = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
    // Force a prepared intent with matching before bytes and no file write yet.
    let change_id = repository::random_id(&f.storage).unwrap();
    let root = f.root.to_str().unwrap().to_string();
    repository::prepare(
        &f.storage,
        &repository::Change {
            id: change_id.clone(),
            root: root.clone(),
            definition_id: row.id.clone(),
            server_name: row.server_name.clone(),
            definition_revision: row.revision,
            action: "deploy".into(),
            state: "prepared".into(),
            before_digest: plan.target.content_digest.clone(),
            intended_digest: Some("a".repeat(64)),
            before_owner: None,
            after_owner: Some(Ownership {
                server_name: row.server_name.clone(),
                definition_revision: row.revision,
                entry_digest: "b".repeat(64),
                generation: change_id.clone(),
                enabled: true,
            }),
            operation_id: None,
            created_at: 1,
        },
    )
    .unwrap();
    assert_eq!(
        preview(&f.storage, &f.environment, &selection, &f.backups).err(),
        Some(Error::RecoveryRequired)
    );
    let recovered = f.recovery();
    assert_eq!(recovered.change_id, change_id);
    assert!(!recovered.registered);
    assert!(!f.root.join("config.toml").exists());
    f.perform(&selection);
    assert!(f.state().deployments[0].state == "written");
}

#[test]
fn dynamic_secret_values_are_refused_without_writing() {
    let f = Fixture::new();
    let row = mcp::save_definition(
        &f.storage,
        &f.store,
        SaveMcpRequest {
            id: None,
            expected_revision: None,
            display_name: "Dynamic".into(),
            server_name: "dynamic-tools".into(),
            enabled: true,
            connection: ConnectionInput::Stdio {
                command: "never".into(),
                args: vec![],
                cwd: None,
                env: vec![SecretInput {
                    name: "TOKEN".into(),
                    value: Some(Secret::new("literal-ok".into())),
                }],
            },
        },
        1,
    )
    .unwrap();
    // Central validation rejects `${`; simulate a later OS-store change that
    // look like expansion syntax so apply must refuse without writing.
    let keys: Vec<String> = f.store.values.borrow().keys().cloned().collect();
    {
        let mut values = f.store.values.borrow_mut();
        for key in keys {
            values.insert(key, "${HOME}".into());
        }
    }
    let selection = f.selection(&row, Action::Deploy);
    let plan = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
    assert_eq!(
        apply(
            &f.storage,
            &f.store,
            &f.environment,
            &ApplyRequest {
                selection,
                approval: plan.approval
            },
            &f.backups
        )
        .err(),
        Some(Error::CredentialUnavailable)
    );
    assert!(!f.root.join("config.toml").exists());
}

#[test]
fn hand_disabled_entries_and_tool_filters_are_reported_on_the_row() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.perform(&f.selection(&row, Action::Deploy));
    let written = &f.state().deployments[0];
    assert_eq!(written.state, "written");
    assert_eq!(written.native_enabled, Some(true));
    assert!(written.unmanaged_keys.is_empty());

    // `enabled = false` by hand: the flag lives in the entry itself.
    let mut doc = f.document();
    doc["mcp_servers"][&row.server_name]["enabled"] = toml_edit::value(false);
    f.write_toml(&doc.to_string());
    let disabled = &f.state().deployments[0];
    assert_eq!(disabled.state, "modified");
    assert!(disabled.ownership.enabled);
    assert_eq!(disabled.native_enabled, Some(false));
    assert!(disabled.unmanaged_keys.is_empty());
    // A user decision is never overwritten, including by Enable.
    assert_eq!(
        preview(
            &f.storage,
            &f.environment,
            &f.selection(&row, Action::Enable),
            &f.backups
        )
        .err(),
        Some(Error::OwnershipConflict)
    );

    // Tool allow/deny lists are unmanaged keys; only their names surface.
    doc["mcp_servers"][&row.server_name]["enabled"] = toml_edit::value(true);
    doc["mcp_servers"][&row.server_name]["disabled_tools"] =
        toml_edit::value(toml_edit::Array::from_iter(["secret-tool-name"]));
    doc["mcp_servers"][&row.server_name]["enabled_tools"] =
        toml_edit::value(toml_edit::Array::from_iter(["search"]));
    f.write_toml(&doc.to_string());
    let filtered = &f.state().deployments[0];
    assert_eq!(filtered.state, "modified");
    assert_eq!(filtered.native_enabled, Some(true));
    assert_eq!(filtered.unmanaged_keys, ["disabled_tools", "enabled_tools"]);
    let json = serde_json::to_string(filtered).unwrap();
    assert!(json.contains("\"nativeEnabled\":true"));
    assert!(
        !json.contains("entryDigest"),
        "the digest hashes protected values"
    );
    assert!(
        !json.contains("secret-tool-name"),
        "values never leave Rust"
    );

    // An entry vibemate cannot interpret is the user's, not a read failure.
    doc["mcp_servers"][&row.server_name]["enabled"] = toml_edit::value("no");
    f.write_toml(&doc.to_string());
    let odd = &f.state().deployments[0];
    assert_eq!(odd.state, "modified");
    assert_eq!(odd.native_enabled, None);

    f.write_toml("");
    let missing = &f.state().deployments[0];
    assert_eq!(missing.state, "missing");
    assert_eq!(missing.native_enabled, None);
}

#[test]
fn previews_mask_url_query_and_argument_credentials() {
    let placeholder = crate::config_plan::SECRET_PLACEHOLDER;
    let entry = serde_json::json!({
        "command": "npx",
        "args": ["-y", "tool", "--api-key", "sk-arg", "--token=tok-arg", "Authorization: Bearer b-arg"],
        "url": "https://mcp.example/sse?api_key=sk-url&region=eu",
        "env": {"API_TOKEN": "sk-env", "REGION": "eu"}
    });
    let masked = super::plan::sanitize(Some(&entry)).expect("entry");
    let text = masked.to_string();
    for secret in ["sk-arg", "tok-arg", "b-arg", "sk-url", "sk-env"] {
        assert!(!text.contains(secret), "{secret} leaked");
    }
    assert_eq!(
        masked["url"],
        format!("https://mcp.example/sse?api_key={placeholder}&region=eu")
    );
    assert_eq!(masked["args"][3], placeholder);
    assert_eq!(masked["args"][1], "tool");
    assert_eq!(masked["command"], "npx");
}

#[test]
fn rewrites_keep_the_table_comment_and_an_inline_parent_is_refused_or_revoked() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.write_toml("model = \"m\"\n");
    f.perform(&f.selection(&row, Action::Deploy));
    let header = format!("[mcp_servers.{}]", row.server_name);
    let text = f.text().replace(&header, &format!("# my tools\n{header}"));
    f.write_toml(&text);
    f.perform(&f.selection(&row, Action::Disable));
    assert!(
        f.text().contains(&format!("# my tools\n{header}")),
        "{}",
        f.text()
    );

    // Same entry, inline parent: semantically unchanged, so still owned.
    let mut doc = f.document();
    let servers = doc["mcp_servers"].as_table().unwrap().clone();
    let mut inline = servers.into_inline_table();
    for (_, entry) in inline.iter_mut() {
        if let Some(table) = entry.as_inline_table_mut() {
            table.fmt();
        }
    }
    doc["mcp_servers"] = toml_edit::value(inline);
    f.write_toml(&doc.to_string());
    assert_eq!(f.state().deployments[0].state, "written");
    let before = f.text();
    assert_eq!(
        preview(
            &f.storage,
            &f.environment,
            &f.selection(&row, Action::Enable),
            &f.backups
        )
        .err(),
        Some(Error::InvalidConfig)
    );
    assert_eq!(f.text(), before, "refused preview writes nothing");
    f.perform(&f.selection(&row, Action::Revoke));
    assert!(f.document().get("mcp_servers").is_none(), "{}", f.text());
    assert!(f.text().contains("model = \"m\""));
}

#[test]
fn revoke_from_an_inline_parent_keeps_the_users_other_entries_verbatim() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.write_toml("model = \"m\"\n");
    f.perform(&f.selection(&row, Action::Deploy));
    // Our entry as inline text, between two user entries with their own
    // spacing, all inside one inline `mcp_servers` table.
    let mut ours = f.document()["mcp_servers"][row.server_name.as_str()]
        .as_table()
        .unwrap()
        .clone()
        .into_inline_table();
    ours.fmt();
    let first = r#"alpha = { command = "a",  args = ["--one"] }"#;
    let last = r#"zeta = {command="z"}"#;
    let name = &row.server_name;
    f.write_toml(&format!(
        "model = \"m\"\nmcp_servers = {{ {first}, {name} = {ours}, {last} }}\n# after\n"
    ));
    assert_eq!(
        f.state()
            .deployments
            .iter()
            .find(|row| row.state != "absent")
            .map(|row| row.state.as_str()),
        Some("written"),
        "{}",
        f.text()
    );
    f.perform(&f.selection(&row, Action::Revoke));
    let text = f.text();
    let doc = f.document();
    let servers = doc["mcp_servers"].as_inline_table().expect("still inline");
    assert!(servers.get(name).is_none(), "{text}");
    assert_eq!(servers.len(), 2, "{text}");
    assert!(text.contains(first), "{text}");
    assert!(text.contains(last), "{text}");
    assert!(text.starts_with("model = \"m\"\n"), "{text}");
    assert!(text.ends_with("# after\n"), "{text}");
}

#[test]
fn odd_hand_added_keys_stay_within_the_interface_limits() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.perform(&f.selection(&row, Action::Deploy));
    let mut doc = f.document();
    let entry = doc["mcp_servers"][row.server_name.as_str()]
        .as_table_mut()
        .unwrap();
    entry.insert("", toml_edit::value(1));
    entry.insert(&"k".repeat(200), toml_edit::value(2));
    for index in 0..70 {
        entry.insert(&format!("extra{index:02}"), toml_edit::value(index));
    }
    f.write_toml(&doc.to_string());
    assert!(f.text().contains("\"\" = 1"), "{}", f.text());

    let state = f.state();
    let deployment = &state.deployments[0];
    assert_eq!(deployment.state, "modified");
    assert_eq!(deployment.unmanaged_keys.len(), 64);
    for key in &deployment.unmanaged_keys {
        assert!((1..=128).contains(&key.encode_utf16().count()), "{key}");
    }
    assert!(
        deployment
            .unmanaged_keys
            .iter()
            .any(|key| key.starts_with("extra"))
    );
    // Every write is still refused: the digest saw all keys.
    let refused = preview(
        &f.storage,
        &f.environment,
        &f.selection(&row, Action::Disable),
        &f.backups,
    )
    .err();
    assert!(
        matches!(
            refused,
            Some(Error::OwnershipConflict | Error::InvalidConfig)
        ),
        "{refused:?}"
    );
}
