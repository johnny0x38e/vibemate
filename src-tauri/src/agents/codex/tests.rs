//! Isolated Codex settings tests: temp `$CODEX_HOME` roots, temp SQLite and a
//! synthetic credential store only. Nothing here touches the real `~/.codex`.

use super::*;
use crate::credentials::{CredentialError, Secret};
use crate::model_catalog::{CapabilityOverrides, EffortLevel, ThinkingEffort};
use crate::models::{AddManualModelRequest, add_manual_model};
use crate::providers::{CreateProviderRequest, DeleteProviderRequest, ProviderRecord};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const KEY: &str = "synthetic-codex-key-123";

#[derive(Default)]
struct Store {
    values: RefCell<BTreeMap<String, String>>,
}
impl CredentialStore for Store {
    fn save(&self, key: &str, value: &Secret) -> Result<(), CredentialError> {
        self.values
            .borrow_mut()
            .insert(key.into(), value.expose().into());
        Ok(())
    }
    fn load(&self, key: &str) -> Result<Option<Secret>, CredentialError> {
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
            "vibemate-codex-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let configured = directory.join("agent");
        std::fs::create_dir_all(&configured).unwrap();
        let environment = DiscoveryEnvironment {
            home: directory.join("home"),
            codex_directory: Some(configured.into_os_string()),
            ..Default::default()
        };
        let root = crate::agents::codex_config_root(&environment).0.unwrap();
        Self {
            storage: Storage::open_in_directory(&directory.join("private")).unwrap(),
            backups: BackupStore::new(directory.join("backups")),
            root,
            store: Store::default(),
            environment,
        }
    }
    fn provider(&self, kind: &str, name: &str, base: &str, protocol: &str) -> ProviderRecord {
        crate::providers::create_provider(
            &self.storage,
            &self.store,
            CreateProviderRequest {
                kind: kind.into(),
                display_name: name.into(),
                base_url: base.into(),
                protocol: protocol.into(),
                extensions: BTreeMap::new(),
                secret: Secret::new(KEY.into()),
            },
            1,
        )
        .unwrap()
    }
    fn deepseek(&self) -> ProviderRecord {
        self.provider(
            "deepseek",
            "Deep Seek",
            "https://api.deepseek.com",
            "chat_completions",
        )
    }
    fn model(&self, provider: &ProviderRecord, id: &str, effort: Option<EffortLevel>) {
        add_manual_model(
            &self.storage,
            &AddManualModelRequest {
                provider_id: provider.id.as_str().into(),
                model_id: id.into(),
                alias: None,
                capability_overrides: CapabilityOverrides::none(),
                context_window: None,
                max_output_tokens: None,
                thinking_effort: effort.map(|level| ThinkingEffort::Any {
                    default_level: Some(level),
                }),
            },
            1,
        )
        .unwrap();
    }
    fn path(&self) -> PathBuf {
        self.root.join("config.toml")
    }
    fn write(&self, text: &str) {
        std::fs::write(self.path(), text).unwrap();
    }
    fn text(&self) -> String {
        std::fs::read_to_string(self.path()).unwrap()
    }
    fn selection(
        &self,
        provider: &ProviderRecord,
        action: Action,
        model: Option<&str>,
    ) -> Selection {
        Selection {
            provider_id: provider.id.as_str().into(),
            expected_revision: provider.revision,
            action,
            model_id: model.map(str::to_owned),
        }
    }
    fn preview(&self, selection: &Selection) -> Result<Preview, Error> {
        preview(&self.storage, &self.environment, selection)
    }
    fn confirm(&self, selection: &Selection, approval: String) -> Result<Applied, Error> {
        apply(
            &self.storage,
            &self.store,
            &self.environment,
            &ApplyRequest {
                selection: selection.clone(),
                approval,
            },
            &self.backups,
        )
    }
    fn perform(&self, selection: &Selection) -> Result<Applied, Error> {
        let plan = self.preview(selection)?;
        self.confirm(selection, plan.approval)
    }
    fn state(&self) -> State {
        read_state(&self.storage, &self.environment, &self.backups).unwrap()
    }
    fn repository_default(&self) -> DefaultOwner {
        repository::default_owner(&self.storage, self.root.to_str().unwrap())
            .unwrap()
            .unwrap()
    }
}

