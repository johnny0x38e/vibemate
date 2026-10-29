//! Temporary-directory tests for Pi and Grok skill deploy.

use super::{
    apply_skill_deploy, disable_skill_deploy, dismiss_skill_deploy_recovery,
    finish_skill_deploy_recovery, list_agent_skill_deployments, list_skill_deploy_recoveries,
    preview_skill_deploy, refresh_skill_deployments_after_update,
};
use crate::agents::DiscoveryEnvironment;
use crate::skills::manifest::MANIFEST_FILE_NAME;
use crate::skills::service::{self, import_one};
use crate::skills::types::{
    DeleteSkillRequest, DisableSkillDeployRequest, ReviewedSkillDeploy, SkillDeployAgent,
    SkillDeployMode, SkillDeployOperationKind, SkillDeployRecoveryAction,
    SkillDeployRecoveryClassification, SkillDeployTargetState, SkillError,
    SkillUpdateDeployOutcome,
};
use crate::skills::{AgentSkillTargetState, SkillLibrary, delete_skill};
use crate::storage::Storage;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const NOW: i64 = 1_700_000_000_000;

struct Fixture {
    root: PathBuf,
    storage: Storage,
    library: SkillLibrary,
    home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "vibemate-skill-deploy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let storage = Storage::open_in_directory(&root).expect("storage");
        let library = SkillLibrary::new(root.join("skills"));
        let home = root.join("home");
        fs::create_dir_all(home.join(".pi").join("agent")).expect("pi root");
        fs::create_dir_all(home.join(".grok")).expect("grok root");
        fs::create_dir_all(home.join(".claude")).expect("claude root");
        fs::create_dir_all(home.join(".codex")).expect("codex root");
        Self {
            root,
            storage,
            library,
            home,
        }
    }

    fn environment(&self) -> DiscoveryEnvironment {
        DiscoveryEnvironment {
            home: self.home.clone(),
            ..DiscoveryEnvironment::default()
        }
    }

    fn import(&self, name: &str) -> crate::skills::SkillRecord {
        let source = self.root.join("sources").join(name);
        fs::create_dir_all(&source).expect("source");
        fs::write(
            source.join(MANIFEST_FILE_NAME),
            format!("---\nname: {name}\ndescription: A test skill.\n---\n\n# {name}\n"),
        )
        .expect("manifest");
        import_one(&self.library, &self.storage, &source, NOW).expect("import")
    }

    fn preview(
        &self,
        agent: SkillDeployAgent,
        skill_id: &str,
    ) -> Result<crate::skills::SkillDeployPreview, SkillError> {
        preview_skill_deploy(
            &self.library,
            &self.storage,
            &self.environment(),
            agent,
            skill_id,
        )
    }

    fn apply(
        &self,
        reviewed: ReviewedSkillDeploy,
    ) -> Result<crate::skills::SkillDeployed, SkillError> {
        apply_skill_deploy(
            &self.library,
            &self.storage,
            &self.environment(),
            reviewed,
            NOW,
        )
    }

    fn disable(
        &self,
        agent: SkillDeployAgent,
        skill_id: &str,
        config_root: &str,
    ) -> Result<crate::skills::SkillDeployDisabled, SkillError> {
        disable_skill_deploy(
            &self.library,
            &self.storage,
            &self.environment(),
            DisableSkillDeployRequest {
                agent_id: agent,
                skill_id: skill_id.to_string(),
                config_root: config_root.to_string(),
            },
        )
    }

    fn target(&self, agent: SkillDeployAgent, name: &str) -> PathBuf {
        match agent {
            // Join per segment so the expected path uses the native separator.
            SkillDeployAgent::Pi => self
                .home
                .join(".pi")
                .join("agent")
                .join("skills")
                .join(name),
            SkillDeployAgent::GrokBuild => self.home.join(".grok").join("skills").join(name),
            SkillDeployAgent::ClaudeCode => self.home.join(".claude").join("skills").join(name),
            SkillDeployAgent::Codex => self.home.join(".codex").join("skills").join(name),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn reviewed_from_preview(
    preview: &crate::skills::SkillDeployPreview,
    mode: SkillDeployMode,
) -> ReviewedSkillDeploy {
    ReviewedSkillDeploy {
        agent_id: preview.agent_id,
        skill_id: preview.skill_id.clone(),
        expected_skill_revision: preview.skill_revision,
        config_root: preview.config_root.clone(),
        mode,
        target_state: preview.target_state,
    }
}

#[test]
fn pi_symlink_deploy_creates_owned_link_and_records_revision() {
    let fixture = Fixture::new();
    let record = fixture.import("demo-skill");
    let preview = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("preview");
    assert_eq!(preview.agent_id, SkillDeployAgent::Pi);
    assert_eq!(preview.target_state, SkillDeployTargetState::Absent);
    assert!(preview.can_apply);
    assert!(preview.symlink_available);

    let applied = fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Symlink))
        .expect("apply");
    assert_eq!(applied.agent_id, SkillDeployAgent::Pi);
    assert_eq!(applied.mode, SkillDeployMode::Symlink);

    let target = fixture.target(SkillDeployAgent::Pi, "demo-skill");
    assert!(
        fs::symlink_metadata(&target)
            .expect("meta")
            .file_type()
            .is_symlink()
    );
    let listed = list_agent_skill_deployments(
        &fixture.library,
        &fixture.storage,
        &fixture.environment(),
        SkillDeployAgent::Pi,
    )
    .expect("list");
    assert_eq!(listed.len(), 1);
    assert!(listed[0].in_sync);
}

#[test]
fn grok_copy_deploy_writes_independent_tree() {
    let fixture = Fixture::new();
    let record = fixture.import("copy-skill");
    let preview = fixture
        .preview(SkillDeployAgent::GrokBuild, &record.id)
        .expect("preview");
    assert_eq!(preview.agent_id, SkillDeployAgent::GrokBuild);
    let applied = fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
        .expect("apply");
    assert_eq!(applied.mode, SkillDeployMode::Copy);

    let target = fixture.target(SkillDeployAgent::GrokBuild, "copy-skill");
    let meta = fs::symlink_metadata(&target).expect("meta");
    assert!(meta.is_dir());
    assert!(!meta.file_type().is_symlink());
    assert!(target.join(MANIFEST_FILE_NAME).is_file());
}

#[test]
fn claude_code_deploy_uses_its_config_root_and_records_ownership() {
    let fixture = Fixture::new();
    let record = fixture.import("claude-skill");
    let preview = fixture
        .preview(SkillDeployAgent::ClaudeCode, &record.id)
        .expect("preview");
    assert_eq!(preview.agent_id, SkillDeployAgent::ClaudeCode);
    assert_eq!(
        Path::new(&preview.config_root),
        fixture.home.join(".claude")
    );
    assert_eq!(
        Path::new(&preview.target_path),
        fixture.target(SkillDeployAgent::ClaudeCode, "claude-skill")
    );
    assert!(preview.symlink_available);

    let applied = fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Symlink))
        .expect("apply");
    assert_eq!(applied.agent_id, SkillDeployAgent::ClaudeCode);
    let target = fixture.target(SkillDeployAgent::ClaudeCode, "claude-skill");
    assert!(
        fs::symlink_metadata(&target)
            .expect("meta")
            .file_type()
            .is_symlink()
    );

    let listed = list_agent_skill_deployments(
        &fixture.library,
        &fixture.storage,
        &fixture.environment(),
        SkillDeployAgent::ClaudeCode,
    )
    .expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].agent_id, "claudeCode");
    assert!(listed[0].in_sync);
}

#[test]
fn claude_code_honors_a_config_directory_override() {
    let fixture = Fixture::new();
    let custom_root = fixture.root.join("custom-claude");
    fs::create_dir_all(&custom_root).expect("custom root");
    let environment = DiscoveryEnvironment {
        claude_directory: Some(custom_root.clone().into_os_string()),
        ..fixture.environment()
    };
    let record = fixture.import("custom-root");
    let preview = preview_skill_deploy(
        &fixture.library,
        &fixture.storage,
        &environment,
        SkillDeployAgent::ClaudeCode,
        &record.id,
    )
    .expect("preview");
    assert_eq!(Path::new(&preview.config_root), custom_root);

    let applied = apply_skill_deploy(
        &fixture.library,
        &fixture.storage,
        &environment,
        reviewed_from_preview(&preview, SkillDeployMode::Copy),
        NOW,
    )
    .expect("apply");
    assert_eq!(applied.mode, SkillDeployMode::Copy);
    assert!(
        custom_root
            .join("skills/custom-root")
            .join(MANIFEST_FILE_NAME)
            .is_file()
    );
}

