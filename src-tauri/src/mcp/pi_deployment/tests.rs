//! End-to-end Pi entry lifecycle and crash-window tests with isolated roots,
//! real SQLite/private snapshots and synthetic OS credentials only.

use super::*;
use crate::credentials::{CredentialError, Secret};
use crate::mcp::{self, ConnectionInput, SaveMcpRequest, SecretInput};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

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
            "vibemate-pi-mcp-flow-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let root = directory.join("agent");
        let storage = Storage::open_in_directory(&directory.join("private")).unwrap();
        let environment = DiscoveryEnvironment {
            home: directory.clone(),
            pi_directory: Some(root.clone().into_os_string()),
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
    fn native(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.root.join("mcp.json")).unwrap()).unwrap()
    }
    fn write(&self, value: &Value) {
        std::fs::create_dir_all(&self.root).unwrap();
        std::fs::write(
            self.root.join("mcp.json"),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
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
fn complete_stdio_flow_preserves_unowned_content_and_only_deploy_reads_keys() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.write(&json!({"autoEnableCodemode":false,"custom":{"keep":true},"mcpServers":{"user":{"command":"never-run-user-command","env":{"PRIVATE":"unowned-synthetic"}}}}));
    let selection = f.selection(&row, Action::Deploy);
    let plan = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(!serialized.contains("synthetic$DOLLAR"));
    assert!(!serialized.contains("unowned-synthetic"));
    assert_eq!(f.store.loads.get(), 0);
    let deployed = f.perform(&selection);
    assert!(deployed.registered);
    assert_eq!(f.store.loads.get(), 1);
    let native = f.native();
    assert_eq!(native["autoEnableCodemode"], false);
    assert_eq!(native["custom"], json!({"keep":true}));
    assert_eq!(
        native["mcpServers"]["user"]["env"]["PRIVATE"],
        "unowned-synthetic"
    );
    assert_eq!(
        native["mcpServers"][&row.server_name]["env"]["CUSTOM_VALUE"],
        "synthetic$$DOLLAR$$!"
    );
    f.store.deny.set(true);
    f.perform(&f.selection(&row, Action::Disable));
    assert_eq!(f.native()["mcpServers"][&row.server_name]["enabled"], false);
    f.perform(&f.selection(&row, Action::Enable));
    assert_eq!(f.native()["mcpServers"][&row.server_name]["enabled"], true);
    let revoked = f.perform(&f.selection(&row, Action::Revoke));
    assert!(!revoked.registered);
    assert!(f.native()["mcpServers"].get(&row.server_name).is_none());
    assert_eq!(f.store.loads.get(), 1);
    assert!(f.state().deployments.is_empty());
    let mut restore = f.selection(&row, Action::Restore);
    restore.history_id = Some(deployed.change_id);
    f.perform(&restore);
    assert_eq!(
        f.native()["mcpServers"][&row.server_name],
        native["mcpServers"][&row.server_name]
    );
    assert_eq!(f.store.loads.get(), 1);
    restore.history_id = Some(revoked.change_id);
    f.perform(&restore);
    assert!(f.state().deployments.is_empty());
}

#[test]
fn http_credentials_are_literal_private_and_never_returned_in_preview() {
    let f = Fixture::new();
    let row = f.definition(true);
    f.perform(&f.selection(&row, Action::Deploy));
    assert_eq!(
        f.native()["mcpServers"][&row.server_name]["headers"]["x-custom-header"],
        "synthetic$$DOLLAR$$!"
    );
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
            std::fs::metadata(f.root.join("mcp.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn stale_target_and_central_revision_write_nothing() {
    let f = Fixture::new();
    let row = f.definition(false);
    let selection = f.selection(&row, Action::Deploy);
    let plan = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
    f.write(&json!({"unrelated":true}));
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
    assert_eq!(f.native(), json!({"unrelated":true}));
}

#[test]
fn foreign_namespace_and_modified_owned_entry_are_never_overwritten() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.write(&json!({"mcpServers":{"local_tools":{"enabled":false}}}));
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
    f.write(&json!({}));
    f.perform(&f.selection(&row, Action::Deploy));
    let mut doc = f.native();
    doc["mcpServers"][&row.server_name]["args"] = json!(["user-changed"]);
    f.write(&doc);
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
    assert_eq!(f.native(), doc);
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
fn restoring_one_entry_keeps_later_user_servers_and_uses_its_own_snapshot() {
    let f = Fixture::new();
    let row = f.definition(false);
    let initial = f.perform(&f.selection(&row, Action::Deploy));
    f.perform(&f.selection(&row, Action::Disable));
    let mut doc = f.native();
    doc["mcpServers"]["later-user"] = json!({"url":"https://example.com/another"});
    doc["custom"] = json!("later-value");
    f.write(&doc);
    let mut selection = f.selection(&row, Action::Restore);
    selection.history_id = Some(initial.change_id.clone());
    f.perform(&selection);
    let after = f.native();
    assert_eq!(after["custom"], "later-value");
    assert_eq!(
        after["mcpServers"]["later-user"],
        doc["mcpServers"]["later-user"]
    );
    assert_eq!(after["mcpServers"][&row.server_name]["enabled"], true);
    let operation: String = f
        .storage
        .lock()
        .unwrap()
        .query_row(
            "SELECT operation_id FROM pi_mcp_change WHERE id=?1",
            [initial.change_id],
            |r| r.get(0),
        )
        .unwrap();
    let path: String = f
        .storage
        .lock()
        .unwrap()
        .query_row(
            "SELECT copy_path FROM config_version_file WHERE version_id=?1",
            [operation],
            |r| r.get(0),
        )
        .unwrap();
    std::fs::write(path, b"corrupt").unwrap();
    assert_eq!(
        preview(&f.storage, &f.environment, &selection, &f.backups).err(),
        Some(Error::SnapshotUnavailable)
    );
    assert_eq!(f.native(), after);
}

#[test]
fn a_retained_result_cannot_borrow_another_operations_identical_copy() {
    let f = Fixture::new();
    let row = f.definition(false);
    let first = f.perform(&f.selection(&row, Action::Deploy));
    let second = f.perform(&f.selection(&row, Action::Deploy));
    let db = f.storage.lock().unwrap();
    let copy:(String,String)=db.query_row("SELECT f.copy_id,f.copy_path FROM config_version_file f JOIN pi_mcp_change c ON c.operation_id=f.version_id WHERE c.id=?1",[second.change_id],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    // Simulate corrupt metadata after releasing the unique copy-id reference;
    // even identical bytes must not substitute another operation's owned copy.
    db.execute(
        "UPDATE config_version_file SET copy_id='unavailable-other-owner' WHERE copy_id=?1",
        [&copy.0],
    )
    .unwrap();
    db.execute("UPDATE config_version_file SET copy_id=?1,copy_path=?2 WHERE version_id=(SELECT operation_id FROM pi_mcp_change WHERE id=?3)",rusqlite::params![copy.0,copy.1,first.change_id]).unwrap();
    drop(db);
    let mut selection = f.selection(&row, Action::Restore);
    selection.history_id = Some(first.change_id);
    assert_eq!(
        preview(&f.storage, &f.environment, &selection, &f.backups).err(),
        Some(Error::SnapshotUnavailable)
    );
}

#[test]
fn cross_instance_configuration_lock_spans_revalidation_and_prevents_stale_writes() {
    use std::{sync::mpsc, time::Duration};
    let f = Fixture::new();
    let row = f.definition(false);
    let selection = f.selection(&row, Action::Deploy);
    let preview = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
    let guard = f.storage.lock_configuration().unwrap();
    let directory = f.directory.clone();
    let root = f.root.clone();
    let values = f.store.values.borrow().clone();
    let (started, started_rx) = mpsc::channel();
    let (done, done_rx) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let storage = Storage::open_in_directory(&directory.join("private")).unwrap();
        let environment = DiscoveryEnvironment {
            home: directory.clone(),
            pi_directory: Some(root.into_os_string()),
            ..Default::default()
        };
        let store = Store {
            values: RefCell::new(values),
            ..Default::default()
        };
        started.send(()).unwrap();
        let result = apply(
            &storage,
            &store,
            &environment,
            &ApplyRequest {
                selection,
                approval: preview.approval,
            },
            &BackupStore::new(directory.join("backups")),
        );
        done.send(result.err()).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        done_rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    f.sql(&format!(
        "UPDATE mcp_definition SET revision=revision+1 WHERE id='{}'",
        row.id
    ));
    drop(guard);
    assert_eq!(
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        Some(Error::DefinitionChanged)
    );
    thread.join().unwrap();
    assert!(!f.root.exists());
    assert!(f.state().recovery.is_none());
}

#[test]
fn missing_or_dynamic_os_credentials_fail_before_any_intent_or_write() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.store.deny.set(true);
    let selection = f.selection(&row, Action::Deploy);
    let plan = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
    let apply_current = || {
        apply(
            &f.storage,
            &f.store,
            &f.environment,
            &ApplyRequest {
                selection: selection.clone(),
                approval: plan.approval.clone(),
            },
            &f.backups,
        )
    };
    assert_eq!(apply_current().err(), Some(Error::CredentialUnavailable));
    assert!(!f.root.exists());
    assert!(f.state().recovery.is_none());
    f.store.deny.set(false);
    for value in f.store.values.borrow_mut().values_mut() {
        *value = "!must-never-run".into();
    }
    assert_eq!(apply_current().err(), Some(Error::CredentialUnavailable));
    assert!(!f.root.exists());
}

#[test]
fn history_is_paginated_and_cannot_select_another_root_or_definition() {
    let f = Fixture::new();
    let row = f.definition(false);
    let first = f.perform(&f.selection(&row, Action::Deploy));
    for action in [Action::Disable, Action::Enable, Action::Disable] {
        f.perform(&f.selection(&row, action));
    }
    let mut request = HistoryRequest {
        definition_id: row.id.clone(),
        after: None,
        limit: 2,
    };
    let page = history(&f.storage, &f.environment, &request).unwrap();
    assert_eq!(page.items.len(), 2);
    request.after = page.next_cursor;
    assert!(request.after.is_some());
    let second = history(&f.storage, &f.environment, &request).unwrap();
    assert_eq!(second.items.len(), 2);
    assert!(second.next_cursor.is_none());
    assert!(
        page.items
            .iter()
            .all(|a| second.items.iter().all(|b| a.id != b.id))
    );
    let other = f.definition(true);
    let mut selection = f.selection(&other, Action::Restore);
    selection.history_id = Some(first.change_id.clone());
    assert_eq!(
        preview(&f.storage, &f.environment, &selection, &f.backups).err(),
        Some(Error::SnapshotUnavailable)
    );
    selection.definition_id = row.id;
    let environment = DiscoveryEnvironment {
        home: f.directory.clone(),
        pi_directory: Some(f.directory.join("other-root").into_os_string()),
        ..Default::default()
    };
    assert_eq!(
        preview(&f.storage, &environment, &selection, &f.backups).err(),
        Some(Error::SnapshotUnavailable)
    );
}

#[test]
fn two_roots_have_independent_ownership_and_all_references_must_be_revoked() {
    let f = Fixture::new();
    let row = f.definition(false);
    let selection = f.selection(&row, Action::Deploy);
    f.perform(&selection);
    let other = DiscoveryEnvironment {
        home: f.directory.clone(),
        pi_directory: Some(f.directory.join("second-agent").into_os_string()),
        ..Default::default()
    };
    let plan = preview(&f.storage, &other, &selection, &f.backups).unwrap();
    apply(
        &f.storage,
        &f.store,
        &other,
        &ApplyRequest {
            selection,
            approval: plan.approval,
        },
        &f.backups,
    )
    .unwrap();
    f.perform(&f.selection(&row, Action::Revoke));
    assert_eq!(
        mcp::delete_definition(
            &f.storage,
            &f.store,
            mcp::DeleteMcpRequest {
                id: row.id.clone(),
                expected_revision: row.revision
            }
        )
        .err(),
        Some(mcp::McpError::McpInUse)
    );
    let state = read_state(
        &f.storage,
        &f.environment,
        &StateRequest {
            definition_id: Some(row.id.clone()),
            after: None,
            limit: 20,
        },
        &f.backups,
    )
    .unwrap();
    assert_eq!(state.deployments.len(), 1);
    assert_eq!(
        state.deployments[0].config_root,
        other.pi_directory.unwrap().to_str().unwrap()
    );
}

#[test]
fn an_externally_removed_entry_can_be_explicitly_revoked_without_recreating_a_file() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.perform(&f.selection(&row, Action::Deploy));
    std::fs::remove_file(f.root.join("mcp.json")).unwrap();
    assert_eq!(f.state().deployments[0].state, "missing");
    f.perform(&f.selection(&row, Action::Revoke));
    assert!(!f.root.join("mcp.json").exists());
    assert!(f.state().deployments.is_empty());
    mcp::delete_definition(
        &f.storage,
        &f.store,
        mcp::DeleteMcpRequest {
            id: row.id,
            expected_revision: row.revision,
        },
    )
    .unwrap();
}

#[test]
fn revoked_generation_invalidates_old_approval_even_when_bytes_return_exactly() {
    let f = Fixture::new();
    let row = f.definition(false);
    std::fs::create_dir_all(&f.root).unwrap();
    let mut original = serde_json::to_vec_pretty(&json!({"mcpServers":{}})).unwrap();
    original.push(b'\n');
    std::fs::write(f.root.join("mcp.json"), &original).unwrap();
    let selection = f.selection(&row, Action::Deploy);
    let old = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
    f.perform(&selection);
    f.perform(&f.selection(&row, Action::Revoke));
    assert_eq!(std::fs::read(f.root.join("mcp.json")).unwrap(), original);
    assert_eq!(
        apply(
            &f.storage,
            &f.store,
            &f.environment,
            &ApplyRequest {
                selection,
                approval: old.approval
            },
            &f.backups
        )
        .err(),
        Some(Error::PlanStale)
    );
}

#[test]
fn repeated_approval_and_changed_native_history_are_refused() {
    let f = Fixture::new();
    let row = f.definition(false);
    let selection = f.selection(&row, Action::Deploy);
    let plan = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
    let request = ApplyRequest {
        selection,
        approval: plan.approval,
    };
    apply(&f.storage, &f.store, &f.environment, &request, &f.backups).unwrap();
    assert_eq!(
        apply(&f.storage, &f.store, &f.environment, &request, &f.backups).err(),
        Some(Error::PlanStale)
    );
    let first = history(
        &f.storage,
        &f.environment,
        &HistoryRequest {
            definition_id: row.id.clone(),
            after: None,
            limit: 20,
        },
    )
    .unwrap()
    .items[0]
        .id
        .clone();
    let mut restore = f.selection(&row, Action::Restore);
    restore.history_id = Some(first);
    let preview = preview(&f.storage, &f.environment, &restore, &f.backups).unwrap();
    let mut document = f.native();
    document["user-change"] = json!(true);
    f.write(&document);
    assert_eq!(
        apply(
            &f.storage,
            &f.store,
            &f.environment,
            &ApplyRequest {
                selection: restore,
                approval: preview.approval
            },
            &f.backups
        )
        .err(),
        Some(Error::PlanStale)
    );
    assert_eq!(f.native(), document);
}

#[cfg(unix)]
#[test]
fn symlink_and_read_only_targets_never_enter_the_writer() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let row = f.definition(false);
    f.write(&json!({}));
    let path = f.root.join("mcp.json");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
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
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::remove_file(&path).unwrap();
    let other = f.directory.join("native.json");
    std::fs::write(&other, b"{}").unwrap();
    std::os::unix::fs::symlink(&other, &path).unwrap();
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
    assert_eq!(std::fs::read(&other).unwrap(), b"{}");
    assert!(path.is_symlink());
}

#[test]
fn crash_before_writer_is_explicitly_abandoned_after_restart_without_replay() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.sql("CREATE TRIGGER injected_before_writer BEFORE INSERT ON config_operation BEGIN SELECT RAISE(ABORT,'injected'); END;");
    let selection = f.selection(&row, Action::Deploy);
    let plan = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
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
        Some(Error::RecoveryRequired)
    );
    assert!(!f.root.exists());
    let state = f.state();
    assert_eq!(state.recovery.unwrap().observation, "before");
    assert_eq!(
        mcp::delete_definition(
            &f.storage,
            &f.store,
            mcp::DeleteMcpRequest {
                id: row.id.clone(),
                expected_revision: row.revision
            }
        )
        .err(),
        Some(mcp::McpError::McpInUse)
    );
    f.sql("DROP TRIGGER injected_before_writer;");
    let reopened = Storage::open_in_directory(&f.directory.join("private")).unwrap();
    let state = read_state(
        &reopened,
        &f.environment,
        &StateRequest {
            definition_id: None,
            after: None,
            limit: 20,
        },
        &f.backups,
    )
    .unwrap();
    let pending = state.recovery.unwrap();
    recover(
        &reopened,
        &f.environment,
        &RecoveryRequest {
            change_id: pending.id,
            approval: pending.approval.unwrap(),
        },
        &f.backups,
    )
    .unwrap();
    assert!(!f.root.exists());
    f.perform(&selection);
}

