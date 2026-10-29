//! End-to-end Claude Code entry lifecycle with isolated roots, real SQLite and
//! private snapshots, and synthetic OS credentials only. Fixtures stand in for
//! `.claude.json`; no test reads or writes the user's real file. Byte-level
//! assertions prove that every key outside `mcpServers.<owned>` is unchanged.

use super::*;
use crate::config_apply::{OperationKind, PlannedFile, WriteTarget, apply_private_files_locked};
use crate::credentials::{CredentialError, Secret};
use crate::mcp::{self, ConnectionInput, SaveMcpRequest, SecretInput};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

/// Synthetic `.claude.json` with account/session/project state, a local-scope
/// server sharing the managed name, odd spacing, escapes and non-ASCII text.
const NATIVE: &str = r#"{
  "numStartups": 42,
  "installMethod": "homebrew",
  "oauthAccount": {
    "emailAddress": "synthetic@example.invalid",
    "accountUuid": "00000000-0000-0000-0000-000000000000"
  },
  "tipsHistory" : {"a": 1, "b": 2.50, "c": 1e3},
  "projects": {
    "/tmp/project": {
      "allowedTools": [],
      "mcpServers": {
        "local-tools": {"type": "stdio", "command": "project-local", "args": []}
      },
      "disabledMcpServers": ["local-tools"]
    }
  },
  "mcpServers": {
    "user": {
      "type": "stdio",
      "command": "never-run-user-command",
      "args": [],
      "env": {"PRIVATE": "unowned-synthetic"}
    }
  },
  "userID": "synthetic-user-id",
  "escaped": "\u00e9 \" \\ } ✓"
}
"#;

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
    /// `CLAUDE_CONFIG_DIR` points at an isolated root.
    fn new() -> Self {
        Self::with_override(true)
    }
    fn with_override(config_dir: bool) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "vibemate-claude-mcp-flow-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let home = directory.join("home");
        let override_root = directory.join("claude-config");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&override_root).unwrap();
        let storage = Storage::open_in_directory(&directory.join("private")).unwrap();
        let environment = DiscoveryEnvironment {
            home: home.clone(),
            claude_directory: config_dir.then(|| override_root.clone().into_os_string()),
            ..Default::default()
        };
        Self {
            backups: BackupStore::new(directory.join("backups")),
            directory,
            root: if config_dir { override_root } else { home },
            storage,
            store: Store::default(),
            environment,
        }
    }
    fn save(
        &self,
        server_name: &str,
        enabled: bool,
        connection: ConnectionInput,
    ) -> mcp::McpRecord {
        mcp::save_definition(
            &self.storage,
            &self.store,
            SaveMcpRequest {
                id: None,
                expected_revision: None,
                display_name: "Synthetic tools".into(),
                server_name: server_name.into(),
                enabled,
                connection,
            },
            1,
        )
        .unwrap()
    }
    fn stdio(&self) -> ConnectionInput {
        ConnectionInput::Stdio {
            command: "nonexistent-vibemate-test-server".into(),
            args: vec!["literal space".into(), "$NOT_EXPANDED".into()],
            cwd: None,
            env: vec![SecretInput {
                name: "CUSTOM_VALUE".into(),
                value: Some(Secret::new("synthetic$DOLLAR$!".into())),
            }],
        }
    }
    fn http(&self) -> ConnectionInput {
        ConnectionInput::Http {
            url: "https://example.com/mcp".into(),
            headers: vec![SecretInput {
                name: "X-Custom-Header".into(),
                value: Some(Secret::new("synthetic$DOLLAR$!".into())),
            }],
        }
    }
    fn definition(&self, http: bool) -> mcp::McpRecord {
        if http {
            self.save("remote-tools", true, self.http())
        } else {
            self.save("local-tools", true, self.stdio())
        }
    }
    fn selection(&self, row: &mcp::McpRecord, action: Action) -> Selection {
        Selection {
            definition_id: row.id.clone(),
            expected_revision: row.revision,
            action,
            history_id: None,
        }
    }
    fn try_preview(&self, selection: &Selection) -> Result<Preview, Error> {
        preview(&self.storage, &self.environment, selection, &self.backups)
    }
    fn perform(&self, selection: &Selection) -> Applied {
        let plan = self.try_preview(selection).unwrap();
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
    fn path(&self) -> PathBuf {
        self.root.join(".claude.json")
    }
    fn bytes(&self) -> Vec<u8> {
        std::fs::read(self.path()).unwrap()
    }
    fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes()).unwrap()
    }
    fn entry(&self, name: &str) -> Value {
        self.json()["mcpServers"][name].clone()
    }
    fn write(&self, text: &str) {
        std::fs::write(self.path(), text).unwrap();
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

/// `after` equals `before` plus exactly one contiguous inserted span.
fn inserted(before: &[u8], after: &[u8]) -> String {
    assert!(after.len() > before.len());
    let start = before
        .iter()
        .zip(after)
        .position(|(a, b)| a != b)
        .unwrap_or(before.len());
    let added = after.len() - before.len();
    assert_eq!(&after[..start], &before[..start], "prefix changed");
    assert_eq!(&after[start + added..], &before[start..], "suffix changed");
    String::from_utf8(after[start..start + added].to_vec()).unwrap()
}

#[test]
fn complete_stdio_flow_preserves_every_unowned_byte_and_only_deploy_reads_keys() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.write(NATIVE);
    let selection = f.selection(&row, Action::Deploy);
    let plan = f.try_preview(&selection).unwrap();
    assert!(plan.target.path.ends_with(".claude.json"));
    let serialized = serde_json::to_string(&plan).unwrap();
    for private in [
        "synthetic$DOLLAR",
        "unowned-synthetic",
        "synthetic@example",
        "synthetic-user-id",
        "project-local",
    ] {
        assert!(!serialized.contains(private), "{private}");
    }
    assert_eq!(
        plan.after_entry.as_ref().unwrap()["env"]["CUSTOM_VALUE"],
        "••••••••"
    );
    assert_eq!(f.store.loads.get(), 0);

    let deployed = f.perform(&selection);
    assert!(deployed.registered && deployed.reload_required);
    assert_eq!(f.store.loads.get(), 1);
    let after_deploy = f.bytes();
    let added = inserted(NATIVE.as_bytes(), &after_deploy);
    assert!(
        added.starts_with(",\n    \"local-tools\": {\n      \"args\": ["),
        "{added}"
    );
    assert_eq!(
        f.entry("local-tools"),
        json!({
            "type": "stdio",
            "command": "nonexistent-vibemate-test-server",
            "args": ["literal space", "$NOT_EXPANDED"],
            "env": {"CUSTOM_VALUE": "synthetic$DOLLAR$!"}
        })
    );
    // Local-scope entry with the same name stays a separate, untouched scope.
    assert_eq!(
        f.json()["projects"]["/tmp/project"]["mcpServers"]["local-tools"]["command"],
        "project-local"
    );
    assert_eq!(f.state().deployments[0].state, "written");

    // Revoke and restore never open OS credentials.
    f.store.deny.set(true);
    let revoked = f.perform(&f.selection(&row, Action::Revoke));
    assert!(!revoked.registered);
    assert_eq!(f.bytes(), NATIVE.as_bytes());
    assert_eq!(f.store.loads.get(), 1);
    assert!(f.state().deployments.is_empty());

    let mut restore = f.selection(&row, Action::Restore);
    restore.history_id = Some(deployed.change_id);
    assert!(f.perform(&restore).registered);
    assert_eq!(f.bytes(), after_deploy);
    restore.history_id = Some(revoked.change_id);
    f.perform(&restore);
    assert_eq!(f.bytes(), NATIVE.as_bytes());
    assert!(f.state().deployments.is_empty());
    assert_eq!(f.store.loads.get(), 1);
}

