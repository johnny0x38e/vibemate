//! Target previews use isolated roots; real Agent files and OS keys are untouched.

use super::*;
use crate::agents::DiscoveryEnvironment;

fn selection(row: &McpRecord) -> PreviewGrokMcpRequest {
    PreviewGrokMcpRequest {
        definition_id: row.id.clone(),
        expected_revision: row.revision,
    }
}

fn definition(f: &Fixture) -> McpRecord {
    let mut input = request();
    if let ConnectionInput::Stdio { cwd, .. } = &mut input.connection {
        *cwd = None;
    }
    save_definition(&f.storage, &FakeStore::default(), input, 1).unwrap()
}

fn environment(f: &Fixture) -> DiscoveryEnvironment {
    DiscoveryEnvironment {
        home: f.path.clone(),
        grok_directory: Some(f.path.clone().into_os_string()),
        ..Default::default()
    }
}

#[test]
fn grok_preview_resolves_default_without_creating_files_or_directories() {
    let f = Fixture::new();
    let row = definition(&f);
    let environment = DiscoveryEnvironment {
        home: f.path.clone(),
        ..Default::default()
    };
    let plan = preview_grok_registration(&f.storage, &environment, &selection(&row)).unwrap();
    assert_eq!(
        plan.target.path,
        f.path.join(".grok/config.toml").to_str().unwrap()
    );
    assert!(!plan.target.exists);
    assert_eq!(plan.target.content_digest, None);
    assert!(!f.path.join(".grok").exists());
}

#[test]
fn grok_preview_honors_override_masks_existing_content_and_reports_personal_disable() {
    let f = Fixture::new();
    let row = definition(&f);
    let root = f.path.join("custom-grok");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("config.toml");
    let bytes = b"disabled_mcp_servers = ['tools']\n[model.user]\napi_key = 'synthetic-native-private'\n[mcp_servers.other]\ncommand = 'never-execute-user-command'\n";
    std::fs::write(&path, bytes).unwrap();
    let environment = DiscoveryEnvironment {
        home: f.path.clone(),
        grok_directory: Some(root.into_os_string()),
        ..Default::default()
    };
    let plan = preview_grok_registration(&f.storage, &environment, &selection(&row)).unwrap();
    assert!(plan.target.exists);
    assert_eq!(plan.target.path, path.to_str().unwrap());
    assert!(plan.target.content_digest.is_some());
    assert!(plan.proposal.disabled_by_user_list);
    let output = serde_json::to_string(&plan).unwrap();
    for private in [
        "synthetic-native-private",
        "never-execute-user-command",
        "api_key",
    ] {
        assert!(!output.contains(private));
    }
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(!f.path.join(".grok").exists());
}

#[test]
fn grok_preview_rejects_invalid_overrides_file_types_and_oversized_targets() {
    let f = Fixture::new();
    let row = definition(&f);
    let mut env = environment(&f);
    for root in ["relative".into(), "".into()] {
        env.grok_directory = Some(root);
        assert_eq!(
            preview_grok_registration(&f.storage, &env, &selection(&row)).unwrap_err(),
            GrokMcpPreviewError::TargetUnavailable
        );
    }
    env = environment(&f);
    let path = f.path.join("config.toml");
    std::fs::create_dir(&path).unwrap();
    assert_eq!(
        preview_grok_registration(&f.storage, &env, &selection(&row)).unwrap_err(),
        GrokMcpPreviewError::TargetUnavailable
    );
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(
        &path,
        vec![b' '; crate::config_plan::MAX_TARGET_BYTES as usize + 1],
    )
    .unwrap();
    assert_eq!(
        preview_grok_registration(&f.storage, &env, &selection(&row)).unwrap_err(),
        GrokMcpPreviewError::TargetUnavailable
    );
    std::fs::write(&path, b"not valid TOML = '").unwrap();
    assert_eq!(
        preview_grok_registration(&f.storage, &env, &selection(&row)).unwrap_err(),
        GrokMcpPreviewError::InvalidConfig
    );
    std::fs::write(&path, [0xff]).unwrap();
    assert_eq!(
        preview_grok_registration(&f.storage, &env, &selection(&row)).unwrap_err(),
        GrokMcpPreviewError::InvalidConfig
    );
}