const ORIGINAL: &str = "# user config\nmodel = \"gpt-5.5\" # keep me\nmodel_reasoning_effort = \"high\"\nprofile = \"work\"\n\n[profiles.work]\nmodel = \"o3\"\n\n[model_providers.mine]\nname = \"Mine\" # hand written\nbase_url = \"https://example.com/v1\"\nenv_key = \"MINE_KEY\"\n";

#[test]
fn deploy_set_default_clear_and_revoke_restore_original_bytes() {
    let f = Fixture::new();
    f.write(ORIGINAL);
    let provider = f.deepseek();
    f.model(&provider, "deepseek-flash", Some(EffortLevel::High));

    let deploy = f.selection(&provider, Action::Deploy, None);
    let plan = f.preview(&deploy).unwrap();
    // Previews are masked and never read the credential.
    let shown = serde_json::to_string(&plan).unwrap();
    assert!(!shown.contains(KEY));
    assert!(shown.contains(crate::config_plan::SECRET_PLACEHOLDER));
    assert_eq!(plan.provider_key, "vibemate-deep-seek");
    assert!(plan.notices.contains(&Notice::PlaintextCredential));
    assert!(f.confirm(&deploy, plan.approval).unwrap().restart_required);

    let text = f.text();
    assert!(
        text.starts_with(ORIGINAL),
        "unrelated bytes must be preserved"
    );
    let doc: toml_edit::DocumentMut = text.parse().unwrap();
    let table = &doc["model_providers"]["vibemate-deep-seek"];
    assert_eq!(table["wire_api"].as_str(), Some("responses"));
    assert_eq!(table["name"].as_str(), Some("vibemate-Deep Seek"));
    assert_eq!(table["base_url"].as_str(), Some("https://api.deepseek.com"));
    assert_eq!(table["experimental_bearer_token"].as_str(), Some(KEY));
    for absent in ["env_key", "auth", "requires_openai_auth"] {
        assert!(table.get(absent).is_none());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(f.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let state = f.state();
    assert_eq!(state.providers.len(), 1);
    assert_eq!(state.providers[0].state, "written");
    assert!(state.profile_active);
    assert!(state.default.is_none());

    // Set the owned default; comments on the existing `model` line survive.
    let set = f.selection(&provider, Action::SetDefault, Some("deepseek-flash"));
    let plan = f.preview(&set).unwrap();
    assert!(plan.notices.contains(&Notice::ProfileOverridesDefault));
    f.confirm(&set, plan.approval).unwrap();
    let text = f.text();
    assert!(text.contains("model = \"deepseek-flash\" # keep me\n"));
    assert!(text.contains("model_provider = \"vibemate-deep-seek\""));
    assert!(text.contains("model_reasoning_effort = \"high\""));
    assert!(
        text.contains("profile = \"work\""),
        "profile is never rewritten"
    );
    assert!(text.contains("[profiles.work]\nmodel = \"o3\"\n"));
    assert_eq!(f.state().default.unwrap().state, "written");

    // Revoke is refused while the provider is the default.
    let revoke = f.selection(&provider, Action::Revoke, None);
    assert_eq!(f.preview(&revoke).unwrap_err(), Error::DefaultInUse);

    f.perform(&f.selection(&provider, Action::ClearDefault, None))
        .unwrap();
    f.perform(&revoke).unwrap();
    assert_eq!(
        f.text(),
        ORIGINAL,
        "revoke restores the exact original bytes"
    );
    assert!(f.state().providers.is_empty());

    let page = history(
        &f.storage,
        &f.environment,
        &HistoryRequest {
            provider_id: None,
            after: None,
            limit: 10,
        },
    )
    .unwrap();
    let actions: Vec<_> = page.items.iter().map(|e| e.action.as_str()).collect();
    assert_eq!(
        actions,
        ["revoke", "clear_default", "set_default", "deploy"]
    );
    // Default changes name their model; provider-table changes have none.
    let models: Vec<_> = page.items.iter().map(|e| e.model_id.as_deref()).collect();
    assert_eq!(
        models,
        [None, Some("deepseek-flash"), Some("deepseek-flash"), None]
    );
    let json = serde_json::to_value(&page.items[0]).unwrap();
    assert!(json.get("modelId").unwrap().is_null());
    assert_eq!(page.items[1].model_id.as_deref(), Some("deepseek-flash"));
}

#[test]
fn missing_file_is_created_without_profile_keys() {
    let f = Fixture::new();
    let provider = f.provider(
        "openrouter",
        "Router",
        "https://openrouter.ai/api/v1",
        "responses",
    );
    f.perform(&f.selection(&provider, Action::Deploy, None))
        .unwrap();
    let text = f.text();
    assert!(text.starts_with("[model_providers.vibemate-router]\n"));
    assert!(text.contains("supports_websockets = false"));
    assert!(!text.contains("profile"));
    assert!(!text.contains("[model_providers]\n"));
}

#[test]
fn reserved_and_unowned_names_conflict_and_owned_names_get_a_suffix() {
    let f = Fixture::new();
    f.write("[model_providers.vibemate-deep-seek]\nname = \"hand\"\n");
    let first = f.deepseek();
    let plan = f.preview(&f.selection(&first, Action::Deploy, None));
    assert_eq!(plan.unwrap_err(), Error::NameConflict);

    f.write("");
    f.perform(&f.selection(&first, Action::Deploy, None))
        .unwrap();
    let second = f.deepseek();
    let plan = f
        .preview(&f.selection(&second, Action::Deploy, None))
        .unwrap();
    assert_eq!(
        plan.provider_key,
        format!("vibemate-deep-seek-{}", &second.id.as_str()[..6])
    );
    for reserved in RESERVED_PROVIDER_IDS {
        assert!(!reserved.starts_with(KEY_PREFIX));
    }
}

#[test]
fn codex_model_switch_or_mcp_add_makes_the_plan_stale() {
    let f = Fixture::new();
    f.write(ORIGINAL);
    let provider = f.deepseek();
    f.model(&provider, "deepseek-v4-pro", None);
    f.perform(&f.selection(&provider, Action::Deploy, None))
        .unwrap();

    let set = f.selection(&provider, Action::SetDefault, Some("deepseek-v4-pro"));
    let plan = f.preview(&set).unwrap();
    assert!(!plan.notices.contains(&Notice::ReleaseOnly));
    // Codex `codex mcp add` rewrites the file between preview and apply.
    let mut text = f.text();
    text.push_str("\n[mcp_servers.added]\ncommand = \"tool\"\n");
    f.write(&text);
    assert_eq!(
        f.confirm(&set, plan.approval).unwrap_err(),
        Error::PlanStale
    );

    f.perform(&set).unwrap();
    // Codex `/model` changes the default and effort after vibemate wrote them.
    let switched = f
        .text()
        .replace("model = \"deepseek-v4-pro\"", "model = \"gpt-5.5-codex\"")
        .replace(
            "model_reasoning_effort = \"high\"",
            "model_reasoning_effort = \"low\"",
        );
    f.write(&switched);
    assert_eq!(f.state().default.unwrap().state, "modified");
    let clear = f.selection(&provider, Action::ClearDefault, None);
    let plan = f.preview(&clear).unwrap();
    assert!(plan.notices.contains(&Notice::ReleaseOnly));
    assert!(!plan.writes_file);
    let applied = f.confirm(&clear, plan.approval).unwrap();
    assert!(!applied.restart_required);
    assert_eq!(f.text(), switched, "release-only never writes");
    assert!(f.state().default.is_none());
}

#[test]
fn hand_edits_to_owned_table_are_conflicts() {
    let f = Fixture::new();
    let provider = f.deepseek();
    f.perform(&f.selection(&provider, Action::Deploy, None))
        .unwrap();
    let text = f.text().replace(
        "wire_api = \"responses\"",
        "wire_api = \"responses\"\nenv_key = \"OTHER\"",
    );
    f.write(&text);
    assert_eq!(f.state().providers[0].state, "modified");
    for action in [Action::Deploy, Action::Revoke] {
        let error = f
            .preview(&f.selection(&provider, action, None))
            .unwrap_err();
        assert_eq!(error, Error::OwnershipConflict);
    }
}

#[test]
fn unsupported_models_urls_and_targets_are_refused() {
    let f = Fixture::new();
    let provider = f.deepseek();
    f.model(&provider, "deepseek-chat", None);
    f.perform(&f.selection(&provider, Action::Deploy, None))
        .unwrap();
    let set = f.selection(&provider, Action::SetDefault, Some("deepseek-chat"));
    assert_eq!(f.preview(&set).unwrap_err(), Error::ModelUnsupported);

    let custom = f.provider(
        "deepseek",
        "Proxy",
        "https://proxy.example.com/v1",
        "chat_completions",
    );
    let error = f.preview(&f.selection(&custom, Action::Deploy, None));
    assert_eq!(error.unwrap_err(), Error::BaseUrlUnverified);

    let stale = Selection {
        expected_revision: provider.revision + 1,
        ..f.selection(&provider, Action::Deploy, None)
    };
    assert_eq!(f.preview(&stale).unwrap_err(), Error::ProviderChanged);
    assert_eq!(
        f.preview(&f.selection(&provider, Action::ClearDefault, None))
            .unwrap_err(),
        Error::NotDeployed
    );
    let bad = f.confirm(
        &f.selection(&provider, Action::Deploy, None),
        "0".repeat(64),
    );
    assert_eq!(bad.unwrap_err(), Error::PlanStale);
}

#[cfg(unix)]
#[test]
fn symlinked_config_is_refused() {
    let f = Fixture::new();
    let elsewhere = f.root.join("elsewhere.toml");
    std::fs::write(&elsewhere, "").unwrap();
    std::os::unix::fs::symlink(&elsewhere, f.path()).unwrap();
    let provider = f.deepseek();
    let error = f.preview(&f.selection(&provider, Action::Deploy, None));
    assert_eq!(error.unwrap_err(), Error::TargetUnavailable);
}

#[test]
fn missing_credential_is_refused_without_writing() {
    let f = Fixture::new();
    let provider = f.deepseek();
    f.store.values.borrow_mut().clear();
    let deploy = f.selection(&provider, Action::Deploy, None);
    let plan = f.preview(&deploy).unwrap();
    assert_eq!(
        f.confirm(&deploy, plan.approval).unwrap_err(),
        Error::CredentialUnavailable
    );
    assert!(!f.path().exists());
    assert!(f.state().providers.is_empty());
}

#[test]
fn bindings_block_provider_and_model_deletion() {
    let f = Fixture::new();
    let provider = f.deepseek();
    f.model(&provider, "deepseek-flash", None);
    f.perform(&f.selection(&provider, Action::Deploy, None))
        .unwrap();
    f.perform(&f.selection(&provider, Action::SetDefault, Some("deepseek-flash")))
        .unwrap();
    let impact = crate::providers::provider_deletion_impact(&f.storage, provider.id.as_str());
    assert!(!impact.unwrap().can_delete);
    let deleted = crate::providers::delete_provider(
        &f.storage,
        &f.store,
        DeleteProviderRequest {
            id: provider.id.as_str().into(),
            expected_revision: provider.revision,
        },
    );
    assert_eq!(
        deleted.unwrap_err(),
        crate::providers::ProviderError::ProviderInUse
    );
    let impact =
        crate::models::model_deletion_impact(&f.storage, provider.id.as_str(), "deepseek-flash");
    assert!(!impact.unwrap().can_delete);
}

#[test]
fn interrupted_change_needs_explicit_recovery() {
    let f = Fixture::new();
    f.write(ORIGINAL);
    let provider = f.deepseek();
    let built = plan::build(
        &f.storage,
        &f.environment,
        &f.selection(&provider, Action::Deploy, None),
    )
    .unwrap();
    let change = repository::Change {
        id: "a".repeat(32),
        root: plan::root_text(&built.root).unwrap().into(),
        provider_id: provider.id.as_str().into(),
        provider_key: built.view.provider_key.clone(),
        action: "deploy".into(),
        state: "prepared".into(),
        before_digest: built.view.target.content_digest.clone(),
        intended_digest: Some("f".repeat(64)),
        before_owner: built.before.clone(),
        after_owner: built.after.clone(),
        operation_id: None,
        created_at: 1,
        model_id: None,
    };
    repository::prepare(&f.storage, &change, Some(provider.revision)).unwrap();
    // Every write is blocked while an intent is pending, MCP included.
    let error = f.preview(&f.selection(&provider, Action::Deploy, None));
    assert_eq!(error.unwrap_err(), Error::RecoveryRequired);
    let recovery = f.state().recovery.unwrap();
    assert_eq!(recovery.observation, "before");
    let applied = recover(
        &f.storage,
        &f.environment,
        &RecoveryRequest {
            change_id: recovery.id,
            approval: recovery.approval.unwrap(),
        },
        &f.backups,
    )
    .unwrap();
    assert!(!applied.restart_required);
    assert!(f.state().recovery.is_none());
    assert_eq!(f.text(), ORIGINAL);
}

#[test]
fn model_support_follows_documented_routes() {
    let f = Fixture::new();
    let provider = f.provider(
        "command-code",
        "Command",
        "https://api.commandcode.ai/provider/v1",
        "chat_completions",
    );
    f.model(&provider, "cc-model", None);
    let mut model = crate::models::get_model_snapshot(
        &f.storage,
        &crate::models::GetModelSnapshotRequest {
            provider_id: provider.id.as_str().into(),
            model_id: "cc-model".into(),
        },
    )
    .unwrap()
    .model;
    assert!(!plan::model_supported(&provider, &model));
    model.supported_endpoints = Some(vec!["/responses".into()]);
    assert!(plan::model_supported(&provider, &model));
    model.selected = false;
    assert!(!plan::model_supported(&provider, &model));
    assert_eq!(
        DEEPSEEK_RESPONSES_MODELS,
        ["deepseek-flash", "deepseek-v4-pro"]
    );
}

impl Fixture {
    /// Deploy a DeepSeek provider and make `deepseek-flash` the default.
    fn deployed_default(&self) -> ProviderRecord {
        let provider = self.deepseek();
        self.model(&provider, "deepseek-flash", None);
        self.perform(&self.selection(&provider, Action::Deploy, None))
            .unwrap();
        self.perform(&self.selection(&provider, Action::SetDefault, Some("deepseek-flash")))
            .unwrap();
        provider
    }
    fn owned_selection(&self, provider: &ProviderRecord, action: Action) -> Selection {
        let state = self.state();
        let binding = state
            .providers
            .iter()
            .find(|row| row.provider_id == provider.id.as_str())
            .unwrap();
        Selection {
            expected_revision: binding.owner.provider_revision,
            ..self.selection(provider, action, None)
        }
    }
}

#[test]
fn removed_default_is_missing_and_clear_default_only_releases_it() {
    let f = Fixture::new();
    f.write("model = \"gpt-5.5\"\n");
    let provider = f.deployed_default();
    assert_eq!(f.state().default.unwrap().state, "written");

    // Removing only one identity key is a modification, not a removal.
    let written = f.text();
    let partial = written.replace("model = \"deepseek-flash\"\n", "");
    assert_ne!(partial, written);
    f.write(&partial);
    assert_eq!(f.state().default.unwrap().state, "modified");

    // Removing both `model` and `model_provider` by hand is `missing`.
    let removed = partial.replace("model_provider = \"vibemate-deep-seek\"\n", "");
    assert_ne!(removed, partial);
    f.write(&removed);
    let state = f.state();
    let default = state.default.unwrap();
    assert_eq!(default.state, "missing");
    assert_eq!(default.model_id, "deepseek-flash");

    // Clearing never re-adds the pre-vibemate `model = "gpt-5.5"`.
    let clear = f.owned_selection(&provider, Action::ClearDefault);
    let plan = f.preview(&clear).unwrap();
    assert!(plan.notices.contains(&Notice::ReleaseOnly));
    assert!(!plan.writes_file);
    let applied = f.confirm(&clear, plan.approval).unwrap();
    assert!(!applied.restart_required);
    assert_eq!(f.text(), removed, "release-only never writes");
    assert!(f.state().default.is_none());
    // The provider is no longer the default, so it can now be revoked.
    f.perform(&f.owned_selection(&provider, Action::Revoke))
        .unwrap();
    assert!(f.state().providers.is_empty());
}

#[test]
fn deleted_config_file_reports_a_missing_default() {
    let f = Fixture::new();
    f.deployed_default();
    std::fs::remove_file(f.path()).unwrap();
    let state = f.state();
    assert_eq!(state.default.unwrap().state, "missing");
    assert_eq!(state.providers[0].state, "missing");
}

#[test]
fn revoke_and_clear_default_pin_the_owned_binding_revision() {
    let f = Fixture::new();
    let provider = f.deployed_default();
    let edited = crate::providers::update_provider(
        &f.storage,
        &crate::providers::UpdateProviderRequest {
            id: provider.id.as_str().into(),
            expected_revision: provider.revision,
            display_name: "Deep Seek 2".into(),
            base_url: provider.base_url.clone(),
            protocol: provider.protocol.as_str().into(),
            extensions: BTreeMap::new(),
        },
        2,
    )
    .unwrap();
    assert!(edited.revision > provider.revision);
    let state = f.state();
    assert!(state.providers[0].outdated);
    assert_eq!(
        state.providers[0].owner.provider_revision,
        provider.revision
    );

    // The central revision is the wrong token for owned-only actions.
    let central = f.selection(&edited, Action::ClearDefault, None);
    assert_eq!(f.preview(&central).unwrap_err(), Error::ProviderChanged);
    // Deploy/set_default still pin the central revision.
    let stale = f.selection(&provider, Action::Deploy, None);
    assert_eq!(f.preview(&stale).unwrap_err(), Error::ProviderChanged);

    let clear = f.owned_selection(&edited, Action::ClearDefault);
    assert_eq!(clear.expected_revision, provider.revision);
    let plan = f.preview(&clear).unwrap();
    assert_eq!(plan.provider_revision, provider.revision);
    f.confirm(&clear, plan.approval).unwrap();
    f.perform(&f.owned_selection(&edited, Action::Revoke))
        .unwrap();
    assert!(f.state().providers.is_empty());
    assert!(!f.text().contains("vibemate-deep-seek"));
}

#[test]
fn an_owned_table_can_be_revoked_after_its_central_row_is_gone() {
    let f = Fixture::new();
    let provider = f.deepseek();
    f.perform(&f.selection(&provider, Action::Deploy, None))
        .unwrap();
    let revoke = f.owned_selection(&provider, Action::Revoke);
    // Deletion is normally refused while bindings exist; simulate an orphan.
    f.storage
        .lock()
        .unwrap()
        .execute(
            "DELETE FROM provider_instance WHERE id=?1",
            [provider.id.as_str()],
        )
        .unwrap();
    assert_eq!(
        f.preview(&f.selection(&provider, Action::Deploy, None))
            .unwrap_err(),
        Error::ProviderChanged
    );
    f.perform(&revoke).unwrap();
    assert!(f.state().providers.is_empty());
}

#[test]
fn deletion_impacts_list_the_codex_rows_that_block_them() {
    use crate::references::{CodexBindingKind, ReferenceState};
    let f = Fixture::new();
    let provider = f.deployed_default();
    let key = f.state().providers[0].owner.provider_key.clone();
    let impact =
        crate::providers::provider_deletion_impact(&f.storage, provider.id.as_str()).unwrap();
    assert!(!impact.can_delete);
    assert!(impact.references.is_empty(), "Pi/Grok list stays unchanged");
    let rows: Vec<_> = impact
        .codex_references
        .iter()
        .map(|row| {
            (
                row.binding,
                row.provider_key.as_str(),
                row.model_id.as_deref(),
                row.state,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                CodexBindingKind::Provider,
                key.as_str(),
                None,
                ReferenceState::Active
            ),
            (
                CodexBindingKind::Default,
                key.as_str(),
                Some("deepseek-flash"),
                ReferenceState::Active
            ),
        ]
    );
    assert_eq!(
        std::path::Path::new(&impact.codex_references[0].config_root),
        f.path().parent().unwrap()
    );
    let json = serde_json::to_value(&impact).unwrap();
    assert_eq!(json["codexReferences"][1]["binding"], "default");
    // The owned table's digest hashes the bearer token; it never leaves Rust.
    let state = serde_json::to_value(f.state()).unwrap();
    assert_eq!(
        state["providers"][0]["owner"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["generation", "providerKey", "providerRevision"]
    );
    assert_eq!(json["codexReferences"][1]["modelId"], "deepseek-flash");

    let model =
        crate::models::model_deletion_impact(&f.storage, provider.id.as_str(), "deepseek-flash")
            .unwrap();
    assert!(!model.can_delete);
    assert_eq!(model.codex_references.len(), 1);
    assert_eq!(model.codex_references[0].binding, CodexBindingKind::Default);

    // Codex `/model` switched away: the owned default is stale but still
    // blocks until it is cleared in Codex settings.
    let text = f.text().replace(&format!("model_provider = \"{key}\""), "");
    f.write(&text);
    let impact =
        crate::providers::provider_deletion_impact(&f.storage, provider.id.as_str()).unwrap();
    assert_eq!(impact.codex_references[1].state, ReferenceState::Stale);
    assert!(!impact.can_delete);

    // Clearing the default and revoking the table frees both records.
    f.perform(&f.owned_selection(&provider, Action::ClearDefault))
        .unwrap();
    f.perform(&f.owned_selection(&provider, Action::Revoke))
        .unwrap();
    let impact =
        crate::providers::provider_deletion_impact(&f.storage, provider.id.as_str()).unwrap();
    assert!(impact.codex_references.is_empty());
    assert!(impact.can_delete);
}

#[test]
fn a_default_without_central_effort_leaves_the_user_effort_alone() {
    let f = Fixture::new();
    f.write("model = \"gpt-5.5\"\nmodel_reasoning_effort = \"high\" # mine\n");
    let provider = f.deepseek();
    f.model(&provider, "deepseek-v4-pro", None);
    f.perform(&f.selection(&provider, Action::Deploy, None))
        .unwrap();
    f.perform(&f.selection(&provider, Action::SetDefault, Some("deepseek-v4-pro")))
        .unwrap();
    let text = f.text();
    assert!(text.contains("model = \"deepseek-v4-pro\"\n"));
    assert!(
        text.contains("model_reasoning_effort = \"high\" # mine\n"),
        "an unstated effort never removes the user's value"
    );
    assert!(!f.repository_default().writes_effort);

    // The effort is the user's: changing it keeps the default `written`.
    f.write(&text.replace("\"high\" # mine", "\"low\" # mine"));
    assert_eq!(f.state().default.unwrap().state, "written");

    // Clearing restores only `model`/`model_provider`; the user's newer
    // effort stays.
    f.perform(&f.selection(&provider, Action::ClearDefault, None))
        .unwrap();
    let text = f.text();
    assert!(text.starts_with("model = \"gpt-5.5\"\nmodel_reasoning_effort = \"low\" # mine\n"));
    assert!(!text.contains("model_provider ="));
}

#[test]
fn a_central_effort_is_written_and_restored_and_never_lingers() {
    let f = Fixture::new();
    f.write("model = \"gpt-5.5\"\n");
    let provider = f.deepseek();
    f.model(&provider, "deepseek-flash", Some(EffortLevel::Low));
    f.model(&provider, "deepseek-v4-pro", None);
    f.perform(&f.selection(&provider, Action::Deploy, None))
        .unwrap();
    f.perform(&f.selection(&provider, Action::SetDefault, Some("deepseek-flash")))
        .unwrap();
    assert!(f.text().contains("model_reasoning_effort = \"low\""));
    assert!(f.repository_default().writes_effort);

    // Switching to a model without an effort puts back the effort from before
    // vibemate (none here) instead of leaving the flash model's value behind.
    f.perform(&f.selection(&provider, Action::SetDefault, Some("deepseek-v4-pro")))
        .unwrap();
    assert!(!f.text().contains("model_reasoning_effort"));
    assert_eq!(f.state().default.unwrap().state, "written");

    // The user then picks an effort; it is theirs and survives clearing.
    f.write(&format!("model_reasoning_effort = \"high\"\n{}", f.text()));
    assert_eq!(f.state().default.unwrap().state, "written");
    f.perform(&f.selection(&provider, Action::ClearDefault, None))
        .unwrap();
    assert!(
        f.text()
            .starts_with("model_reasoning_effort = \"high\"\nmodel = \"gpt-5.5\"\n")
    );

    // With an effort-writing default, clearing restores the original effort.
    f.perform(&f.selection(&provider, Action::SetDefault, Some("deepseek-flash")))
        .unwrap();
    assert!(f.text().contains("model_reasoning_effort = \"low\""));
    f.perform(&f.selection(&provider, Action::ClearDefault, None))
        .unwrap();
    assert!(f.text().contains("model_reasoning_effort = \"high\""));
}

#[test]
fn snapshots_without_the_effort_flag_owned_the_effort() {
    let legacy = serde_json::json!({
        "provider": null,
        "default": {
            "providerId": "0".repeat(32),
            "modelId": "m",
            "providerKey": "vibemate-demo",
            "entryDigest": "0".repeat(64),
            "previous": {"model": null, "modelProvider": null, "modelReasoningEffort": null},
            "generation": "0".repeat(32),
        }
    });
    let owners: super::Owners = serde_json::from_value(legacy).unwrap();
    assert!(owners.default.unwrap().writes_effort);
}

#[test]
fn provider_previews_mask_base_url_credentials() {
    let placeholder = crate::config_plan::SECRET_PLACEHOLDER;
    let masked = super::plan::mask(&serde_json::json!({
        "name": "Example",
        "base_url": "https://user:pw-url@api.example/v1?api_key=sk-url&api-version=1",
        "wire_api": "responses"
    }));
    let text = masked.to_string();
    assert!(
        !text.contains("pw-url") && !text.contains("sk-url"),
        "{text}"
    );
    assert_eq!(
        masked["base_url"],
        format!("https://user:{placeholder}@api.example/v1?api_key={placeholder}&api-version=1")
    );
    assert_eq!(masked["wire_api"], "responses");
}
