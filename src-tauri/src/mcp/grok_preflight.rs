//! Pure Grok MCP first-registration checks against supplied TOML bytes. This
//! module returns only a masked proposal; target discovery, OS credentials,
//! filesystem writes, ownership and runtime server discovery belong elsewhere.

use serde::Serialize;
use serde_json::{Value, json};
use toml_edit::DocumentMut;

use super::{ConnectionRecord, McpError, get_definition};
use crate::{config_plan::SECRET_PLACEHOLDER, storage::Storage};

/// Safe failures that never include native TOML, parser diagnostics or values.
#[derive(Debug, PartialEq, Eq)]
pub enum GrokMcpPreflightError {
    /// Central metadata is unavailable, absent or at a different revision.
    Definition(McpError),
    /// Supplied bytes exceed bounds or contain an invalid supported container.
    InvalidConfig,
    /// The exact native name exists, regardless of its contents or enablement.
    NameConflict,
    /// Grok cannot admit qualified tools with this central server identifier.
    UnsupportedServerName,
    /// A public field would introduce Grok environment expansion.
    DynamicValueUnsupported,
    /// Relative cwd or leading home expansion has not been verified for Grok.
    PathSemanticsUnverified,
}

/// New-entry proposal, not an approval token or a written/connected registration.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokMcpPreflight {
    /// Stable central identity used for this proposal.
    pub definition_id: String,
    /// Central revision to recheck before any future mutation.
    pub revision: i64,
    /// Exact key proposed below `mcp_servers`, without Pi name normalization.
    pub server_name: String,
    /// Native TOML entry represented as JSON; env/header values are placeholders.
    pub redacted_entry: Value,
    /// The current user-level disable list independently disables this name.
    /// This proposal neither removes the name nor asserts runtime effectiveness.
    pub disabled_by_user_list: bool,
}

/// Propose a first user-level Grok registration without performing side effects.
///
/// `native_bytes` is caller-supplied TOML, not a frontend write document. `None`
/// means absent; blank TOML is a valid empty document. A future IPC layer must
/// select and bound the target in Rust. Only central metadata is read: credentials
/// are not loaded, existing native content is not returned, and no server,
/// command, network request or write is executed. Project/policy/compatibility
/// sources and running sessions remain outside this preflight.
///
/// # Errors
/// Rejects stale/deleted definitions, malformed/oversized native containers and
/// exact existing names (there is no ownership ledger permitting replacement).
/// Names must be usable in Grok's qualified tool catalog. Dynamic public fields,
/// relative cwd and unverified leading `~/` paths are explicitly refused rather
/// than silently translated or dropped. Errors contain no supplied file content.
pub fn preflight_grok_registration(
    storage: &Storage,
    definition_id: &str,
    expected_revision: i64,
    native_bytes: Option<&[u8]>,
) -> Result<GrokMcpPreflight, GrokMcpPreflightError> {
    use GrokMcpPreflightError::{Definition, UnsupportedServerName};
    let record = get_definition(storage, definition_id).map_err(Definition)?;
    if record.revision != expected_revision {
        return Err(Definition(McpError::RevisionConflict));
    }
    // Central identifiers are portable but deliberately broader than Grok's
    // catalog prefix: a TOML-valid name can still make every tool inadmissible.
    if !record
        .server_name
        .bytes()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        || record.server_name.contains("__")
        || record.server_name.ends_with('_')
    {
        return Err(UnsupportedServerName);
    }
    let disabled_by_user_list = check_native(native_bytes, &record.server_name)?;
    let redacted_entry = match record.connection {
        ConnectionRecord::Stdio {
            command,
            args,
            cwd,
            env,
        } => {
            for value in std::iter::once(&command)
                .chain(args.iter())
                .chain(cwd.iter())
            {
                if value.contains("${") {
                    return Err(GrokMcpPreflightError::DynamicValueUnsupported);
                }
                if value.starts_with("~/") {
                    return Err(GrokMcpPreflightError::PathSemanticsUnverified);
                }
            }
            if cwd
                .as_ref()
                .is_some_and(|path| !std::path::Path::new(path).is_absolute())
            {
                return Err(GrokMcpPreflightError::PathSemanticsUnverified);
            }
            let mut entry = json!({
                "command": command, "args": args,
                "env": masked_fields(env), "enabled": record.enabled
            });
            if let Some(cwd) = cwd {
                entry["cwd"] = json!(cwd);
            }
            entry
        }
        ConnectionRecord::Http { url, headers } => {
            // Grok's own HTTP CLI writes url without a type discriminator. Do
            // not invent Pi's type syntax or map Streamable HTTP to legacy SSE.
            json!({"url": url, "headers": masked_fields(headers), "enabled": record.enabled})
        }
    };
    Ok(GrokMcpPreflight {
        definition_id: record.id,
        revision: record.revision,
        server_name: record.server_name,
        redacted_entry,
        disabled_by_user_list,
    })
}

fn masked_fields(names: Vec<String>) -> serde_json::Map<String, Value> {
    names
        .into_iter()
        .map(|name| (name, json!(SECRET_PLACEHOLDER)))
        .collect()
}

fn check_native(bytes: Option<&[u8]>, name: &str) -> Result<bool, GrokMcpPreflightError> {
    use GrokMcpPreflightError::{InvalidConfig, NameConflict};
    let Some(bytes) = bytes else {
        return Ok(false);
    };
    if bytes.len() as u64 > crate::config_plan::MAX_TARGET_BYTES {
        return Err(InvalidConfig);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| InvalidConfig)?;
    let document = text.parse::<DocumentMut>().map_err(|_| InvalidConfig)?;
    if let Some(servers) = document.get("mcp_servers") {
        let servers = servers.as_table_like().ok_or(InvalidConfig)?;
        // Grok uses server__tool, keeping '-' intact. Unlike Pi, my-tools and
        // my_tools are separate catalog prefixes; only the exact key conflicts.
        if servers.contains_key(name) {
            return Err(NameConflict);
        }
    }
    let mut disabled = false;
    if let Some(names) = document.get("disabled_mcp_servers") {
        let names = names.as_array().ok_or(InvalidConfig)?;
        for value in names {
            let value = value.as_str().ok_or(InvalidConfig)?;
            disabled |= value == name;
        }
    }
    // Invalid individual foreign server entries are never inspected or emitted.
    // They still occupy their names; replacing them requires explicit ownership.
    Ok(disabled)
}
