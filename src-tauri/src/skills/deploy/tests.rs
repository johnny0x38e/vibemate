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
use crate::skills::{SkillLibrary, delete_skill};
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
        fs::create_dir_all(home.join(".pi/agent")).expect("pi root");
        fs::create_dir_all(home.join(".grok")).expect("grok root");
        fs::create_dir_all(home.join(".claude")).expect("claude root");
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
            SkillDeployAgent::Pi => self.home.join(".pi/agent/skills").join(name),
            SkillDeployAgent::GrokBuild => self.home.join(".grok/skills").join(name),
            SkillDeployAgent::ClaudeCode => self.home.join(".claude/skills").join(name),
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
        preview.config_root,
        fixture.home.join(".claude").to_string_lossy()
    );
    assert_eq!(
        preview.target_path,
        fixture
            .home
            .join(".claude")
            .join("skills")
            .join("claude-skill")
            .to_string_lossy()
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
    assert_eq!(preview.config_root, custom_root.to_string_lossy());

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
