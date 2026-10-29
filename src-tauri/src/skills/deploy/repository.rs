//! SQL for skill deployment ownership rows and the unfinished-operation journal.
//! Filesystem work stays in `deploy`.

use super::super::repository::lock;
use super::super::types::{
    SkillDeployMode, SkillDeployOperationKind, SkillDeployOperationState, SkillError,
};
use crate::storage::Storage;
use rusqlite::{OptionalExtension, params};

/// One stored deployment before filesystem classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StoredDeployment {
    pub(super) skill_id: String,
    pub(super) agent_id: String,
    pub(super) config_root: String,
    pub(super) target_name: String,
    pub(super) mode: SkillDeployMode,
    pub(super) skill_revision: i64,
    pub(super) content_digest: String,
    pub(super) deployed_at: i64,
}

/// Values written when a deploy succeeds.
pub(super) struct DeploymentUpsert<'a> {
    pub(super) skill_id: &'a str,
    pub(super) agent_id: &'a str,
    pub(super) config_root: &'a str,
    pub(super) target_name: &'a str,
    pub(super) mode: SkillDeployMode,
    pub(super) skill_revision: i64,
    pub(super) content_digest: &'a str,
    pub(super) deployed_at: i64,
}

fn mode_value(mode: SkillDeployMode) -> &'static str {
    match mode {
        SkillDeployMode::Symlink => "symlink",
        SkillDeployMode::Copy => "copy",
    }
}

fn mode_from_stored(value: &str) -> Result<SkillDeployMode, SkillError> {
    match value {
        "symlink" => Ok(SkillDeployMode::Symlink),
        "copy" => Ok(SkillDeployMode::Copy),
        _ => Err(SkillError::InvalidStoredSkill),
    }
}

fn map_row(row: &rusqlite::Row<'_>) -> Result<StoredDeployment, rusqlite::Error> {
    let mode: String = row.get(4)?;
    Ok(StoredDeployment {
        skill_id: row.get(0)?,
        agent_id: row.get(1)?,
        config_root: row.get(2)?,
        target_name: row.get(3)?,
        mode: mode_from_stored(&mode).map_err(|_| rusqlite::Error::InvalidQuery)?,
        skill_revision: row.get(5)?,
        content_digest: row.get(6)?,
        deployed_at: row.get(7)?,
    })
}

const SELECT_COLS: &str = "skill_id, agent_id, config_root, target_name, mode,
    skill_revision, content_digest, deployed_at";

/// Read the deployment for one skill at one Agent root, if any.
pub(super) fn get_deployment(
    storage: &Storage,
    skill_id: &str,
    agent_id: &str,
    config_root: &str,
) -> Result<Option<StoredDeployment>, SkillError> {
    let connection = lock(storage)?;
    connection
        .query_row(
            &format!(
                "SELECT {SELECT_COLS} FROM skill_deployment
                 WHERE skill_id=?1 AND agent_id=?2 AND config_root=?3"
            ),
            params![skill_id, agent_id, config_root],
            map_row,
        )
        .optional()
        .map_err(|_| SkillError::ReadFailed)
}

/// Insert or replace the ownership row after a successful filesystem write.
pub(super) fn upsert_deployment(
    storage: &Storage,
    row: &DeploymentUpsert<'_>,
) -> Result<(), SkillError> {
    let connection = lock(storage)?;
    connection
        .execute(
            "INSERT INTO skill_deployment(
                skill_id, agent_id, config_root, target_name, mode,
                skill_revision, content_digest, deployed_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(agent_id, config_root, target_name) DO UPDATE SET
                skill_id=excluded.skill_id,
                mode=excluded.mode,
                skill_revision=excluded.skill_revision,
                content_digest=excluded.content_digest,
                deployed_at=excluded.deployed_at",
            params![
                row.skill_id,
                row.agent_id,
                row.config_root,
                row.target_name,
                mode_value(row.mode),
                row.skill_revision,
                row.content_digest,
                row.deployed_at,
            ],
        )
        .map_err(|_| SkillError::WriteFailed)?;
    Ok(())
}

/// Remove the ownership row after a successful revoke (or detached clear).
pub(super) fn delete_deployment(
    storage: &Storage,
    skill_id: &str,
    agent_id: &str,
    config_root: &str,
) -> Result<(), SkillError> {
    let connection = lock(storage)?;
    let changed = connection
        .execute(
            "DELETE FROM skill_deployment
             WHERE skill_id=?1 AND agent_id=?2 AND config_root=?3",
            params![skill_id, agent_id, config_root],
        )
        .map_err(|_| SkillError::WriteFailed)?;
    if changed == 0 {
        return Err(SkillError::NotFound);
    }
    Ok(())
}