#[test]
fn crash_after_native_write_recovers_journal_and_ownership_without_rewriting() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.sql("CREATE TRIGGER injected_after_write BEFORE UPDATE OF state ON config_operation_step WHEN NEW.state='applied' BEGIN SELECT RAISE(ABORT,'injected'); END;");
    let selection = f.selection(&row, Action::Deploy);
    let plan = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
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
        Some(Error::RecoveryRequired)
    );
    let written = std::fs::read(f.root.join("mcp.json")).unwrap();
    assert_eq!(f.state().recovery.unwrap().observation, "intended");
    f.sql("DROP TRIGGER injected_after_write;");
    let recovered = f.recovery();
    assert!(recovered.registered);
    assert_eq!(std::fs::read(f.root.join("mcp.json")).unwrap(), written);
    assert!(f.state().recovery.is_none());
    assert_eq!(f.state().deployments[0].state, "written");
    let mut restore = f.selection(&row, Action::Restore);
    restore.history_id = Some(recovered.change_id);
    preview(&f.storage, &f.environment, &restore, &f.backups).unwrap();
}

#[test]
fn missing_recovery_result_copy_reports_its_cause_and_retains_the_intent() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.sql("CREATE TRIGGER injected_finalize BEFORE UPDATE OF state ON pi_mcp_change WHEN NEW.state='completed' BEGIN SELECT RAISE(ABORT,'injected'); END;");
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
        Some(Error::OutcomeUnknown)
    );
    f.sql("DROP TRIGGER injected_finalize;");
    let pending = f.state().recovery.unwrap();
    let path:String=f.storage.lock().unwrap().query_row("SELECT f.copy_path FROM config_version_file f JOIN config_operation o ON o.id=f.version_id WHERE o.owner_ref=?1",[&pending.id],|r|r.get(0)).unwrap();
    std::fs::remove_file(path).unwrap();
    let written = std::fs::read(f.root.join("mcp.json")).unwrap();
    assert_eq!(
        recover(
            &f.storage,
            &f.environment,
            &RecoveryRequest {
                change_id: pending.id,
                approval: pending.approval.unwrap()
            },
            &f.backups
        )
        .err(),
        Some(Error::SnapshotUnavailable)
    );
    assert_eq!(std::fs::read(f.root.join("mcp.json")).unwrap(), written);
    assert!(f.state().deployments.is_empty());
    assert!(f.state().recovery.is_some());
}