#[test]
fn http_entry_is_typed_literal_private_and_masked_in_preview() {
    let f = Fixture::new();
    let row = f.definition(true);
    f.perform(&f.selection(&row, Action::Deploy));
    assert_eq!(
        f.entry("remote-tools"),
        json!({
            "type": "http",
            "url": "https://example.com/mcp",
            "headers": {"x-custom-header": "synthetic$DOLLAR$!"}
        })
    );
    // Missing file: created with only the container Claude Code reads.
    assert_eq!(
        f.json().as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["mcpServers"]
    );
    for action in [Action::Deploy, Action::Revoke] {
        let plan = f.try_preview(&f.selection(&row, action)).unwrap();
        let text = serde_json::to_string(&plan).unwrap();
        assert!(!text.contains("synthetic"));
        assert!(text.contains("••••••••"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(f.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // Updating with identical credentials is idempotent on the file.
    let before = f.bytes();
    f.perform(&f.selection(&row, Action::Deploy));
    assert_eq!(f.bytes(), before);
}

#[test]
fn default_root_is_home_claude_json_not_the_claude_directory() {
    let f = Fixture::with_override(false);
    let row = f.definition(false);
    f.write("{\"userID\":\"u\",\"projects\":{}}");
    f.perform(&f.selection(&row, Action::Deploy));
    assert!(f.root.join(".claude.json").exists());
    assert!(!f.root.join(".claude").join(".claude.json").exists());
    let added = inserted(b"{\"userID\":\"u\",\"projects\":{}}", &f.bytes());
    assert!(
        added.starts_with(",\"mcpServers\":{\"local-tools\":{"),
        "{added}"
    );
    let state = f.state();
    assert_eq!(state.config_root, f.root.to_str().unwrap());
    assert!(state.target_path.ends_with("/.claude.json"));
    assert!(!state.capabilities.toggle);
    // An unusable override never falls back to the home file.
    let broken = DiscoveryEnvironment {
        home: f.environment.home.clone(),
        claude_directory: Some("relative/dir".into()),
        ..Default::default()
    };
    assert_eq!(
        preview(
            &f.storage,
            &broken,
            &f.selection(&row, Action::Revoke),
            &f.backups
        )
        .err(),
        Some(Error::TargetUnavailable)
    );
}

#[test]
fn toggles_and_unsupported_definitions_are_explicit_errors() {
    let f = Fixture::new();
    f.write(NATIVE);
    let row = f.definition(false);
    for action in [Action::Enable, Action::Disable] {
        assert_eq!(
            f.try_preview(&f.selection(&row, action)).err(),
            Some(Error::ToggleUnsupported)
        );
    }
    f.perform(&f.selection(&row, Action::Deploy));
    for action in [Action::Enable, Action::Disable] {
        assert_eq!(
            f.try_preview(&f.selection(&row, action)).err(),
            Some(Error::ToggleUnsupported)
        );
    }
    let deployed = f.bytes();
    let disabled = f.save("off-tools", false, f.stdio());
    let with_cwd = f.save(
        "cwd-tools",
        true,
        ConnectionInput::Stdio {
            command: "/bin/server".into(),
            args: vec![],
            cwd: Some("/tmp".into()),
            env: vec![],
        },
    );
    let reserved = f.save("workspace", true, f.stdio());
    let dynamic_arg = f.save(
        "dynamic-arg",
        true,
        ConnectionInput::Stdio {
            command: "server".into(),
            args: vec!["${PROBE:-fallback}".into()],
            cwd: None,
            env: vec![],
        },
    );
    let home_path = f.save(
        "home-path",
        true,
        ConnectionInput::Stdio {
            command: "~/bin/server".into(),
            args: vec![],
            cwd: None,
            env: vec![],
        },
    );
    for (row, error) in [
        (&disabled, Error::ToggleUnsupported),
        (&with_cwd, Error::UnsupportedField),
        (&reserved, Error::UnsupportedServerName),
        (&dynamic_arg, Error::DynamicValueUnsupported),
        (&home_path, Error::PathSemanticsUnverified),
    ] {
        assert_eq!(
            f.try_preview(&f.selection(row, Action::Deploy)).err(),
            Some(error),
            "{}",
            row.server_name
        );
    }
    assert_eq!(f.bytes(), deployed);
    // Wire format: enable/disable decode, then fail with the explicit code.
    let wire: Selection = serde_json::from_value(json!({
        "definitionId": row.id, "expectedRevision": row.revision,
        "action": "disable", "historyId": null
    }))
    .unwrap();
    assert_eq!(f.try_preview(&wire).err(), Some(Error::ToggleUnsupported));
    assert_eq!(
        serde_json::to_value(Error::ToggleUnsupported).unwrap(),
        "toggle_unsupported"
    );
    assert!(
        serde_json::from_value::<Selection>(json!({
            "definitionId": row.id, "expectedRevision": 1,
            "action": "deploy", "historyId": null, "path": "/tmp/x"
        }))
        .is_err()
    );
}

#[test]
fn invalid_or_ambiguous_native_documents_are_never_rewritten() {
    let f = Fixture::new();
    let row = f.definition(false);
    for text in [
        "not json",
        "[]",
        "{\"mcpServers\": []}",
        "{\"mcpServers\": {}, \"mcpServers\": {}}",
        "{\"mcpServers\": {\"local-tools\": {}, \"local-tools\": {}}}",
    ] {
        f.write(text);
        assert_eq!(
            f.try_preview(&f.selection(&row, Action::Deploy)).err(),
            Some(Error::InvalidConfig),
            "{text}"
        );
        assert_eq!(f.bytes(), text.as_bytes());
    }
    #[cfg(unix)]
    {
        f.write("{}");
        let link = f.root.join("elsewhere.json");
        std::fs::rename(f.path(), &link).unwrap();
        std::os::unix::fs::symlink(&link, f.path()).unwrap();
        assert_eq!(
            f.try_preview(&f.selection(&row, Action::Deploy)).err(),
            Some(Error::TargetUnavailable)
        );
    }
}

#[test]
fn stale_target_and_central_revision_write_nothing() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.write(NATIVE);
    let selection = f.selection(&row, Action::Deploy);
    let plan = f.try_preview(&selection).unwrap();
    // Claude Code itself bumped a counter after the review.
    let changed = NATIVE.replace("\"numStartups\": 42", "\"numStartups\": 43");
    f.write(&changed);
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
        f.try_preview(&selection).err(),
        Some(Error::DefinitionChanged)
    );
    assert_eq!(f.bytes(), changed.as_bytes());
}

#[test]
fn foreign_name_and_modified_owned_entry_are_never_overwritten() {
    let f = Fixture::new();
    let row = f.definition(false);
    let foreign = "{\"mcpServers\": {\"local-tools\": {\"command\": \"foreign\"}}}";
    f.write(foreign);
    assert_eq!(
        f.try_preview(&f.selection(&row, Action::Deploy)).err(),
        Some(Error::NameConflict)
    );
    assert_eq!(f.bytes(), foreign.as_bytes());
    // Exact names only: underscores and case are different servers.
    f.write("{\"mcpServers\": {\"local_tools\": {\"command\": \"a\"}, \"Local-Tools\": {\"command\": \"b\"}}}");
    f.perform(&f.selection(&row, Action::Deploy));
    assert_eq!(f.entry("local_tools")["command"], "a");
    assert_eq!(f.entry("Local-Tools")["command"], "b");
    let modified = String::from_utf8(f.bytes())
        .unwrap()
        .replace("\"literal space\"", "\"user-changed\"");
    f.write(&modified);
    for action in [Action::Deploy, Action::Revoke] {
        assert_eq!(
            f.try_preview(&f.selection(&row, action)).err(),
            Some(Error::OwnershipConflict)
        );
    }
    assert_eq!(f.state().deployments[0].state, "modified");
    assert_eq!(f.bytes(), modified.as_bytes());
    // Unrelated edits and key reordering inside the entry are not modifications.
    let fresh = Fixture::new();
    let row = fresh.definition(false);
    fresh.perform(&fresh.selection(&row, Action::Deploy));
    let entry = fresh.entry("local-tools");
    fresh.write(&format!(
        "{{\"numStartups\": 7, \"mcpServers\": {{\"local-tools\": {{\"env\": {}, \"type\": \"stdio\", \"args\": {}, \"command\": {}}}}}}}",
        entry["env"], entry["args"], entry["command"]
    ));
    assert_eq!(fresh.state().deployments[0].state, "written");
    fresh.perform(&fresh.selection(&row, Action::Revoke));
    assert_eq!(fresh.bytes(), b"{\"numStartups\": 7, \"mcpServers\": {}}");
}

#[test]
fn deployment_and_pending_intents_block_central_deletion_until_revoke() {
    let f = Fixture::new();
    let row = f.definition(true);
    f.perform(&f.selection(&row, Action::Deploy));
    let delete = || mcp::DeleteMcpRequest {
        id: row.id.clone(),
        expected_revision: row.revision,
    };
    assert_eq!(
        mcp::delete_definition(&f.storage, &f.store, delete()).err(),
        Some(mcp::McpError::McpInUse)
    );
    f.perform(&f.selection(&row, Action::Revoke));
    mcp::delete_definition(&f.storage, &f.store, delete()).unwrap();
}

#[test]
fn restoring_one_entry_keeps_later_user_servers_and_unrelated_keys() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.write(NATIVE);
    let initial = f.perform(&f.selection(&row, Action::Deploy));
    f.perform(&f.selection(&row, Action::Revoke));
    // After revocation, Claude Code and the user changed other state.
    let later = NATIVE
        .replace("\"numStartups\": 42", "\"numStartups\": 99")
        .replace(
            "  \"mcpServers\": {\n",
            "  \"mcpServers\": {\n    \"later-user\": {\"type\": \"http\", \"url\": \"https://example.com/another\"},\n",
        );
    f.write(&later);
    let mut selection = f.selection(&row, Action::Restore);
    selection.history_id = Some(initial.change_id);
    f.store.deny.set(true);
    f.perform(&selection);
    let added = inserted(later.as_bytes(), &f.bytes());
    assert!(added.contains("\"local-tools\""), "{added}");
    assert_eq!(f.json()["numStartups"], 99);
    assert_eq!(f.entry("later-user")["url"], "https://example.com/another");
    assert_eq!(
        f.entry("local-tools")["env"]["CUSTOM_VALUE"],
        "synthetic$DOLLAR$!"
    );
    let history = history(
        &f.storage,
        &f.environment,
        &HistoryRequest {
            definition_id: row.id.clone(),
            after: None,
            limit: 10,
        },
    )
    .unwrap();
    let actions: Vec<_> = history
        .items
        .iter()
        .map(|i| (i.action.as_str(), i.registered))
        .collect();
    assert_eq!(
        actions,
        [("restore", true), ("revoke", false), ("deploy", true)]
    );
}

fn prepared_change(
    f: &Fixture,
    row: &mcp::McpRecord,
    plan: &Preview,
    intended: Option<&[u8]>,
    entry: &Value,
) -> repository::Change {
    let change_id = repository::random_id(&f.storage).unwrap();
    let change = repository::Change {
        id: change_id.clone(),
        root: f.root.to_str().unwrap().to_string(),
        definition_id: row.id.clone(),
        server_name: row.server_name.clone(),
        definition_revision: row.revision,
        action: "deploy".into(),
        state: "prepared".into(),
        before_digest: plan.target.content_digest.clone(),
        intended_digest: Some(
            intended.map_or_else(|| "a".repeat(64), crate::config_plan::digest_hex),
        ),
        before_owner: None,
        after_owner: Some(Ownership {
            server_name: row.server_name.clone(),
            definition_revision: row.revision,
            entry_digest: plan::entry_digest(entry),
            generation: change_id,
        }),
        operation_id: None,
        created_at: 1,
    };
    repository::prepare(&f.storage, &change).unwrap();
    change
}

#[test]
fn pending_before_observation_can_be_abandoned_without_replay() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.write(NATIVE);
    let selection = f.selection(&row, Action::Deploy);
    let plan = f.try_preview(&selection).unwrap();
    let change = prepared_change(&f, &row, &plan, None, &json!({}));
    assert_eq!(
        f.try_preview(&selection).err(),
        Some(Error::RecoveryRequired)
    );
    let delete = mcp::DeleteMcpRequest {
        id: row.id.clone(),
        expected_revision: row.revision,
    };
    assert_eq!(
        mcp::delete_definition(&f.storage, &f.store, delete).err(),
        Some(mcp::McpError::McpInUse)
    );
    let recovery = f.state().recovery.unwrap();
    assert_eq!(recovery.observation, "before");
    let recovered = f.recovery();
    assert_eq!(recovered.change_id, change.id);
    assert!(!recovered.registered);
    assert_eq!(f.bytes(), NATIVE.as_bytes());
    f.perform(&selection);
    assert_eq!(f.state().deployments[0].state, "written");
}