#[test]
fn claude_code_rejects_a_relative_config_directory() {
    let fixture = Fixture::new();
    let environment = DiscoveryEnvironment {
        claude_directory: Some("relative".into()),
        ..fixture.environment()
    };
    let record = fixture.import("relative-root");
    let error = preview_skill_deploy(
        &fixture.library,
        &fixture.storage,
        &environment,
        SkillDeployAgent::ClaudeCode,
        &record.id,
    )
    .expect_err("relative root");
    assert_eq!(error, SkillError::AgentLocationUnavailable);
}

#[test]
fn claude_code_and_pi_deployments_are_independent() {
    let fixture = Fixture::new();
    let record = fixture.import("two-agents");
    let pi = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("pi preview");
    fixture
        .apply(reviewed_from_preview(&pi, SkillDeployMode::Symlink))
        .expect("pi apply");
    let claude = fixture
        .preview(SkillDeployAgent::ClaudeCode, &record.id)
        .expect("claude preview");
    fixture
        .apply(reviewed_from_preview(&claude, SkillDeployMode::Symlink))
        .expect("claude apply");

    fixture
        .disable(SkillDeployAgent::Pi, &record.id, &pi.config_root)
        .expect("disable pi");
    assert!(!fixture.target(SkillDeployAgent::Pi, "two-agents").exists());
    assert!(
        fixture
            .target(SkillDeployAgent::ClaudeCode, "two-agents")
            .exists()
    );
}

#[test]
fn refresh_after_content_replace_updates_symlink_metadata_and_rewrites_copy() {
    let fixture = Fixture::new();
    let record = fixture.import("refresh-skill");
    let pi = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("pi");
    fixture
        .apply(reviewed_from_preview(&pi, SkillDeployMode::Symlink))
        .expect("pi symlink");
    let grok = fixture
        .preview(SkillDeployAgent::GrokBuild, &record.id)
        .expect("grok");
    fixture
        .apply(reviewed_from_preview(&grok, SkillDeployMode::Copy))
        .expect("grok copy");

    let candidate = fixture.root.join("candidate");
    fs::create_dir_all(&candidate).expect("candidate");
    fs::write(
        candidate.join(MANIFEST_FILE_NAME),
        "---\nname: refresh-skill\ndescription: Updated.\n---\n\n# refreshed\n",
    )
    .expect("manifest");
    let updated = service::replace_managed_content(
        &fixture.library,
        &fixture.storage,
        &service::ManagedReplace {
            id: &record.id,
            expected_revision: record.revision,
            candidate: &candidate,
            source_revision: None,
            installer_command: None,
            now_ms: NOW + 1,
        },
    )
    .expect("replace");
    assert_ne!(updated.content_digest, record.content_digest);

    let results = refresh_skill_deployments_after_update(
        &fixture.library,
        &fixture.storage,
        &updated,
        NOW + 2,
    )
    .expect("refresh");
    assert_eq!(results.len(), 2);
    let symlink = results
        .iter()
        .find(|row| row.agent_id == SkillDeployAgent::Pi)
        .expect("pi row");
    assert_eq!(symlink.outcome, SkillUpdateDeployOutcome::SymlinkUpdated);
    let copy = results
        .iter()
        .find(|row| row.agent_id == SkillDeployAgent::GrokBuild)
        .expect("grok row");
    assert_eq!(copy.outcome, SkillUpdateDeployOutcome::Refreshed);

    let copy_body = fs::read_to_string(
        fixture
            .target(SkillDeployAgent::GrokBuild, "refresh-skill")
            .join(MANIFEST_FILE_NAME),
    )
    .expect("copy manifest");
    assert!(copy_body.contains("# refreshed"));
}

#[test]
fn same_skill_can_deploy_to_pi_and_grok_independently() {
    let fixture = Fixture::new();
    let record = fixture.import("shared");
    let pi = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("pi preview");
    fixture
        .apply(reviewed_from_preview(&pi, SkillDeployMode::Symlink))
        .expect("pi apply");
    let grok = fixture
        .preview(SkillDeployAgent::GrokBuild, &record.id)
        .expect("grok preview");
    fixture
        .apply(reviewed_from_preview(&grok, SkillDeployMode::Symlink))
        .expect("grok apply");

    assert!(fixture.target(SkillDeployAgent::Pi, "shared").exists());
    assert!(
        fixture
            .target(SkillDeployAgent::GrokBuild, "shared")
            .exists()
    );

    fixture
        .disable(SkillDeployAgent::Pi, &record.id, &pi.config_root)
        .expect("disable pi");
    assert!(!fixture.target(SkillDeployAgent::Pi, "shared").exists());
    assert!(
        fixture
            .target(SkillDeployAgent::GrokBuild, "shared")
            .exists()
    );
}

#[test]
fn foreign_target_is_previewed_but_not_applied() {
    let fixture = Fixture::new();
    let record = fixture.import("taken");
    let target = fixture.target(SkillDeployAgent::GrokBuild, "taken");
    fs::create_dir_all(&target).expect("foreign");
    fs::write(target.join(MANIFEST_FILE_NAME), "user content").expect("file");

    let preview = fixture
        .preview(SkillDeployAgent::GrokBuild, &record.id)
        .expect("preview");
    assert_eq!(preview.target_state, SkillDeployTargetState::Foreign);
    assert!(!preview.can_apply);
    let error = fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Symlink))
        .expect_err("foreign refused");
    assert_eq!(error, SkillError::DeployModeInvalid);
    assert!(Path::new(&target.join(MANIFEST_FILE_NAME)).exists());
}

#[test]
fn disable_removes_owned_symlink_only() {
    let fixture = Fixture::new();
    let record = fixture.import("revoke-me");
    let preview = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Symlink))
        .expect("apply");

    let neighbour = fixture.target(SkillDeployAgent::Pi, "user-own");
    fs::create_dir_all(&neighbour).expect("neighbour");
    fs::write(neighbour.join("keep.txt"), "keep").expect("keep");

    let disabled = fixture
        .disable(SkillDeployAgent::Pi, &record.id, &preview.config_root)
        .expect("disable");
    assert!(disabled.target_removed);
    assert!(!fixture.target(SkillDeployAgent::Pi, "revoke-me").exists());
    assert!(neighbour.join("keep.txt").exists());
}

#[test]
fn delete_skill_refuses_while_deployed_to_either_agent() {
    let fixture = Fixture::new();
    let record = fixture.import("in-use");
    let preview = fixture
        .preview(SkillDeployAgent::GrokBuild, &record.id)
        .expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Symlink))
        .expect("apply");

    let error = delete_skill(
        &fixture.library,
        &fixture.storage,
        DeleteSkillRequest {
            id: record.id.clone(),
            expected_revision: record.revision,
        },
    )
    .expect_err("in use");
    assert_eq!(error, SkillError::SkillInUse);

    fixture
        .disable(
            SkillDeployAgent::GrokBuild,
            &record.id,
            &preview.config_root,
        )
        .expect("disable");
    delete_skill(
        &fixture.library,
        &fixture.storage,
        DeleteSkillRequest {
            id: record.id.clone(),
            expected_revision: record.revision,
        },
    )
    .expect("delete after disable");
}

#[test]
fn stale_target_state_is_rejected() {
    let fixture = Fixture::new();
    let record = fixture.import("stale");
    let preview = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("preview");
    fs::create_dir_all(fixture.target(SkillDeployAgent::Pi, "stale")).expect("foreign");
    let error = fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Symlink))
        .expect_err("stale");
    assert_eq!(error, SkillError::TargetChanged);
}

struct JournalSeed<'a> {
    id: &'a str,
    kind: &'a str,
    skill_id: &'a str,
    agent_id: &'a str,
    config_root: &'a str,
    target_name: &'a str,
    target_path: &'a str,
    mode: &'a str,
    skill_revision: i64,
    content_digest: &'a str,
}

fn insert_journal(fixture: &Fixture, seed: &JournalSeed<'_>) {
    fixture
        .storage
        .lock()
        .expect("lock")
        .execute(
            "INSERT INTO skill_deploy_operation(
                id, kind, state, skill_id, agent_id, config_root, target_name,
                target_path, mode, skill_revision, content_digest, created_at, updated_at
             ) VALUES(?1,?2,'prepared',?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
            rusqlite::params![
                seed.id,
                seed.kind,
                seed.skill_id,
                seed.agent_id,
                seed.config_root,
                seed.target_name,
                seed.target_path,
                seed.mode,
                seed.skill_revision,
                seed.content_digest,
                NOW,
            ],
        )
        .expect("insert journal");
}

