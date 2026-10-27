//! Filesystem preview tests use isolated roots and never inspect real Agent data.

use super::*;
use crate::agents::DiscoveryEnvironment;

fn input(row: &McpRecord) -> PreviewPiMcpRequest {
    PreviewPiMcpRequest {
        definition_id: row.id.clone(),
        expected_revision: row.revision,
    }
}

#[test]
fn pi_preview_resolves_default_without_creating_a_target() {
    let f = Fixture::new();
    let store = FakeStore::default();
    let row = save_definition(&f.storage, &store, request(), 1).unwrap();
    let environment = DiscoveryEnvironment {
        home: f.path.clone(),
        ..Default::default()
    };
    let plan = preview_pi_registration(&f.storage, &environment, &input(&row)).unwrap();
    // `pi_config_root` joins the relative default ".pi/agent" in one step, then
    // "mcp.json". On Windows the slash inside that relative path stays.
    assert_eq!(
        plan.target.path,
        f.path.join(".pi/agent").join("mcp.json").to_str().unwrap()
    );
    assert!(!plan.target.exists);
    assert_eq!(plan.target.content_digest, None);
    assert!(!f.path.join(".pi").exists());
}

#[test]
fn pi_preview_honors_override_and_never_returns_existing_document_values() {
    let f = Fixture::new();
    let store = FakeStore::default();
    let row = save_definition(&f.storage, &store, request(), 1).unwrap();
    let root = f.path.join("custom-agent");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("mcp.json");
    let bytes = br#"{"autoEnableCodemode":false,"mcpServers":{"other":{"headers":{"Authorization":"private-native-synthetic"}}}}"#;
    std::fs::write(&path, bytes).unwrap();
    let environment = DiscoveryEnvironment {
        home: f.path.clone(),
        pi_directory: Some(root.into_os_string()),
        ..Default::default()
    };
    let plan = preview_pi_registration(&f.storage, &environment, &input(&row)).unwrap();
    assert!(plan.target.exists);
    assert_eq!(plan.target.path, path.to_str().unwrap());
    assert!(plan.target.content_digest.is_some());
    assert!(
        !serde_json::to_string(&plan)
            .unwrap()
            .contains("private-native-synthetic")
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(!f.path.join(".pi").exists());
}

#[test]
fn pi_preview_rejects_relative_override_invalid_files_and_directories() {
    let f = Fixture::new();
    let store = FakeStore::default();
    let row = save_definition(&f.storage, &store, request(), 1).unwrap();
    let mut environment = DiscoveryEnvironment {
        home: f.path.clone(),
        pi_directory: Some("relative".into()),
        ..Default::default()
    };
    assert_eq!(
        preview_pi_registration(&f.storage, &environment, &input(&row)).unwrap_err(),
        PiMcpPreviewError::TargetUnavailable
    );
    environment.pi_directory = Some(f.path.clone().into_os_string());
    let path = f.path.join("mcp.json");
    std::fs::write(&path, b"invalid").unwrap();
    assert_eq!(
        preview_pi_registration(&f.storage, &environment, &input(&row)).unwrap_err(),
        PiMcpPreviewError::InvalidConfig
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(
        preview_pi_registration(&f.storage, &environment, &input(&row)).unwrap_err(),
        PiMcpPreviewError::TargetUnavailable
    );
}

#[test]
fn pi_preview_rejects_frontend_paths_and_reports_safe_conflict_codes() {
    let f = Fixture::new();
    let store = FakeStore::default();
    let row = save_definition(&f.storage, &store, request(), 1).unwrap();
    assert!(
        serde_json::from_value::<PreviewPiMcpRequest>(serde_json::json!({
            "definitionId":row.id,"expectedRevision":row.revision,"path":"/anywhere"
        }))
        .is_err()
    );
    let environment = DiscoveryEnvironment {
        home: f.path.clone(),
        pi_directory: Some(f.path.clone().into_os_string()),
        ..Default::default()
    };
    let mut request = input(&row);
    request.expected_revision += 1;
    assert_eq!(
        preview_pi_registration(&f.storage, &environment, &request).unwrap_err(),
        PiMcpPreviewError::RevisionConflict
    );
    std::fs::write(
        f.path.join("mcp.json"),
        br#"{"mcpServers":{"tools":{"enabled":false}}}"#,
    )
    .unwrap();
    assert_eq!(
        preview_pi_registration(&f.storage, &environment, &input(&row)).unwrap_err(),
        PiMcpPreviewError::NameConflict
    );
    request.definition_id = "not-an-id".into();
    assert_eq!(
        preview_pi_registration(&f.storage, &environment, &request).unwrap_err(),
        PiMcpPreviewError::InvalidRequest
    );
}

#[cfg(unix)]
#[test]
fn pi_preview_reports_symlinks_without_replacing_them_and_refuses_dangling_links() {
    let f = Fixture::new();
    let store = FakeStore::default();
    let row = save_definition(&f.storage, &store, request(), 1).unwrap();
    let environment = DiscoveryEnvironment {
        home: f.path.clone(),
        pi_directory: Some(f.path.clone().into_os_string()),
        ..Default::default()
    };
    let destination = f.path.join("native.json");
    std::fs::write(&destination, b"{}").unwrap();
    let path = f.path.join("mcp.json");
    std::os::unix::fs::symlink(&destination, &path).unwrap();
    assert!(
        preview_pi_registration(&f.storage, &environment, &input(&row))
            .unwrap()
            .target
            .symlink
    );
    assert!(path.is_symlink());
    std::fs::remove_file(&destination).unwrap();
    assert_eq!(
        preview_pi_registration(&f.storage, &environment, &input(&row)).unwrap_err(),
        PiMcpPreviewError::TargetUnavailable
    );
    assert!(path.is_symlink());
}