#[test]
fn interrupted_write_is_reconciled_from_the_intended_observation() {
    let f = Fixture::new();
    let row = f.definition(false);
    f.write(NATIVE);
    let selection = f.selection(&row, Action::Deploy);
    let plan = f.try_preview(&selection).unwrap();
    // Simulate a crash after the shared writer renamed the file but before the
    // domain finalized ownership: intent is prepared and the journal applied.
    let entry = json!({"type": "stdio", "command": "nonexistent-vibemate-test-server",
        "args": ["literal space", "$NOT_EXPANDED"], "env": {"CUSTOM_VALUE": "synthetic$DOLLAR$!"}});
    let content = json_edit::parse(NATIVE.as_bytes())
        .unwrap()
        .with_entry("local-tools", Some(&entry))
        .unwrap();
    let change = prepared_change(&f, &row, &plan, Some(&content), &entry);
    {
        let guard = f.storage.lock_configuration().unwrap();
        apply_private_files_locked(
            &f.storage,
            OperationKind::ClaudeMcp,
            Some(&change.id),
            &[PlannedFile {
                target: WriteTarget::from_plan(&plan.target),
                content: content.clone(),
            }],
            &f.backups,
            &guard,
        )
        .unwrap();
    }
    assert_eq!(f.bytes(), content);
    let observed = f.state();
    assert!(observed.deployments.is_empty());
    let recovery = observed.recovery.unwrap();
    assert_eq!(
        (recovery.observation.as_str(), recovery.action.as_str()),
        ("intended", "deploy")
    );
    // A stale approval is refused.
    assert_eq!(
        recover(
            &f.storage,
            &f.environment,
            &RecoveryRequest {
                change_id: recovery.id.clone(),
                approval: "0".repeat(64),
            },
            &f.backups,
        )
        .err(),
        Some(Error::PlanStale)
    );
    let recovered = f.recovery();
    assert!(recovered.registered);
    assert_eq!(f.state().deployments[0].state, "written");
    assert!(f.state().recovery.is_none());
    // The reconciled result is a restorable history entry.
    f.perform(&f.selection(&row, Action::Revoke));
    assert_eq!(f.bytes(), NATIVE.as_bytes());
    let mut restore = f.selection(&row, Action::Restore);
    restore.history_id = Some(change.id);
    f.store.deny.set(true);
    f.perform(&restore);
    assert_eq!(f.bytes(), content);
}

#[test]
fn dynamic_secret_values_are_refused_without_writing() {
    let f = Fixture::new();
    let row = f.definition(true);
    // Central validation rejects `${`; simulate a later OS-store change that
    // Claude Code would expand at load time so apply must refuse.
    let keys: Vec<String> = f.store.values.borrow().keys().cloned().collect();
    {
        let mut values = f.store.values.borrow_mut();
        for key in keys {
            values.insert(key, "${HOME}".into());
        }
    }
    let selection = f.selection(&row, Action::Deploy);
    let plan = f.try_preview(&selection).unwrap();
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
    assert!(!f.path().exists());
}