#[test]
fn successful_apply_and_disable_leave_no_journal_row() {
    let fixture = Fixture::new();
    let record = fixture.import("clean-journal");
    let preview = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Symlink))
        .expect("apply");
    assert!(
        list_skill_deploy_recoveries(&fixture.library, &fixture.storage)
            .expect("list")
            .is_empty()
    );
    fixture
        .disable(SkillDeployAgent::Pi, &record.id, &preview.config_root)
        .expect("disable");
    assert!(
        list_skill_deploy_recoveries(&fixture.library, &fixture.storage)
            .expect("list")
            .is_empty()
    );
}

#[test]
fn crash_after_apply_write_without_ownership_needs_record_and_finish_records_it() {
    let fixture = Fixture::new();
    let record = fixture.import("needs-record");
    let preview = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("preview");
    let target = fixture.target(SkillDeployAgent::Pi, &record.name);
    fs::create_dir_all(target.parent().expect("parent")).expect("skills dir");
    #[cfg(unix)]
    std::os::unix::fs::symlink(fixture.library.root().join(&record.id), &target).expect("symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(fixture.library.root().join(&record.id), &target)
        .expect("symlink");
    let op_id = "a".repeat(32);
    insert_journal(
        &fixture,
        &JournalSeed {
            id: &op_id,
            kind: "apply",
            skill_id: &record.id,
            agent_id: "pi",
            config_root: &preview.config_root,
            target_name: &record.name,
            target_path: &preview.target_path,
            mode: "symlink",
            skill_revision: record.revision,
            content_digest: &record.content_digest,
        },
    );

    let listed = list_skill_deploy_recoveries(&fixture.library, &fixture.storage).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].kind, SkillDeployOperationKind::Apply);
    assert_eq!(
        listed[0].classification,
        SkillDeployRecoveryClassification::NeedsRecord
    );
    assert!(listed[0].can_finish);
    assert!(!listed[0].can_dismiss);

    finish_skill_deploy_recovery(
        &fixture.library,
        &fixture.storage,
        SkillDeployRecoveryAction {
            id: op_id.clone(),
            expected_classification: SkillDeployRecoveryClassification::NeedsRecord,
        },
    )
    .expect("finish");
    assert!(
        list_skill_deploy_recoveries(&fixture.library, &fixture.storage)
            .expect("list")
            .is_empty()
    );
    let deployments = list_agent_skill_deployments(
        &fixture.library,
        &fixture.storage,
        &fixture.environment(),
        SkillDeployAgent::Pi,
    )
    .expect("deployments");
    assert_eq!(deployments.len(), 1);
    assert_eq!(deployments[0].skill_id, record.id);
    assert!(deployments[0].in_sync);

    let error = finish_skill_deploy_recovery(
        &fixture.library,
        &fixture.storage,
        SkillDeployRecoveryAction {
            id: op_id,
            expected_classification: SkillDeployRecoveryClassification::NeedsRecord,
        },
    )
    .expect_err("second finish");
    assert_eq!(error, SkillError::NotFound);
}

#[test]
fn crash_after_disable_removal_with_stale_ownership_needs_record_clear() {
    let fixture = Fixture::new();
    let record = fixture.import("needs-clear");
    let preview = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Symlink))
        .expect("apply");
    let target = fixture.target(SkillDeployAgent::Pi, &record.name);
    // Windows DeleteFileW refuses the directory link apply created.
    #[cfg(windows)]
    fs::remove_dir(&target).expect("remove target");
    #[cfg(not(windows))]
    fs::remove_file(&target).expect("remove target");
    let op_id = "b".repeat(32);
    insert_journal(
        &fixture,
        &JournalSeed {
            id: &op_id,
            kind: "disable",
            skill_id: &record.id,
            agent_id: "pi",
            config_root: &preview.config_root,
            target_name: &record.name,
            target_path: &preview.target_path,
            mode: "symlink",
            skill_revision: record.revision,
            content_digest: &record.content_digest,
        },
    );

    let listed = list_skill_deploy_recoveries(&fixture.library, &fixture.storage).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].classification,
        SkillDeployRecoveryClassification::NeedsRecordClear
    );

    finish_skill_deploy_recovery(
        &fixture.library,
        &fixture.storage,
        SkillDeployRecoveryAction {
            id: op_id,
            expected_classification: SkillDeployRecoveryClassification::NeedsRecordClear,
        },
    )
    .expect("finish");
    assert!(
        list_agent_skill_deployments(
            &fixture.library,
            &fixture.storage,
            &fixture.environment(),
            SkillDeployAgent::Pi,
        )
        .expect("deployments")
        .is_empty()
    );
    assert!(
        list_skill_deploy_recoveries(&fixture.library, &fixture.storage)
            .expect("list")
            .is_empty()
    );
}

#[test]
fn foreign_target_conflict_refuses_finish_and_allows_dismiss() {
    let fixture = Fixture::new();
    let record = fixture.import("conflict");
    let preview = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("preview");
    let target = fixture.target(SkillDeployAgent::Pi, &record.name);
    fs::create_dir_all(&target).expect("foreign");
    fs::write(target.join("foreign.txt"), "nope").expect("file");
    let op_id = "c".repeat(32);
    insert_journal(
        &fixture,
        &JournalSeed {
            id: &op_id,
            kind: "apply",
            skill_id: &record.id,
            agent_id: "pi",
            config_root: &preview.config_root,
            target_name: &record.name,
            target_path: &preview.target_path,
            mode: "symlink",
            skill_revision: record.revision,
            content_digest: &record.content_digest,
        },
    );

    let listed = list_skill_deploy_recoveries(&fixture.library, &fixture.storage).expect("list");
    assert_eq!(
        listed[0].classification,
        SkillDeployRecoveryClassification::Conflict
    );
    assert!(!listed[0].can_finish);
    assert!(listed[0].can_dismiss);

    let error = finish_skill_deploy_recovery(
        &fixture.library,
        &fixture.storage,
        SkillDeployRecoveryAction {
            id: op_id.clone(),
            expected_classification: SkillDeployRecoveryClassification::Conflict,
        },
    )
    .expect_err("finish conflict");
    assert_eq!(error, SkillError::TargetConflict);
    assert!(target.join("foreign.txt").exists());

    dismiss_skill_deploy_recovery(
        &fixture.library,
        &fixture.storage,
        SkillDeployRecoveryAction {
            id: op_id,
            expected_classification: SkillDeployRecoveryClassification::Conflict,
        },
    )
    .expect("dismiss");
    assert!(target.join("foreign.txt").exists());
    assert!(
        list_skill_deploy_recoveries(&fixture.library, &fixture.storage)
            .expect("list")
            .is_empty()
    );
}