/// Every deployment under one Agent configuration root.
pub(super) fn list_deployments_for_root(
    storage: &Storage,
    agent_id: &str,
    config_root: &str,
) -> Result<Vec<StoredDeployment>, SkillError> {
    let connection = lock(storage)?;
    let mut statement = connection
        .prepare(&format!(
            "SELECT {SELECT_COLS} FROM skill_deployment
             WHERE agent_id=?1 AND config_root=?2
             ORDER BY target_name, skill_id"
        ))
        .map_err(|_| SkillError::ReadFailed)?;
    let rows = statement
        .query_map(params![agent_id, config_root], map_row)
        .map_err(|_| SkillError::ReadFailed)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SkillError::ReadFailed)?;
    Ok(rows)
}

/// Every configuration root named by an ownership row or a journal row.
pub(super) fn list_config_roots(storage: &Storage) -> Result<Vec<String>, SkillError> {
    let connection = lock(storage)?;
    let mut statement = connection
        .prepare(
            "SELECT config_root FROM skill_deployment
             UNION
             SELECT config_root FROM skill_deploy_operation
             ORDER BY 1",
        )
        .map_err(|_| SkillError::ReadFailed)?;
    let rows = statement
        .query_map([], |row| row.get(0))
        .map_err(|_| SkillError::ReadFailed)?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|_| SkillError::ReadFailed)?;
    Ok(rows)
}

/// Every deployment recorded for one central skill.
pub(super) fn list_deployments_for_skill(
    storage: &Storage,
    skill_id: &str,
) -> Result<Vec<StoredDeployment>, SkillError> {
    let connection = lock(storage)?;
    let mut statement = connection
        .prepare(&format!(
            "SELECT {SELECT_COLS} FROM skill_deployment
             WHERE skill_id=?1
             ORDER BY agent_id, config_root"
        ))
        .map_err(|_| SkillError::ReadFailed)?;
    let rows = statement
        .query_map(params![skill_id], map_row)
        .map_err(|_| SkillError::ReadFailed)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SkillError::ReadFailed)?;
    Ok(rows)
}

/// Whether any Agent still holds a deployment of this skill.
pub(super) fn skill_has_deployments(storage: &Storage, skill_id: &str) -> Result<bool, SkillError> {
    let connection = lock(storage)?;
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM skill_deployment WHERE skill_id=?1)",
            [skill_id],
            |row| row.get(0),
        )
        .map_err(|_| SkillError::ReadFailed)
}

/// One unfinished deploy/disable journal row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StoredOperation {
    pub(super) id: String,
    pub(super) kind: SkillDeployOperationKind,
    pub(super) state: SkillDeployOperationState,
    pub(super) skill_id: String,
    pub(super) agent_id: String,
    pub(super) config_root: String,
    pub(super) target_name: String,
    pub(super) target_path: String,
    pub(super) mode: Option<SkillDeployMode>,
    pub(super) skill_revision: i64,
    pub(super) content_digest: String,
    pub(super) created_at: i64,
    pub(super) updated_at: i64,
}

/// Values written when a deploy or disable starts mutating the filesystem.
pub(super) struct OperationInsert<'a> {
    pub(super) id: &'a str,
    pub(super) kind: SkillDeployOperationKind,
    pub(super) skill_id: &'a str,
    pub(super) agent_id: &'a str,
    pub(super) config_root: &'a str,
    pub(super) target_name: &'a str,
    pub(super) target_path: &'a str,
    pub(super) mode: Option<SkillDeployMode>,
    pub(super) skill_revision: i64,
    pub(super) content_digest: &'a str,
    pub(super) now_ms: i64,
}

fn kind_value(kind: SkillDeployOperationKind) -> &'static str {
    match kind {
        SkillDeployOperationKind::Apply => "apply",
        SkillDeployOperationKind::Disable => "disable",
    }
}

fn kind_from_stored(value: &str) -> Result<SkillDeployOperationKind, SkillError> {
    match value {
        "apply" => Ok(SkillDeployOperationKind::Apply),
        "disable" => Ok(SkillDeployOperationKind::Disable),
        _ => Err(SkillError::InvalidStoredSkill),
    }
}

fn state_from_stored(value: &str) -> Result<SkillDeployOperationState, SkillError> {
    match value {
        "prepared" => Ok(SkillDeployOperationState::Prepared),
        "unknown" => Ok(SkillDeployOperationState::Unknown),
        _ => Err(SkillError::InvalidStoredSkill),
    }
}

fn optional_mode_value(mode: Option<SkillDeployMode>) -> Option<&'static str> {
    mode.map(mode_value)
}

