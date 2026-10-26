//! End-to-end Grok entry lifecycle with isolated roots, real SQLite/private
//! snapshots and synthetic OS credentials only. Native TOML is mutated with
//! toml_edit; unrelated model tables and `disabled_mcp_servers` stay intact.

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
            "vibemate-grok-mcp-flow-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let root = directory.join("agent");
        let storage = Storage::open_in_directory(&directory.join("private")).unwrap();
        let environment = DiscoveryEnvironment {
            home: directory.clone(),
            grok_directory: Some(root.clone().into_os_string()),
            ..Default::default()
        };
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
disabled_mcp_servers = ["local-tools", "other"]

[models]
default = "keep-me"

[mcp_servers.user]
command = "never-run-user-command"
env = { PRIVATE = "unowned-synthetic" }
"#,
    );
    let selection = f.selection(&row, Action::Deploy);
    let plan = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
    assert!(plan.disabled_by_user_list);
    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(!serialized.contains("synthetic$DOLLAR"));
    assert!(!serialized.contains("unowned-synthetic"));
    assert_eq!(f.store.loads.get(), 0);
    let deployed = f.perform(&selection);
    assert!(deployed.registered);
    assert_eq!(f.store.loads.get(), 1);
    let doc = f.document();
    assert_eq!(doc["models"]["default"].as_str(), Some("keep-me"));
    assert_eq!(
        doc["mcp_servers"]["user"]["env"]["PRIVATE"].as_str(),
        Some("unowned-synthetic")
    );
    // Disable list is observed, never cleared by deploy.
    assert!(
        doc["disabled_mcp_servers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some("local-tools"))
    );
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
}

#[test]
fn http_credentials_are_literal_and_never_returned_in_preview() {
    let f = Fixture::new();
    let row = f.definition(true);
    f.perform(&f.selection(&row, Action::Deploy));
    assert_eq!(
        f.entry(&row.server_name)["headers"]["x-custom-header"],
        "synthetic$DOLLAR$!"
    );
    assert!(f.entry(&row.server_name).get("type").is_none());
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
fn hyphen_and_underscore_names_are_independent() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.write_toml(
        r#"
[mcp_servers.local_tools]
command = "foreign"
enabled = false
"#,
    );
    // Unlike Pi, local_tools does not collide with local-tools.
    f.perform(&f.selection(&row, Action::Deploy));
    let doc = f.document();
    assert!(
        doc["mcp_servers"]
            .as_table()
            .unwrap()
            .contains_key("local_tools")
    );
    assert!(
        doc["mcp_servers"]
            .as_table()
            .unwrap()
            .contains_key("local-tools")
    );
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
fn restoring_one_entry_keeps_later_user_servers_and_model_tables() {
    let f = Fixture::new();
    let row = f.definition(false);
    let initial = f.perform(&f.selection(&row, Action::Deploy));
    f.perform(&f.selection(&row, Action::Disable));
    let mut doc = f.document();
    doc["mcp_servers"]["later-user"] = toml_edit::table();
    doc["mcp_servers"]["later-user"]["url"] = toml_edit::value("https://example.com/another");
    doc["models"]["default"] = toml_edit::value("later-model");
    f.write_toml(&doc.to_string());
    let mut selection = f.selection(&row, Action::Restore);
    selection.history_id = Some(initial.change_id.clone());
    f.perform(&selection);
    let after = f.document();
    assert_eq!(after["models"]["default"].as_str(), Some("later-model"));
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
    // would expand at Grok load time so apply must refuse without writing.
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