#[test]
fn dismiss_refuses_needs_record_inconsistency() {
    let fixture = Fixture::new();
    let record = fixture.import("no-dismiss");
    let preview = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("preview");
    let target = fixture.target(SkillDeployAgent::Pi, &record.name);
    fs::create_dir_all(target.parent().expect("parent")).expect("skills dir");
    #[cfg(unix)]
    std::os::unix::fs::symlink(fixture.library.root().join(&record.id), &target).expect("symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(fixture.library.root().join(&record.id), &target)
        .expect("symlink");
    let op_id = "d".repeat(32);
    insert_journal(
        &fixture,
        &JournalSeed {
            id: &op_id,
            kind: "apply",
            skill_id: &record.id,
            agent_id: "pi",
            config_root: &preview.config_root,
            target_name: &record.name,
            target_path: &preview.target_path,
            mode: "symlink",
            skill_revision: record.revision,
            content_digest: &record.content_digest,
        },
    );

    let error = dismiss_skill_deploy_recovery(
        &fixture.library,
        &fixture.storage,
        SkillDeployRecoveryAction {
            id: op_id,
            expected_classification: SkillDeployRecoveryClassification::NeedsRecord,
        },
    )
    .expect_err("dismiss");
    assert_eq!(error, SkillError::RecoveryConflict);
}

#[test]
fn delete_skill_refuses_while_journal_names_the_skill() {
    let fixture = Fixture::new();
    let record = fixture.import("journal-block");
    let preview = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("preview");
    let op_id = "e".repeat(32);
    insert_journal(
        &fixture,
        &JournalSeed {
            id: &op_id,
            kind: "apply",
            skill_id: &record.id,
            agent_id: "pi",
            config_root: &preview.config_root,
            target_name: &record.name,
            target_path: &preview.target_path,
            mode: "symlink",
            skill_revision: record.revision,
            content_digest: &record.content_digest,
        },
    );

    let error = delete_skill(
        &fixture.library,
        &fixture.storage,
        DeleteSkillRequest {
            id: record.id.clone(),
            expected_revision: record.revision,
        },
    )
    .expect_err("in use by journal");
    assert_eq!(error, SkillError::SkillInUse);

    dismiss_skill_deploy_recovery(
        &fixture.library,
        &fixture.storage,
        SkillDeployRecoveryAction {
            id: op_id,
            expected_classification: SkillDeployRecoveryClassification::NotStarted,
        },
    )
    .expect("dismiss empty");
    delete_skill(
        &fixture.library,
        &fixture.storage,
        DeleteSkillRequest {
            id: record.id.clone(),
            expected_revision: record.revision,
        },
    )
    .expect("delete after dismiss");
}

#[test]
fn changed_classification_refuses_finish() {
    let fixture = Fixture::new();
    let record = fixture.import("stale-class");
    let preview = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("preview");
    let op_id = "f".repeat(32);
    insert_journal(
        &fixture,
        &JournalSeed {
            id: &op_id,
            kind: "apply",
            skill_id: &record.id,
            agent_id: "pi",
            config_root: &preview.config_root,
            target_name: &record.name,
            target_path: &preview.target_path,
            mode: "symlink",
            skill_revision: record.revision,
            content_digest: &record.content_digest,
        },
    );
    let error = finish_skill_deploy_recovery(
        &fixture.library,
        &fixture.storage,
        SkillDeployRecoveryAction {
            id: op_id,
            expected_classification: SkillDeployRecoveryClassification::Completed,
        },
    )
    .expect_err("stale classification");
    assert_eq!(error, SkillError::TargetChanged);
}

#[test]
fn codex_deploys_user_level_and_reports_shared_duplicates() {
    let fixture = Fixture::new();
    let record = fixture.import("codex-skill");
    let preview = fixture
        .preview(SkillDeployAgent::Codex, &record.id)
        .expect("preview");
    assert_eq!(Path::new(&preview.config_root), fixture.home.join(".codex"));
    assert_eq!(preview.shared_duplicate_path, None);
    let json = serde_json::to_value(&preview).expect("json");
    assert!(json.get("sharedDuplicatePath").is_none());
    assert_eq!(json["agentId"], "codex");

    let shared = fixture
        .home
        .join(".agents")
        .join("skills")
        .join("codex-skill");
    fs::create_dir_all(&shared).expect("shared skill");
    let preview = fixture
        .preview(SkillDeployAgent::Codex, &record.id)
        .expect("preview");
    assert_eq!(
        preview.shared_duplicate_path.as_deref().map(Path::new),
        Some(shared.as_path())
    );
    // Other Agents never report the Codex-only shared directory.
    let claude = fixture
        .preview(SkillDeployAgent::ClaudeCode, &record.id)
        .expect("preview");
    assert_eq!(claude.shared_duplicate_path, None);

    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
        .expect("apply");
    let target = fixture.target(SkillDeployAgent::Codex, "codex-skill");
    assert!(target.join(MANIFEST_FILE_NAME).is_file());
    let listed = list_agent_skill_deployments(
        &fixture.library,
        &fixture.storage,
        &fixture.environment(),
        SkillDeployAgent::Codex,
    )
    .expect("list");
    assert_eq!(listed[0].agent_id, "codex");
    fixture
        .disable(SkillDeployAgent::Codex, &record.id, &preview.config_root)
        .expect("disable");
    assert!(!target.exists());
}

const ALL_AGENTS: [SkillDeployAgent; 4] = [
    SkillDeployAgent::Pi,
    SkillDeployAgent::GrokBuild,
    SkillDeployAgent::ClaudeCode,
    SkillDeployAgent::Codex,
];

impl Fixture {
    fn resync(
        &self,
        preview: &crate::skills::SkillDeployPreview,
        mode: SkillDeployMode,
    ) -> Result<crate::skills::SkillResynced, SkillError> {
        super::resync_skill_deploy(
            &self.library,
            &self.storage,
            &self.environment(),
            resync_request(preview, mode),
            NOW + 1,
        )
    }

    fn targets(
        &self,
        agent: SkillDeployAgent,
    ) -> Result<Vec<crate::skills::AgentSkillTarget>, SkillError> {
        super::preview_agent_skill_targets(&self.library, &self.storage, &self.environment(), agent)
    }

    fn backups(&self) -> PathBuf {
        self.root.join("skill-deploy-backups")
    }

    /// Backup folders for one (Agent, skill), oldest first.
    fn skill_backups(&self, agent: SkillDeployAgent, skill_id: &str) -> Vec<PathBuf> {
        let dir = self.backups().join(agent.as_str()).join(skill_id);
        let mut out: Vec<_> = fs::read_dir(dir)
            .map(|entries| entries.map(|entry| entry.expect("entry").path()).collect())
            .unwrap_or_default();
        out.sort();
        out
    }

    fn in_sync(&self, agent: SkillDeployAgent) -> Vec<bool> {
        list_agent_skill_deployments(&self.library, &self.storage, &self.environment(), agent)
            .expect("list")
            .into_iter()
            .map(|row| row.in_sync)
            .collect()
    }
}

fn resync_request(
    preview: &crate::skills::SkillDeployPreview,
    mode: SkillDeployMode,
) -> crate::skills::ResyncSkillDeployRequest {
    crate::skills::ResyncSkillDeployRequest {
        agent_id: preview.agent_id,
        skill_id: preview.skill_id.clone(),
        config_root: preview.config_root.clone(),
        expected_skill_revision: preview.skill_revision,
        reviewed_target_path: preview.target_path.clone(),
        reviewed_target_state: preview.target_state,
        mode,
    }
}

fn skills_dir(fixture: &Fixture, agent: SkillDeployAgent) -> PathBuf {
    fixture
        .target(agent, "x")
        .parent()
        .expect("skills dir")
        .to_path_buf()
}

#[test]
fn preview_is_read_only_for_every_agent() {
    let fixture = Fixture::new();
    let record = fixture.import("quiet-skill");
    // Codex root itself is missing too: preview must not create it.
    fs::remove_dir_all(fixture.home.join(".codex")).expect("drop codex root");
    for agent in ALL_AGENTS {
        let preview = fixture.preview(agent, &record.id).expect("preview");
        assert_eq!(preview.target_state, SkillDeployTargetState::Absent);
        assert!(preview.can_apply);
        assert!(
            !skills_dir(&fixture, agent).exists(),
            "{agent:?} preview created its skills dir"
        );
        let targets = fixture.targets(agent).expect("targets");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].target_state, AgentSkillTargetState::Absent);
        assert!(!skills_dir(&fixture, agent).exists());
    }
    assert!(!fixture.home.join(".codex").exists());
    // The probe never leaves anything below the Agent roots.
    assert!(
        !skills_dir(&fixture, SkillDeployAgent::Pi)
            .join(".vibemate-symlink-probe")
            .exists()
    );
}

#[test]
fn apply_creates_the_skills_dir_only_on_write() {
    let fixture = Fixture::new();
    let record = fixture.import("late-dir");
    let preview = fixture
        .preview(SkillDeployAgent::Codex, &record.id)
        .expect("preview");
    assert!(!skills_dir(&fixture, SkillDeployAgent::Codex).exists());
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
        .expect("apply");
    assert!(
        fixture
            .target(SkillDeployAgent::Codex, "late-dir")
            .join(MANIFEST_FILE_NAME)
            .is_file()
    );
}

#[test]
fn resync_overwrites_an_edited_copy_and_keeps_a_backup() {
    for agent in ALL_AGENTS {
        let fixture = Fixture::new();
        let record = fixture.import("edited-copy");
        let preview = fixture.preview(agent, &record.id).expect("preview");
        fixture
            .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
            .expect("apply");
        let target = fixture.target(agent, "edited-copy");
        fs::write(target.join(MANIFEST_FILE_NAME), "hand edit").expect("edit");
        fs::write(target.join("notes.md"), "extra").expect("extra");
        assert_eq!(fixture.in_sync(agent), vec![false]);

        let drifted = fixture.preview(agent, &record.id).expect("preview");
        assert_eq!(drifted.target_state, SkillDeployTargetState::Foreign);
        assert!(drifted.overwrites_user_changes);
        let backup_dir = fixture.backups().join(agent.as_str()).join(&record.id);
        assert_eq!(
            drifted.backup_dir.as_deref().map(Path::new),
            Some(backup_dir.as_path())
        );
        assert!(!backup_dir.exists(), "preview creates nothing");
        let resynced = fixture
            .resync(&drifted, SkillDeployMode::Copy)
            .expect("resync");
        assert_eq!(resynced.deployed.agent_id, agent);
        assert_eq!(resynced.deployed.mode, SkillDeployMode::Copy);
        assert_eq!(resynced.deployed.target_path, drifted.target_path);
        assert!(resynced.overwrote_user_changes);
        assert_eq!(fixture.in_sync(agent), vec![true]);
        let manifest = fs::read_to_string(target.join(MANIFEST_FILE_NAME)).expect("manifest");
        assert!(manifest.contains("description: A test skill."));
        assert!(!target.join("notes.md").exists());

        // Exactly one backup holds the edited content, and the answer names it.
        let operations = fixture.skill_backups(agent, &record.id);
        assert_eq!(operations.len(), 1);
        assert_eq!(
            resynced.backup_path.as_deref().map(Path::new),
            Some(operations[0].as_path())
        );
        let saved = operations[0].join("edited-copy");
        assert_eq!(
            fs::read_to_string(saved.join(MANIFEST_FILE_NAME)).expect("saved"),
            "hand edit"
        );
        assert_eq!(
            fs::read_to_string(saved.join("notes.md")).expect("saved"),
            "extra"
        );
        // No aside, staging entry or journal row is left behind.
        let leftovers: Vec<_> = fs::read_dir(target.parent().expect("parent"))
            .expect("skills dir")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("edited-copy")]);
        let root = target.parent().and_then(Path::parent).expect("agent root");
        assert!(!root.join(".vibemate-staging").exists());
        assert!(
            list_skill_deploy_recoveries(&fixture.library, &fixture.storage)
                .expect("recoveries")
                .is_empty()
        );
    }
}