#[test]
fn crash_after_writer_before_metadata_preserves_pending_and_refuses_external_divergence() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.sql("CREATE TRIGGER injected_finalize BEFORE UPDATE OF state ON pi_mcp_change WHEN NEW.state='completed' BEGIN SELECT RAISE(ABORT,'injected'); END;");
    let selection = f.selection(&row, Action::Deploy);
    let plan = preview(&f.storage, &f.environment, &selection, &f.backups).unwrap();
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
        Some(Error::OutcomeUnknown)
    );
    let intended = std::fs::read(f.root.join("mcp.json")).unwrap();
    let old = f.state().recovery.unwrap();
    f.sql("DROP TRIGGER injected_finalize;");
    f.write(&json!({"externally":"changed"}));
    let pending = f.state().recovery.unwrap();
    assert_eq!(pending.observation, "conflict");
    assert!(pending.approval.is_none());
    assert_eq!(
        recover(
            &f.storage,
            &f.environment,
            &RecoveryRequest {
                change_id: old.id,
                approval: old.approval.unwrap()
            },
            &f.backups
        )
        .err(),
        Some(Error::PlanStale)
    );
    assert_eq!(
        preview(&f.storage, &f.environment, &selection, &f.backups).err(),
        Some(Error::RecoveryRequired)
    );
    std::fs::write(f.root.join("mcp.json"), intended).unwrap();
    f.recovery();
    assert_eq!(f.state().deployments.len(), 1);
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