fn optional_mode_from_stored(value: Option<String>) -> Result<Option<SkillDeployMode>, SkillError> {
    match value {
        None => Ok(None),
        Some(text) => mode_from_stored(&text).map(Some),
    }
}

fn map_operation_row(row: &rusqlite::Row<'_>) -> Result<StoredOperation, rusqlite::Error> {
    let kind: String = row.get(1)?;
    let state: String = row.get(2)?;
    let mode: Option<String> = row.get(8)?;
    Ok(StoredOperation {
        id: row.get(0)?,
        kind: kind_from_stored(&kind).map_err(|_| rusqlite::Error::InvalidQuery)?,
        state: state_from_stored(&state).map_err(|_| rusqlite::Error::InvalidQuery)?,
        skill_id: row.get(3)?,
        agent_id: row.get(4)?,
        config_root: row.get(5)?,
        target_name: row.get(6)?,
        target_path: row.get(7)?,
        mode: optional_mode_from_stored(mode).map_err(|_| rusqlite::Error::InvalidQuery)?,
        skill_revision: row.get(9)?,
        content_digest: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

const OPERATION_COLS: &str = "id, kind, state, skill_id, agent_id, config_root, target_name,
    target_path, mode, skill_revision, content_digest, created_at, updated_at";

/// Insert a `prepared` journal row before the first filesystem change.
pub(super) fn insert_operation(
    storage: &Storage,
    row: &OperationInsert<'_>,
) -> Result<(), SkillError> {
    let connection = lock(storage)?;
    connection
        .execute(
            "INSERT INTO skill_deploy_operation(
                id, kind, state, skill_id, agent_id, config_root, target_name,
                target_path, mode, skill_revision, content_digest, created_at, updated_at
             ) VALUES(?1,?2,'prepared',?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
            params![
                row.id,
                kind_value(row.kind),
                row.skill_id,
                row.agent_id,
                row.config_root,
                row.target_name,
                row.target_path,
                optional_mode_value(row.mode),
                row.skill_revision,
                row.content_digest,
                row.now_ms,
            ],
        )
        .map_err(|_| SkillError::WriteFailed)?;
    Ok(())
}

/// Mark a journal row unknown when rollback after a write cannot be proven.
pub(super) fn mark_operation_unknown(
    storage: &Storage,
    id: &str,
    now_ms: i64,
) -> Result<(), SkillError> {
    let connection = lock(storage)?;
    let changed = connection
        .execute(
            "UPDATE skill_deploy_operation
             SET state='unknown', updated_at=?2
             WHERE id=?1",
            params![id, now_ms],
        )
        .map_err(|_| SkillError::WriteFailed)?;
    if changed == 0 {
        return Err(SkillError::NotFound);
    }
    Ok(())
}

/// Remove a journal row after ownership and the filesystem agree, or after dismiss.
pub(super) fn delete_operation(storage: &Storage, id: &str) -> Result<(), SkillError> {
    let connection = lock(storage)?;
    let changed = connection
        .execute("DELETE FROM skill_deploy_operation WHERE id=?1", [id])
        .map_err(|_| SkillError::WriteFailed)?;
    if changed == 0 {
        return Err(SkillError::NotFound);
    }
    Ok(())
}

/// Read one journal row by identity.
pub(super) fn get_operation(
    storage: &Storage,
    id: &str,
) -> Result<Option<StoredOperation>, SkillError> {
    let connection = lock(storage)?;
    connection
        .query_row(
            &format!("SELECT {OPERATION_COLS} FROM skill_deploy_operation WHERE id=?1"),
            [id],
            map_operation_row,
        )
        .optional()
        .map_err(|_| SkillError::ReadFailed)
}

/// Every unfinished journal row, oldest first.
pub(super) fn list_operations(storage: &Storage) -> Result<Vec<StoredOperation>, SkillError> {
    let connection = lock(storage)?;
    let mut statement = connection
        .prepare(&format!(
            "SELECT {OPERATION_COLS} FROM skill_deploy_operation
             ORDER BY created_at ASC, id ASC"
        ))
        .map_err(|_| SkillError::ReadFailed)?;
    let rows = statement
        .query_map([], map_operation_row)
        .map_err(|_| SkillError::ReadFailed)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SkillError::ReadFailed)?;
    Ok(rows)
}

/// Whether any unfinished journal row still names this skill.
pub(super) fn skill_has_operations(storage: &Storage, skill_id: &str) -> Result<bool, SkillError> {
    let connection = lock(storage)?;
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM skill_deploy_operation WHERE skill_id=?1)",
            [skill_id],
            |row| row.get(0),
        )
        .map_err(|_| SkillError::ReadFailed)
}