#[cfg(unix)]
#[test]
fn resync_repoints_a_retargeted_link_without_touching_its_destination() {
    let fixture = Fixture::new();
    let record = fixture.import("moved-link");
    let preview = fixture
        .preview(SkillDeployAgent::ClaudeCode, &record.id)
        .expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Symlink))
        .expect("apply");
    let target = fixture.target(SkillDeployAgent::ClaudeCode, "moved-link");
    let elsewhere = fixture.root.join("elsewhere");
    fs::create_dir_all(&elsewhere).expect("elsewhere");
    fs::write(elsewhere.join("keep.txt"), "keep").expect("keep");
    fs::remove_file(&target).expect("unlink");
    std::os::unix::fs::symlink(&elsewhere, &target).expect("relink");

    let drifted = fixture
        .preview(SkillDeployAgent::ClaudeCode, &record.id)
        .expect("preview");
    assert_eq!(drifted.target_state, SkillDeployTargetState::Foreign);
    fixture
        .resync(&drifted, SkillDeployMode::Symlink)
        .expect("resync");
    assert_eq!(fixture.in_sync(SkillDeployAgent::ClaudeCode), vec![true]);
    assert_eq!(
        fs::read_to_string(elsewhere.join("keep.txt")).expect("kept"),
        "keep"
    );
    let operation = fixture
        .skill_backups(SkillDeployAgent::ClaudeCode, &record.id)
        .pop()
        .expect("one backup");
    let note =
        fs::read_to_string(operation.join("moved-link").join("link-target.txt")).expect("note");
    assert_eq!(note, elsewhere.to_string_lossy());
}

#[test]
fn resync_recreates_a_deleted_target_and_can_switch_mode() {
    let fixture = Fixture::new();
    let record = fixture.import("gone-skill");
    let preview = fixture
        .preview(SkillDeployAgent::Codex, &record.id)
        .expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Symlink))
        .expect("apply");
    fs::remove_dir_all(skills_dir(&fixture, SkillDeployAgent::Codex)).expect("drop skills dir");
    let absent = fixture
        .preview(SkillDeployAgent::Codex, &record.id)
        .expect("preview");
    assert_eq!(absent.target_state, SkillDeployTargetState::Absent);
    assert!(!skills_dir(&fixture, SkillDeployAgent::Codex).exists());
    assert!(!absent.overwrites_user_changes);
    assert!(absent.backup_dir.is_none());
    let resynced = fixture
        .resync(&absent, SkillDeployMode::Copy)
        .expect("resync");
    assert_eq!(resynced.deployed.mode, SkillDeployMode::Copy);
    assert!(!resynced.overwrote_user_changes);
    assert!(resynced.backup_path.is_none());
    assert_eq!(fixture.in_sync(SkillDeployAgent::Codex), vec![true]);
    assert!(!fixture.backups().exists(), "absent targets need no backup");
}

#[test]
fn resync_refuses_targets_without_ownership_or_with_stale_reviews() {
    let fixture = Fixture::new();
    let record = fixture.import("guarded");
    // A user folder with no ownership row: not_found, untouched.
    let target = fixture.target(SkillDeployAgent::Pi, "guarded");
    fs::create_dir_all(&target).expect("foreign");
    fs::write(target.join("mine.txt"), "mine").expect("mine");
    let foreign = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("preview");
    assert!(!foreign.overwrites_user_changes, "no ownership, no resync");
    assert!(foreign.backup_dir.is_none());
    assert_eq!(
        fixture.resync(&foreign, SkillDeployMode::Copy).unwrap_err(),
        SkillError::NotFound
    );
    assert_eq!(
        fs::read_to_string(target.join("mine.txt")).expect("kept"),
        "mine"
    );
    fs::remove_dir_all(&target).expect("clear");

    let preview = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
        .expect("apply");
    fs::write(target.join("extra.txt"), "edit").expect("edit");
    let drifted = fixture
        .preview(SkillDeployAgent::Pi, &record.id)
        .expect("preview");

    let mut wrong_state = resync_request(&drifted, SkillDeployMode::Copy);
    wrong_state.reviewed_target_state = SkillDeployTargetState::OwnedCopy;
    let mut wrong_path = resync_request(&drifted, SkillDeployMode::Copy);
    wrong_path.reviewed_target_path = fixture
        .target(SkillDeployAgent::Pi, "other")
        .to_string_lossy()
        .into_owned();
    let mut wrong_root = resync_request(&drifted, SkillDeployMode::Copy);
    wrong_root.config_root = fixture.home.to_string_lossy().into_owned();
    let mut stale = resync_request(&drifted, SkillDeployMode::Copy);
    stale.expected_skill_revision += 1;
    let mut invalid = resync_request(&drifted, SkillDeployMode::Copy);
    invalid.expected_skill_revision = 0;
    for (request, code) in [
        (wrong_state, SkillError::TargetChanged),
        (wrong_path, SkillError::TargetChanged),
        (wrong_root, SkillError::TargetChanged),
        (stale, SkillError::RevisionConflict),
        (invalid, SkillError::InvalidRequest),
    ] {
        let error = super::resync_skill_deploy(
            &fixture.library,
            &fixture.storage,
            &fixture.environment(),
            request,
            NOW,
        )
        .unwrap_err();
        assert_eq!(error, code);
    }
    assert_eq!(
        fs::read_to_string(target.join("extra.txt")).expect("kept"),
        "edit"
    );
    assert!(!fixture.backups().exists());
    assert!(
        list_skill_deploy_recoveries(&fixture.library, &fixture.storage)
            .expect("recoveries")
            .is_empty()
    );
}

#[test]
fn resync_refuses_an_occupant_of_a_different_kind() {
    let fixture = Fixture::new();
    let record = fixture.import("kind-swap");
    let preview = fixture
        .preview(SkillDeployAgent::GrokBuild, &record.id)
        .expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
        .expect("apply");
    let target = fixture.target(SkillDeployAgent::GrokBuild, "kind-swap");
    fs::remove_dir_all(&target).expect("drop copy");
    fs::write(&target, "a plain file").expect("file");
    let drifted = fixture
        .preview(SkillDeployAgent::GrokBuild, &record.id)
        .expect("preview");
    assert_eq!(drifted.target_state, SkillDeployTargetState::Foreign);
    assert_eq!(
        fixture.resync(&drifted, SkillDeployMode::Copy).unwrap_err(),
        SkillError::TargetConflict
    );
    assert_eq!(fs::read_to_string(&target).expect("kept"), "a plain file");
}

#[test]
fn agent_skill_targets_report_every_library_skill() {
    let fixture = Fixture::new();
    let owned = fixture.import("owned-one");
    let occupied = fixture.import("occupied-one");
    let free = fixture.import("free-one");
    let preview = fixture
        .preview(SkillDeployAgent::Pi, &owned.id)
        .expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
        .expect("apply");
    fs::create_dir_all(fixture.target(SkillDeployAgent::Pi, "occupied-one")).expect("foreign");

    let targets = fixture.targets(SkillDeployAgent::Pi).expect("targets");
    let state = |id: &str| {
        targets
            .iter()
            .find(|row| row.skill_id == id)
            .map(|row| row.target_state)
    };
    assert_eq!(targets.len(), 3);
    assert_eq!(state(&owned.id), Some(AgentSkillTargetState::OwnedCopy));
    assert_eq!(state(&occupied.id), Some(AgentSkillTargetState::Foreign));
    assert_eq!(state(&free.id), Some(AgentSkillTargetState::Absent));
    assert!(
        targets
            .iter()
            .all(|row| row.shared_duplicate_path.is_none())
    );
    let row = targets
        .iter()
        .find(|row| row.skill_id == free.id)
        .expect("row");
    assert_eq!(
        Path::new(&row.target_path),
        fixture.target(SkillDeployAgent::Pi, "free-one")
    );
    assert!(row.symlink_available);
}

