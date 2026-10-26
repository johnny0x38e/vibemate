//! Grok proposals use isolated metadata and a store that panics on secret reads.

use super::*;
use serde_json::json;

fn portable_request() -> SaveMcpRequest {
    let mut input = request();
    if let ConnectionInput::Stdio { cwd, .. } = &mut input.connection {
        // The pure preflight runs on every native platform. Absolute paths follow
        // that platform's semantics, not the macOS evidence machine's syntax.
        *cwd = Some(std::env::temp_dir().to_str().unwrap().to_owned());
    }
    input
}

#[test]
fn grok_preflight_masks_stdio_preserves_arguments_and_does_not_mutate() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let mut input = portable_request();
    if let ConnectionInput::Stdio { args, env, .. } = &mut input.connection {
        *args = vec!["literal space".into(), "--flag".into(), "$LITERAL".into()];
        *env = vec![secret("TOKEN", Some("synthetic-grok-private-value"))];
    }
    let row = save_definition(&fixture.storage, &store, input, 1).unwrap();
    let before_row = serde_json::to_value(&row).unwrap();
    let before_secrets = store.values.borrow().clone();
    let original = br#"
# User content is not part of the proposal
[ui]
theme = "native-private-setting"
[mcp_servers.other.env]
TOKEN = "native-private-value"
"#;
    let plan = preflight_grok_registration(&fixture.storage, &row.id, row.revision, Some(original))
        .unwrap();
    assert_eq!(plan.definition_id, row.id);
    assert_eq!(plan.revision, row.revision);
    assert_eq!(plan.server_name, "tools");
    assert!(!plan.disabled_by_user_list);
    assert_eq!(
        plan.redacted_entry,
        json!({
            "command": "nonexistent-vibemate-test-server",
            "args": ["literal space", "--flag", "$LITERAL"],
            "cwd": std::env::temp_dir().to_str().unwrap(),
            "env": {"TOKEN": crate::config_plan::SECRET_PLACEHOLDER},
            "enabled": true
        })
    );
    let output = format!("{plan:?} {}", serde_json::to_string(&plan).unwrap());
    for private in [
        "synthetic-grok-private-value",
        "native-private-value",
        "native-private-setting",
        "other",
    ] {
        assert!(!output.contains(private));
    }
    assert_eq!(*store.values.borrow(), before_secrets);
    assert_eq!(
        serde_json::to_value(get_definition(&fixture.storage, &row.id).unwrap()).unwrap(),
        before_row
    );
    assert!(
        fixture
            .storage
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM config_operation", [], |r| r
                .get::<_, i64>(0))
            .unwrap()
            == 0
    );
}

#[test]
fn grok_preflight_maps_disabled_http_without_stdio_or_invented_type_fields() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let mut input = request();
    input.enabled = false;
    input.connection = ConnectionInput::Http {
        url: "https://example.com/mcp".into(),
        headers: vec![secret("Authorization", Some("Bearer synthetic-grok-http"))],
    };
    let row = save_definition(&fixture.storage, &store, input, 1).unwrap();
    let plan = preflight_grok_registration(&fixture.storage, &row.id, row.revision, None).unwrap();
    assert_eq!(
        plan.redacted_entry,
        json!({
            "url": "https://example.com/mcp", "enabled": false,
            "headers": {"authorization": crate::config_plan::SECRET_PLACEHOLDER}
        })
    );
    assert!(
        !serde_json::to_string(&plan)
            .unwrap()
            .contains("synthetic-grok-http")
    );
}

#[test]
fn grok_preflight_does_not_claim_enablement_over_the_personal_disable_list() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let row = save_definition(&fixture.storage, &store, portable_request(), 1).unwrap();
    for (bytes, disabled) in [
        (
            b"disabled_mcp_servers = ['other', 'tools']".as_slice(),
            true,
        ),
        (b"disabled_mcp_servers = ['TOOLS', 'other']", false),
        (b"disabled_mcp_servers = []", false),
    ] {
        let plan =
            preflight_grok_registration(&fixture.storage, &row.id, row.revision, Some(bytes))
                .unwrap();
        assert_eq!(plan.disabled_by_user_list, disabled);
        assert_eq!(plan.redacted_entry["enabled"], true);
    }
}

#[test]
fn grok_preflight_refuses_exact_names_even_when_disabled_or_malformed() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let mut input = portable_request();
    input.server_name = "my-tools".into();
    let row = save_definition(&fixture.storage, &store, input, 1).unwrap();
    for text in [
        "[mcp_servers.my-tools]\nenabled = false",
        "mcp_servers = { my-tools = 42 }",
        "mcp_servers.my-tools = []",
        "[[mcp_servers.my-tools]]",
    ] {
        assert_eq!(
            preflight_grok_registration(
                &fixture.storage,
                &row.id,
                row.revision,
                Some(text.as_bytes()),
            )
            .unwrap_err(),
            GrokMcpPreflightError::NameConflict
        );
    }
    for text in [
        "[mcp_servers.my_tools]\nenabled = true",
        "mcp_servers = { MY-tools = {} }",
        "mcp_servers = {}",
    ] {
        preflight_grok_registration(
            &fixture.storage,
            &row.id,
            row.revision,
            Some(text.as_bytes()),
        )
        .unwrap();
    }
}