#[test]
fn grok_preview_rejects_frontend_paths_documents_and_invalid_selections() {
    let f = Fixture::new();
    let row = definition(&f);
    for (key, value) in [
        ("path", serde_json::json!("/anywhere")),
        ("nativeBytes", serde_json::json!([])),
    ] {
        let mut request =
            serde_json::json!({"definitionId":row.id,"expectedRevision":row.revision});
        request[key] = value;
        assert!(serde_json::from_value::<PreviewGrokMcpRequest>(request).is_err());
    }
    for request in [
        PreviewGrokMcpRequest {
            definition_id: "not-an-id".into(),
            expected_revision: 1,
        },
        PreviewGrokMcpRequest {
            definition_id: row.id.clone(),
            expected_revision: 0,
        },
    ] {
        assert_eq!(
            preview_grok_registration(&f.storage, &environment(&f), &request).unwrap_err(),
            GrokMcpPreviewError::InvalidRequest
        );
    }
    let mut request = selection(&row);
    request.expected_revision += 1;
    assert_eq!(
        preview_grok_registration(&f.storage, &environment(&f), &request).unwrap_err(),
        GrokMcpPreviewError::RevisionConflict
    );
    std::fs::write(
        f.path.join("config.toml"),
        "[mcp_servers.tools]\nenabled = false",
    )
    .unwrap();
    assert_eq!(
        preview_grok_registration(&f.storage, &environment(&f), &selection(&row)).unwrap_err(),
        GrokMcpPreviewError::NameConflict
    );
    std::fs::remove_file(f.path.join("config.toml")).unwrap();
    delete_definition(
        &f.storage,
        &FakeStore::default(),
        DeleteMcpRequest {
            id: row.id.clone(),
            expected_revision: row.revision,
        },
    )
    .unwrap();
    assert_eq!(
        preview_grok_registration(&f.storage, &environment(&f), &selection(&row)).unwrap_err(),
        GrokMcpPreviewError::DefinitionUnavailable
    );
}

#[test]
fn grok_preview_maps_unsupported_input_errors_without_raw_values() {
    for (name, command, cwd, expected) in [
        (
            "1tools",
            "probe",
            None,
            GrokMcpPreviewError::UnsupportedServerName,
        ),
        (
            "tools",
            "${PROBE}/probe",
            None,
            GrokMcpPreviewError::DynamicValueUnsupported,
        ),
        (
            "tools",
            "probe",
            Some("relative/work"),
            GrokMcpPreviewError::PathSemanticsUnverified,
        ),
    ] {
        let f = Fixture::new();
        let mut input = request();
        input.server_name = name.into();
        if let ConnectionInput::Stdio {
            command: executable,
            cwd: directory,
            ..
        } = &mut input.connection
        {
            *executable = command.into();
            *directory = cwd.map(str::to_owned);
        }
        let row = save_definition(&f.storage, &FakeStore::default(), input, 1).unwrap();
        let error =
            preview_grok_registration(&f.storage, &environment(&f), &selection(&row)).unwrap_err();
        assert_eq!(error, expected);
        assert!(!serde_json::to_string(&error).unwrap().contains("${PROBE}"));
    }
}

#[cfg(unix)]
#[test]
fn grok_preview_reports_readable_symlink_and_refuses_dangling_link_without_replacement() {
    let f = Fixture::new();
    let row = definition(&f);
    let destination = f.path.join("native.toml");
    std::fs::write(&destination, b"# empty").unwrap();
    let path = f.path.join("config.toml");
    std::os::unix::fs::symlink(&destination, &path).unwrap();
    let plan = preview_grok_registration(&f.storage, &environment(&f), &selection(&row)).unwrap();
    assert!(plan.target.symlink);
    assert_eq!(std::fs::read(&destination).unwrap(), b"# empty");
    assert!(path.is_symlink());
    std::fs::remove_file(&destination).unwrap();
    assert_eq!(
        preview_grok_registration(&f.storage, &environment(&f), &selection(&row)).unwrap_err(),
        GrokMcpPreviewError::TargetUnavailable
    );
    assert!(path.is_symlink());
}