#[test]
fn symlink_probe_dir_is_removed_on_every_exit_without_following_the_link() {
    let fixture = Fixture::new();
    let kept = fixture.root.join("kept");
    fs::create_dir_all(&kept).expect("kept dir");
    fs::write(kept.join("file.txt"), "keep").expect("kept file");

    // Early exit after the link exists: the guard still cleans up.
    let base = fixture.root.join("probe-ok");
    fs::create_dir(&base).expect("probe dir");
    let linked = super::create_dir_symlink(&kept, &base.join(super::PROBE_LINK)).is_ok();
    drop(super::ProbeDir(base.clone()));
    assert!(!base.exists(), "probe dir removed (link created: {linked})");
    assert_eq!(fs::read_to_string(kept.join("file.txt")).unwrap(), "keep");

    // A panic while the guard is alive still removes the directory.
    let base = fixture.root.join("probe-panic");
    fs::create_dir(&base).expect("probe dir");
    let guarded = base.clone();
    let outcome = std::panic::catch_unwind(move || {
        let _guard = super::ProbeDir(guarded);
        panic!("probe failed");
    });
    assert!(outcome.is_err());
    assert!(!base.exists());
}

#[test]
fn resync_keeps_the_newest_five_backups_per_agent_and_skill() {
    let fixture = Fixture::new();
    let record = fixture.import("many-edits");
    let other = fixture.import("other-skill");
    let agent = SkillDeployAgent::Pi;
    for id in [&record.id, &other.id] {
        let preview = fixture.preview(agent, id).expect("preview");
        fixture
            .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
            .expect("apply");
    }
    // Another skill's backup must survive this skill's pruning.
    let other_target = fixture.target(agent, "other-skill");
    fs::write(other_target.join("edit.txt"), "other").expect("edit");
    let drifted = fixture.preview(agent, &other.id).expect("preview");
    fixture
        .resync(&drifted, SkillDeployMode::Copy)
        .expect("resync other");

    let target = fixture.target(agent, "many-edits");
    for round in 0..7_i64 {
        fs::write(target.join("edit.txt"), format!("edit {round}")).expect("edit");
        let drifted = fixture.preview(agent, &record.id).expect("preview");
        super::resync_skill_deploy(
            &fixture.library,
            &fixture.storage,
            &fixture.environment(),
            resync_request(&drifted, SkillDeployMode::Copy),
            NOW + 10 + round,
        )
        .expect("resync");
    }
    let kept = fixture.skill_backups(agent, &record.id);
    assert_eq!(kept.len(), 5);
    let contents: Vec<_> = kept
        .iter()
        .map(|dir| fs::read_to_string(dir.join("many-edits").join("edit.txt")).expect("saved"))
        .collect();
    assert_eq!(contents, ["edit 2", "edit 3", "edit 4", "edit 5", "edit 6"]);
    assert_eq!(fixture.skill_backups(agent, &other.id).len(), 1);
}

#[test]
fn resync_sweeps_only_vibemate_leftovers_without_a_journal_row() {
    let fixture = Fixture::new();
    let record = fixture.import("swept");
    let agent = SkillDeployAgent::Codex;
    let preview = fixture.preview(agent, &record.id).expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
        .expect("apply");
    let root = fixture.home.join(".codex");
    let staging = root.join(super::staging::STAGING_DIR);
    let skills = skills_dir(&fixture, agent);
    let orphan = "a".repeat(32);
    for path in [
        staging.join(format!("stage-{orphan}")),
        staging.join(format!("aside-{orphan}")),
        skills.join(format!(".vibemate-resync-{orphan}")),
    ] {
        fs::create_dir_all(&path).expect("leftover");
        fs::write(path.join("SKILL.md"), "old").expect("leftover file");
    }
    // Names vibemate does not own stay, wherever they are.
    let user_named = [
        staging.join("notes"),
        staging.join("stage-not-an-id"),
        skills.join(".vibemate-resync-mine"),
    ];
    for path in &user_named {
        fs::create_dir_all(path).expect("user entry");
    }

    fs::write(fixture.target(agent, "swept").join("edit.txt"), "edit").expect("edit");
    let drifted = fixture.preview(agent, &record.id).expect("preview");
    fixture
        .resync(&drifted, SkillDeployMode::Copy)
        .expect("resync");
    assert!(!staging.join(format!("stage-{orphan}")).exists());
    assert!(!staging.join(format!("aside-{orphan}")).exists());
    assert!(!skills.join(format!(".vibemate-resync-{orphan}")).exists());
    for path in &user_named {
        assert!(path.exists(), "{} must stay", path.display());
    }
    // The staging folder is not inside `skills/`.
    assert_ne!(staging.parent(), Some(skills.as_path()));
}