#[test]
fn grok_preflight_rejects_names_that_cannot_qualify_tools_without_renaming_them() {
    for name in ["1tools", "my__tools", "tools_", "_"] {
        let fixture = Fixture::new();
        let store = FakeStore::default();
        let mut input = portable_request();
        input.server_name = name.into();
        let row = save_definition(&fixture.storage, &store, input, 1).unwrap();
        assert_eq!(
            preflight_grok_registration(&fixture.storage, &row.id, row.revision, None,)
                .unwrap_err(),
            GrokMcpPreflightError::UnsupportedServerName
        );
    }
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let mut input = portable_request();
    input.server_name = "_Tools-2".into();
    let row = save_definition(&fixture.storage, &store, input, 1).unwrap();
    assert_eq!(
        preflight_grok_registration(&fixture.storage, &row.id, row.revision, None,)
            .unwrap()
            .server_name,
        "_Tools-2"
    );
}

#[test]
fn grok_preflight_refuses_dynamic_stdio_fields_instead_of_expanding_them() {
    for field in ["command", "args", "cwd"] {
        let fixture = Fixture::new();
        let store = FakeStore::default();
        let mut input = portable_request();
        if let ConnectionInput::Stdio {
            command, args, cwd, ..
        } = &mut input.connection
        {
            match field {
                "command" => *command = "${PROBE}/server".into(),
                "args" => *args = vec!["${PROBE:-fallback}".into()],
                _ => *cwd = Some("${PROBE}/work".into()),
            }
        }
        let row = save_definition(&fixture.storage, &store, input, 1).unwrap();
        assert_eq!(
            preflight_grok_registration(&fixture.storage, &row.id, row.revision, None,)
                .unwrap_err(),
            GrokMcpPreflightError::DynamicValueUnsupported
        );
    }
}

#[test]
fn grok_preflight_refuses_unverified_path_semantics_without_dropping_cwd() {
    for field in ["command", "args", "cwd", "relative_cwd"] {
        let fixture = Fixture::new();
        let store = FakeStore::default();
        let mut input = portable_request();
        if let ConnectionInput::Stdio {
            command, args, cwd, ..
        } = &mut input.connection
        {
            match field {
                "command" => *command = "~/bin/server".into(),
                "args" => *args = vec!["~/data".into()],
                "cwd" => *cwd = Some("~/work".into()),
                _ => *cwd = Some("relative/work".into()),
            }
        }
        let row = save_definition(&fixture.storage, &store, input, 1).unwrap();
        assert_eq!(
            preflight_grok_registration(&fixture.storage, &row.id, row.revision, None,)
                .unwrap_err(),
            GrokMcpPreflightError::PathSemanticsUnverified
        );
    }
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let mut input = portable_request();
    if let ConnectionInput::Stdio { cwd, .. } = &mut input.connection {
        *cwd = None;
    }
    let row = save_definition(&fixture.storage, &store, input, 1).unwrap();
    let plan = preflight_grok_registration(&fixture.storage, &row.id, row.revision, None).unwrap();
    assert!(plan.redacted_entry.get("cwd").is_none());
}

#[test]
fn grok_preflight_rejects_invalid_containers_and_accepts_empty_toml() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let row = save_definition(&fixture.storage, &store, portable_request(), 1).unwrap();
    for bytes in [
        b"[".as_slice(),
        b"\xff",
        b"{}",
        b"mcp_servers = []",
        b"mcp_servers = 1",
        b"[[mcp_servers]]",
        b"mcp_servers = 'secret-native-error'",
        b"disabled_mcp_servers = false",
        b"disabled_mcp_servers = [1]",
        b"disabled_mcp_servers = ['tools', false]",
        b"[mcp_servers]\nx = 1\nx = 2",
    ] {
        let error =
            preflight_grok_registration(&fixture.storage, &row.id, row.revision, Some(bytes))
                .unwrap_err();
        assert_eq!(error, GrokMcpPreflightError::InvalidConfig);
        assert!(!format!("{error:?}").contains("secret-native-error"));
    }
    let oversized = vec![b' '; crate::config_plan::MAX_TARGET_BYTES as usize + 1];
    assert_eq!(
        preflight_grok_registration(&fixture.storage, &row.id, row.revision, Some(&oversized),)
            .unwrap_err(),
        GrokMcpPreflightError::InvalidConfig
    );
    for bytes in [
        b"".as_slice(),
        b" \n\t",
        b"# only comments\n",
        b"[mcp_servers]",
        b"mcp_servers = {}",
    ] {
        preflight_grok_registration(&fixture.storage, &row.id, row.revision, Some(bytes)).unwrap();
    }
}

#[test]
fn grok_preflight_requires_current_existing_metadata() {
    let fixture = Fixture::new();
    let store = FakeStore::default();
    let row = save_definition(&fixture.storage, &store, portable_request(), 1).unwrap();
    assert_eq!(
        preflight_grok_registration(&fixture.storage, &row.id, row.revision + 1, None,)
            .unwrap_err(),
        GrokMcpPreflightError::Definition(McpError::RevisionConflict)
    );
    delete_definition(
        &fixture.storage,
        &store,
        DeleteMcpRequest {
            id: row.id.clone(),
            expected_revision: row.revision,
        },
    )
    .unwrap();
    assert_eq!(
        preflight_grok_registration(&fixture.storage, &row.id, row.revision, None,).unwrap_err(),
        GrokMcpPreflightError::Definition(McpError::NotFound)
    );
}