#[test]
fn agent_skill_targets_mark_unreadable_rows_and_codex_shared_duplicates() {
    let fixture = Fixture::new();
    let record = fixture.import("dup-skill");
    let shared = fixture
        .home
        .join(".agents")
        .join("skills")
        .join("dup-skill");
    fs::create_dir_all(&shared).expect("shared dup");
    let codex = fixture.targets(SkillDeployAgent::Codex).expect("targets");
    assert_eq!(codex[0].skill_id, record.id);
    assert_eq!(
        codex[0].shared_duplicate_path.as_deref().map(Path::new),
        Some(shared.as_path())
    );
    let json = serde_json::to_value(&codex[0]).expect("json");
    assert!(json.get("sharedDuplicatePath").is_some());
    for agent in [
        SkillDeployAgent::Pi,
        SkillDeployAgent::GrokBuild,
        SkillDeployAgent::ClaudeCode,
    ] {
        let rows = fixture.targets(agent).expect("targets");
        assert!(rows[0].shared_duplicate_path.is_none());
        let json = serde_json::to_value(&rows[0]).expect("json");
        assert!(
            json.get("sharedDuplicatePath").is_none(),
            "omitted, never null"
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // A target whose metadata cannot be read (skills dir not searchable).
        let skills = skills_dir(&fixture, SkillDeployAgent::Pi);
        fs::create_dir_all(skills.join("dup-skill")).expect("target");
        fs::set_permissions(&skills, fs::Permissions::from_mode(0o000)).expect("lock");
        // Root ignores modes; then the check below cannot be made.
        let readable = fs::symlink_metadata(skills.join("dup-skill")).is_ok();
        let rows = fixture.targets(SkillDeployAgent::Pi);
        fs::set_permissions(&skills, fs::Permissions::from_mode(0o755)).expect("unlock");
        let rows = rows.expect("targets");
        if !readable {
            assert_eq!(rows[0].target_state, AgentSkillTargetState::Unreadable);
            let json = serde_json::to_value(&rows[0]).expect("json");
            assert_eq!(json["targetState"], "unreadable");
        }
    }
}

/// Run the startup sweep the way the application does: under the deploy lock.
fn startup_sweep(fixture: &Fixture) {
    let guard = super::lock_skill_deploys().expect("deploy lock");
    super::sweep_skill_deploy_leftovers(&guard, &fixture.storage, &fixture.environment())
        .expect("startup sweep");
}

/// Names directly inside a directory, sorted; empty when it does not exist.
fn names_in(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|entry| {
                    entry
                        .expect("entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Insert a journal row so a leftover named after it counts as in recovery.
fn journal_row(fixture: &Fixture, id: &str, skill_id: &str, root: &Path) {
    let root_text = root.to_str().expect("utf-8 root");
    super::repository::insert_operation(
        &fixture.storage,
        &super::repository::OperationInsert {
            id,
            kind: SkillDeployOperationKind::Apply,
            skill_id,
            agent_id: SkillDeployAgent::Codex.as_str(),
            config_root: root_text,
            target_name: "pending",
            target_path: root_text,
            mode: Some(SkillDeployMode::Copy),
            skill_revision: 1,
            content_digest: &"0".repeat(64),
            now_ms: NOW,
        },
    )
    .expect("journal row");
}

#[test]
fn apply_refresh_and_probe_leave_nothing_but_targets_in_skills() {
    let fixture = Fixture::new();
    let copied = fixture.import("copied");
    let linked = fixture.import("linked");
    let agent = SkillDeployAgent::Codex;
    let preview = fixture.preview(agent, &copied.id).expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
        .expect("copy apply");
    let preview = fixture.preview(agent, &linked.id).expect("preview");
    let symlink = preview.symlink_available;
    if symlink {
        fixture
            .apply(reviewed_from_preview(&preview, SkillDeployMode::Symlink))
            .expect("symlink apply");
    }
    let results = refresh_skill_deployments_after_update(
        &fixture.library,
        &fixture.storage,
        &copied,
        NOW + 2,
    )
    .expect("refresh");
    assert_eq!(results[0].outcome, SkillUpdateDeployOutcome::Refreshed);

    let skills = skills_dir(&fixture, agent);
    let expected: Vec<String> = if symlink {
        vec!["copied".into(), "linked".into()]
    } else {
        vec!["copied".into()]
    };
    assert_eq!(names_in(&skills), expected);
    let root = fixture.home.join(".codex");
    assert!(!root.join(super::staging::STAGING_DIR).exists());
}

#[test]
fn sweeps_remove_only_vibemate_named_deploy_leftovers() {
    let fixture = Fixture::new();
    let record = fixture.import("kept");
    let agent = SkillDeployAgent::Codex;
    let preview = fixture.preview(agent, &record.id).expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
        .expect("apply");
    let root = fixture.home.join(".codex");
    let skills = skills_dir(&fixture, agent);
    let staging = root.join(super::staging::STAGING_DIR);
    let orphan = "b".repeat(32);
    let pending = "c".repeat(32);
    journal_row(&fixture, &pending, &record.id, &root);
    let managed = fixture.library.root().join(&record.id);

    fs::create_dir_all(&staging).expect("staging");
    let removed_before_operation = [
        staging.join(format!("deploy-{orphan}")),
        skills.join(format!(".vibemate-deploy-{orphan}")),
    ];
    for path in &removed_before_operation {
        fs::create_dir_all(path).expect("leftover");
        fs::write(path.join(MANIFEST_FILE_NAME), "partial").expect("leftover file");
    }
    // Probe links point at the managed copy: they must be unlinked, never
    // followed into the library.
    let probe_links = [
        staging.join(format!("probe-{orphan}")),
        skills.join(".vibemate-symlink-probe"),
    ];
    let mut links_made = true;
    for link in &probe_links {
        links_made &= super::create_dir_symlink(&managed, link).is_ok();
    }
    // A fresh refresh leftover may belong to a running refresh: kept by every
    // sweep, startup included, until it is older than the refresh age.
    let refresh = staging.join(format!("refresh-{orphan}"));
    fs::create_dir_all(&refresh).expect("refresh leftover");
    let in_recovery = staging.join(format!("deploy-{pending}"));
    fs::create_dir_all(&in_recovery).expect("journaled leftover");
    let user_named = [
        staging.join("deploy-notes"),
        staging.join(format!("probe-{orphan}x")),
        skills.join(".vibemate-deploy-mine"),
        skills.join(".vibemate-symlink-probe-notes"),
        root.join(".vibemate-deploy-elsewhere"),
    ];
    for path in &user_named {
        fs::create_dir_all(path).expect("user entry");
    }

    // A deploy of another skill sweeps first.
    let other = fixture.import("other");
    let preview = fixture.preview(agent, &other.id).expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
        .expect("apply other");
    for path in &removed_before_operation {
        assert!(!path.exists(), "{} must be swept", path.display());
    }
    if links_made {
        for link in &probe_links {
            assert!(fs::symlink_metadata(link).is_err(), "{}", link.display());
        }
    }
    assert!(managed.join(MANIFEST_FILE_NAME).is_file(), "library intact");
    assert!(refresh.exists(), "a fresh refresh entry stays");
    assert!(in_recovery.exists(), "a journaled entry stays");
    for path in &user_named {
        assert!(path.exists(), "{} must stay", path.display());
    }

    // Startup applies the same age: a fresh refresh entry (perhaps another
    // instance's running refresh) stays, as do the journaled entry and every
    // user name.
    startup_sweep(&fixture);
    assert!(refresh.exists(), "a fresh refresh entry survives startup");
    assert!(in_recovery.exists());
    for path in &user_named {
        assert!(path.exists(), "{} must stay", path.display());
    }
    // Past the age the refresh entry is removed; user names still stay.
    std::thread::sleep(std::time::Duration::from_millis(50));
    super::staging::sweep_with(
        &fixture.storage,
        &skills,
        std::time::Duration::from_millis(10),
    );
    assert!(!refresh.exists(), "a stale refresh entry is swept");
    assert!(in_recovery.exists());
    for path in &user_named {
        assert!(path.exists(), "{} must stay", path.display());
    }
    // Once the operation is gone its entry is swept as well.
    super::repository::delete_operation(&fixture.storage, &pending).expect("drop row");
    startup_sweep(&fixture);
    assert!(!in_recovery.exists());
    assert_eq!(names_in(&skills).len(), 4, "{:?}", names_in(&skills));
}

#[test]
fn startup_sweep_creates_nothing_and_reaches_recorded_roots() {
    let fixture = Fixture::new();
    fs::remove_dir_all(fixture.home.join(".codex")).expect("drop codex root");
    startup_sweep(&fixture);
    assert!(!fixture.home.join(".codex").exists());
    for agent in ALL_AGENTS {
        assert!(!skills_dir(&fixture, agent).exists());
    }

    // A root recorded by a journal row is swept even when no Agent resolves to
    // it any more.
    let record = fixture.import("moved");
    let moved = fixture.root.join("old-codex-home");
    let staging = moved.join(super::staging::STAGING_DIR);
    let orphan = "d".repeat(32);
    fs::create_dir_all(moved.join("skills")).expect("old skills");
    fs::create_dir_all(staging.join(format!("stage-{orphan}"))).expect("leftover");
    journal_row(&fixture, &"e".repeat(32), &record.id, &moved);
    startup_sweep(&fixture);
    assert!(!staging.exists());
}

/// N2: when `skills/` is a link to a folder elsewhere (possibly another
/// volume), staging is created beside the link's destination so every rename
/// stays on one filesystem, and the sweep finds it there.
#[test]
fn staging_follows_a_linked_skills_dir_to_its_destination() {
    let fixture = Fixture::new();
    let record = fixture.import("linked-home");
    let agent = SkillDeployAgent::Codex;
    let elsewhere = fixture.root.join("elsewhere");
    let real_skills = elsewhere.join("codex-skills");
    fs::create_dir_all(&real_skills).expect("real skills");
    let skills = skills_dir(&fixture, agent);
    if super::create_dir_symlink(&real_skills, &skills).is_err() {
        // Directory links are unavailable here (for example Windows without
        // Developer Mode); nothing to check.
        return;
    }
    let real_staging = fs::canonicalize(&elsewhere)
        .expect("canonical")
        .join(super::staging::STAGING_DIR);
    assert_eq!(
        super::staging::location(&skills).as_deref(),
        Some(real_staging.as_path())
    );
    let orphan = "f".repeat(32);
    fs::create_dir_all(real_staging.join(format!("aside-{orphan}"))).expect("leftover");

    let preview = fixture.preview(agent, &record.id).expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
        .expect("apply through the link");
    assert!(!real_staging.exists(), "leftover swept, staging released");
    assert!(
        real_skills
            .join("linked-home")
            .join(MANIFEST_FILE_NAME)
            .is_file()
    );

    fs::write(real_skills.join("linked-home").join("edit.txt"), "edit").expect("edit");
    let drifted = fixture.preview(agent, &record.id).expect("preview");
    assert_eq!(drifted.target_state, SkillDeployTargetState::Foreign);
    let resynced = fixture
        .resync(&drifted, SkillDeployMode::Copy)
        .expect("resync through the link");
    assert!(resynced.overwrote_user_changes);
    assert_eq!(names_in(&real_skills), vec!["linked-home".to_string()]);
    assert!(!real_skills.join("linked-home").join("edit.txt").exists());
    // Nothing was staged in the Agent root or left beside the destination.
    assert!(
        !fixture
            .home
            .join(".codex")
            .join(super::staging::STAGING_DIR)
            .exists()
    );
    assert!(!real_staging.exists());
    assert_eq!(names_in(&elsewhere), vec!["codex-skills".to_string()]);
}

/// N1: backups hold edited user content, so they are private on Unix even
/// when the replaced files were world-readable or executable.
#[cfg(unix)]
#[test]
fn resync_backups_are_owner_only_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| {
        fs::symlink_metadata(path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777
    };
    let fixture = Fixture::new();
    let record = fixture.import("private-copy");
    let agent = SkillDeployAgent::GrokBuild;
    // An earlier build left the backup root with default permissions.
    fs::create_dir_all(fixture.backups()).expect("old backup root");
    fs::set_permissions(fixture.backups(), fs::Permissions::from_mode(0o755)).expect("chmod");
    let preview = fixture.preview(agent, &record.id).expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
        .expect("apply");
    let target = fixture.target(agent, "private-copy");
    fs::create_dir_all(target.join("scripts")).expect("subdir");
    fs::set_permissions(target.join("scripts"), fs::Permissions::from_mode(0o755)).expect("chmod");
    fs::write(target.join("scripts").join("run.sh"), "#!/bin/sh\n").expect("script");
    fs::set_permissions(
        target.join("scripts").join("run.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("chmod");
    fs::set_permissions(
        target.join(MANIFEST_FILE_NAME),
        fs::Permissions::from_mode(0o644),
    )
    .expect("chmod");
    let drifted = fixture.preview(agent, &record.id).expect("preview");
    let resynced = fixture
        .resync(&drifted, SkillDeployMode::Copy)
        .expect("resync");
    let folder = PathBuf::from(resynced.backup_path.expect("backup"));
    let saved = folder.join("private-copy");
    for dir in [
        fixture.backups(),
        fixture.backups().join(agent.as_str()),
        fixture.backups().join(agent.as_str()).join(&record.id),
        folder.clone(),
        saved.clone(),
        saved.join("scripts"),
    ] {
        assert_eq!(mode(&dir), 0o700, "{}", dir.display());
    }
    for file in [
        saved.join(MANIFEST_FILE_NAME),
        saved.join("scripts").join("run.sh"),
    ] {
        assert_eq!(mode(&file), 0o600, "{}", file.display());
    }
    assert_eq!(
        fs::read_to_string(saved.join("scripts").join("run.sh")).expect("saved"),
        "#!/bin/sh\n"
    );
}

/// N1: a replaced link is backed up as a note, also owner-only.
#[cfg(unix)]
#[test]
fn resync_link_note_backup_is_owner_only_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let record = fixture.import("relinked");
    let agent = SkillDeployAgent::ClaudeCode;
    let preview = fixture.preview(agent, &record.id).expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Symlink))
        .expect("apply");
    let target = fixture.target(agent, "relinked");
    let elsewhere = fixture.root.join("elsewhere-skill");
    fs::create_dir_all(&elsewhere).expect("elsewhere");
    fs::remove_file(&target).expect("unlink");
    std::os::unix::fs::symlink(&elsewhere, &target).expect("relink");
    let drifted = fixture.preview(agent, &record.id).expect("preview");
    let resynced = fixture
        .resync(&drifted, SkillDeployMode::Symlink)
        .expect("resync");
    let folder = PathBuf::from(resynced.backup_path.expect("backup"));
    let note = folder.join("relinked").join("link-target.txt");
    let mode = |path: &Path| {
        fs::symlink_metadata(path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(&folder), 0o700);
    assert_eq!(mode(&folder.join("relinked")), 0o700);
    assert_eq!(mode(&note), 0o600);
}

/// Suggestion 4: a copy whose files cannot be read is not a user edit. The
/// single preview reports `read_failed`, the batch row is `unreadable`, the
/// deployment list still loads (not in sync), and resync never backs it up
/// or overwrites it. Removing `SKILL.md` is still an edit (`foreign`).
#[cfg(unix)]
#[test]
fn unreadable_copy_content_is_read_failed_not_foreign() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let record = fixture.import("locked-copy");
    let agent = SkillDeployAgent::Pi;
    let preview = fixture.preview(agent, &record.id).expect("preview");
    fixture
        .apply(reviewed_from_preview(&preview, SkillDeployMode::Copy))
        .expect("apply");
    let target = fixture.target(agent, "locked-copy");
    let manifest = target.join(MANIFEST_FILE_NAME);
    fs::set_permissions(&manifest, fs::Permissions::from_mode(0o000)).expect("lock");
    // Root ignores modes; then the check below cannot be made.
    let unreadable = fs::File::open(&manifest).is_err();
    let single = fixture.preview(agent, &record.id);
    let batch = fixture.targets(agent);
    let listed = fixture.in_sync(agent);
    // Disable must not treat an unreadable copy as foreign (which would drop
    // ownership) nor delete it: it fails before any journal row or removal.
    let disabled = fixture.disable(agent, &record.id, &preview.config_root);
    fs::set_permissions(&manifest, fs::Permissions::from_mode(0o644)).expect("unlock");
    if unreadable {
        assert_eq!(single.err(), Some(SkillError::ReadFailed));
        assert_eq!(
            batch.expect("targets")[0].target_state,
            AgentSkillTargetState::Unreadable
        );
        assert_eq!(listed, vec![false]);
        assert_eq!(disabled.err(), Some(SkillError::ReadFailed));
        assert!(
            super::repository::get_deployment(
                &fixture.storage,
                &record.id,
                agent.as_str(),
                &preview.config_root,
            )
            .expect("ownership")
            .is_some(),
            "the ownership record is kept"
        );
        assert!(manifest.is_file(), "the copy is left in place");
        assert!(
            list_skill_deploy_recoveries(&fixture.library, &fixture.storage)
                .expect("recoveries")
                .is_empty(),
            "no journal row is left behind"
        );
    }

    let scripts = target.join("scripts");
    fs::create_dir_all(&scripts).expect("subdir");
    fs::set_permissions(&scripts, fs::Permissions::from_mode(0o000)).expect("lock dir");
    let unreadable = fs::read_dir(&scripts).is_err();
    let single = fixture.preview(agent, &record.id);
    fs::set_permissions(&scripts, fs::Permissions::from_mode(0o755)).expect("unlock dir");
    if unreadable {
        assert_eq!(single.err(), Some(SkillError::ReadFailed));
    }
    fs::remove_dir(&scripts).expect("drop subdir");

    fs::remove_file(&manifest).expect("delete manifest");
    let edited = fixture.preview(agent, &record.id).expect("preview");
    assert_eq!(edited.target_state, SkillDeployTargetState::Foreign);
}

/// M2: deploy writes in one process are serialized. While another holder has
/// the deploy lock an apply waits, so it cannot sweep or reuse a staging
/// folder that the holder is still writing.
#[test]
fn a_deploy_waits_while_another_holds_the_deploy_lock() {
    let fixture = Fixture::new();
    let record = fixture.import("waits");
    let agent = SkillDeployAgent::Codex;
    let preview = fixture.preview(agent, &record.id).expect("preview");
    let reviewed = reviewed_from_preview(&preview, SkillDeployMode::Copy);
    let target = fixture.target(agent, "waits");
    let guard = super::lock_skill_deploys().expect("deploy lock");
    std::thread::scope(|scope| {
        let applying = scope.spawn(|| fixture.apply(reviewed));
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!applying.is_finished(), "apply must wait for the lock");
        assert!(!target.exists());
        drop(guard);
        applying.join().expect("thread").expect("apply");
    });
    assert!(target.join(MANIFEST_FILE_NAME).is_file());
}

/// M2: the application takes the deploy lock during setup and hands it to the
/// background startup sweep. A deploy command that arrives meanwhile runs only
/// after the sweep has finished, while read-only previews are not held up.
#[test]
fn deploy_commands_run_after_the_background_startup_sweep() {
    let fixture = Fixture::new();
    let record = fixture.import("after-sweep");
    let agent = SkillDeployAgent::Codex;
    let preview = fixture.preview(agent, &record.id).expect("preview");
    let reviewed = reviewed_from_preview(&preview, SkillDeployMode::Copy);
    let order = std::sync::Mutex::new(Vec::new());
    // "Setup": the lock is taken before any command can run.
    let guard = super::lock_skill_deploys().expect("deploy lock");
    std::thread::scope(|scope| {
        let applying = scope.spawn(|| {
            let result = fixture.apply(reviewed);
            order.lock().expect("order").push("apply");
            result
        });
        let sweeping = scope.spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(200));
            super::sweep_skill_deploy_leftovers(&guard, &fixture.storage, &fixture.environment())
                .expect("sweep");
            order.lock().expect("order").push("sweep");
            drop(guard);
        });
        // Read-only work is not gated by the lock.
        assert!(fixture.preview(agent, &record.id).is_ok());
        assert!(fixture.targets(agent).is_ok());
        sweeping.join().expect("sweep thread");
        applying.join().expect("apply thread").expect("apply");
    });
    assert_eq!(*order.lock().expect("order"), vec!["sweep", "apply"]);
}

/// M2: a waiter gives up with `operation_failed` instead of hanging forever
/// when the holder never finishes (for example a sweep stuck on a network
/// volume).
#[test]
fn a_deploy_lock_wait_is_bounded() {
    let guard = super::lock_skill_deploys().expect("deploy lock");
    let waited = std::thread::scope(|scope| {
        scope
            .spawn(|| super::lock::acquire(std::time::Duration::from_millis(50)).map(drop))
            .join()
            .expect("thread")
    });
    assert_eq!(waited, Err(SkillError::OperationFailed));
    drop(guard);
    assert!(super::lock::acquire(std::time::Duration::from_secs(5)).is_ok());
}
