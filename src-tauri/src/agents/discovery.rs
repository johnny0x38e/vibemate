//! Bounded filesystem observations for six fixed Agent CLIs. Contents are read only
//! for Pi package metadata tied to the resolved entry; configs remain unopened.

use super::*;
use std::{fs, io::Read, path::Path};

const METADATA_LIMIT: u64 = 64 * 1024;
const PATH_LIMIT: usize = 64;

fn display_path(path: &Path) -> Option<String> {
    let value = path.to_str()?;
    (path.is_absolute() && value.len() <= 4096 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn manual_path(value: Option<&str>) -> Result<Option<PathBuf>, DiscoveryError> {
    value
        .map(|value| {
            let path = PathBuf::from(value);
            display_path(&path)
                .map(|_| path)
                .ok_or(DiscoveryError::InvalidPath)
        })
        .transpose()
}

/// Resolve an Agent configuration root from an explicit override, then the
/// documented default below the platform home.
fn config_directory(
    home: &Path,
    override_path: Option<&OsString>,
    default: &str,
) -> (Option<PathBuf>, DirectorySource) {
    match override_path {
        Some(value) => {
            let path = PathBuf::from(value);
            if display_path(&path).is_some() {
                (Some(path), DirectorySource::Environment)
            } else {
                (None, DirectorySource::Unavailable)
            }
        }
        None => {
            let path = home.join(default);
            if display_path(&path).is_some() {
                (Some(path), DirectorySource::Default)
            } else {
                (None, DirectorySource::Unavailable)
            }
        }
    }
}

/// Claude Code's user configuration root: `CLAUDE_CONFIG_DIR` when set and
/// usable, then `<home>/.claude`.
///
/// Detection and the configuration preview resolve the root through this one
/// function, so an override is accepted or rejected identically in both.
pub(crate) fn claude_config_root(
    environment: &DiscoveryEnvironment,
) -> (Option<PathBuf>, DirectorySource) {
    config_directory(
        &environment.home,
        environment.claude_directory.as_ref(),
        ".claude",
    )
}

/// Pi's agent directory: `PI_CODING_AGENT_DIR` when set and usable, then
/// `<home>/.pi/agent`. Skill deployment writes under `<root>/skills/`.
pub(crate) fn pi_config_root(
    environment: &DiscoveryEnvironment,
) -> (Option<PathBuf>, DirectorySource) {
    config_directory(
        &environment.home,
        environment.pi_directory.as_ref(),
        ".pi/agent",
    )
}

/// Grok Build configuration root: `GROK_HOME` when set and usable, then
/// `<home>/.grok`. Skill deployment writes under `<root>/skills/`.
pub(crate) fn grok_config_root(
    environment: &DiscoveryEnvironment,
) -> (Option<PathBuf>, DirectorySource) {
    config_directory(
        &environment.home,
        environment.grok_directory.as_ref(),
        ".grok",
    )
}

/// Codex user state root: `CODEX_HOME` when non-empty, then `<home>/.codex`.
///
/// Codex ignores an empty CODEX_HOME, but rejects an explicit directory that
/// does not exist and canonicalizes one that does. Keep that distinction
/// without reading config. Discovery and Codex writers share this function.
pub(crate) fn codex_config_root(
    environment: &DiscoveryEnvironment,
) -> (Option<PathBuf>, DirectorySource) {
    let override_path = environment
        .codex_directory
        .as_ref()
        .filter(|p| !p.is_empty());
    let (root, source) = config_directory(&environment.home, override_path, ".codex");
    if override_path.is_some() {
        let resolved = root
            .filter(|p| fs::metadata(p).is_ok_and(|m| m.is_dir()))
            .and_then(|p| fs::canonicalize(p).ok())
            .filter(|p| display_path(p).is_some());
        match resolved {
            Some(root) => (Some(root), source),
            None => (None, DirectorySource::Unavailable),
        }
    } else {
        (root, source)
    }
}

fn file_state(path: &Path) -> FileState {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => FileState::Present,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => FileState::Missing,
        _ => FileState::Unavailable,
    }
}

fn entry_names(id: AgentId) -> &'static [&'static str] {
    match (id, cfg!(windows)) {
        (AgentId::ClaudeCode, true) => &["claude.exe", "claude.cmd", "claude.bat", "claude"],
        (AgentId::Codex, true) => &["codex.exe", "codex.cmd", "codex.bat", "codex"],
        (AgentId::OpenCode, true) => &["opencode.exe", "opencode.cmd", "opencode.bat", "opencode"],
        (AgentId::Hermes, true) => &["hermes.exe", "hermes.cmd", "hermes.bat", "hermes"],
        (AgentId::ClaudeCode, false) => &["claude"],
        (AgentId::Codex, false) => &["codex"],
        (AgentId::OpenCode, false) => &["opencode"],
        (AgentId::Hermes, false) => &["hermes"],
        (AgentId::Pi, true) => &["pi.exe", "pi.cmd", "pi.bat", "pi"],
        (AgentId::GrokBuild, true) => &["grok.exe", "grok.cmd", "grok.bat", "grok"],
        (AgentId::Pi, false) => &["pi"],
        (AgentId::GrokBuild, false) => &["grok"],
    }
}

fn has_entry(path: &Path) -> bool {
    // symlink_metadata retains broken links as unavailable observations. Do not
    // skip a shadowing entry and imply a later PATH candidate will be launched.
    !matches!(fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

fn candidate(
    id: AgentId,
    manual: Option<PathBuf>,
    environment: &DiscoveryEnvironment,
    directory: Option<&Path>,
) -> (Option<PathBuf>, EntrySource) {
    if let Some(path) = manual {
        return (Some(path), EntrySource::Manual);
    }
    if let Some(value) = &environment.path {
        for directory in std::env::split_paths(value).take(PATH_LIMIT) {
            if display_path(&directory).is_none() {
                continue;
            }
            for name in entry_names(id) {
                let path = directory.join(name);
                if has_entry(&path) {
                    return (Some(path), EntrySource::Path);
                }
            }
        }
    }
    // Codex npm/Homebrew installations have no single documented user bin root.
    // PATH and an explicit manual path are sufficient; do not guess a prefix.
    let roots = match id {
        AgentId::Codex => Vec::new(),
        // These installer fallback paths are documented for POSIX only.
        // Windows still uses its desktop PATH and explicit manual entries.
        AgentId::ClaudeCode | AgentId::OpenCode if cfg!(windows) => Vec::new(),
        AgentId::OpenCode => vec![environment.home.join(".opencode/bin")],
        AgentId::Hermes if cfg!(windows) => environment
            .local_app_data
            .as_ref()
            .map(PathBuf::from)
            .filter(|root| display_path(root).is_some())
            .map(|root| vec![root.join("hermes/bin")])
            .unwrap_or_default(),
        _ => vec![environment.home.join(".local/bin")],
    };
    let mut roots = roots;
    if id == AgentId::Pi
        && let Some(directory) = directory
    {
        roots.push(directory.join("bin"));
    }
    for root in roots {
        for name in entry_names(id) {
            let path = root.join(name);
            if has_entry(&path) {
                return (Some(path), EntrySource::UserEntry);
            }
        }
    }
    (None, EntrySource::None)
}

fn safe_version(value: &str) -> bool {
    if value.len() > 64 || !value.is_ascii() {
        return false;
    }
    let (core, suffix) = value.split_once('-').unwrap_or((value, ""));
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 10 && part.bytes().all(|byte| byte.is_ascii_digit())
        })
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
        && (!value.contains('-') || !suffix.is_empty())
}

pub(super) fn pi_version(entry: &Path) -> Option<String> {
    for directory in entry.parent()?.ancestors().take(8) {
        let manifest = directory.join("package.json");
        // Do not read directories, devices, FIFOs or symlinked metadata. The
        // byte cap is enforced while reading as well as before it.
        let metadata = fs::symlink_metadata(&manifest).ok();
        if !metadata.is_some_and(|m| m.is_file() && m.len() <= METADATA_LIMIT) {
            continue;
        }
        let file = fs::File::open(&manifest).ok()?;
        let mut bytes = Vec::new();
        file.take(METADATA_LIMIT + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 > METADATA_LIMIT {
            return None;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        if value["name"].as_str() != Some("@earendil-works/pi-coding-agent") {
            continue;
        }
        let bin = value["bin"]["pi"].as_str()?;
        let bin = Path::new(bin);
        if bin.is_absolute()
            || bin
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            return None;
        }
        if fs::canonicalize(directory.join(bin)).ok().as_deref() != Some(entry) {
            continue;
        }
        let version = value["version"].as_str()?;
        return safe_version(version).then(|| version.to_owned());
    }
    None
}

fn grok_version(entry: &Path) -> Option<String> {
    let name = entry.file_name()?.to_str()?.strip_prefix("grok-")?;
    for suffix in [
        "-macos-aarch64",
        "-macos-x86_64",
        "-linux-aarch64",
        "-linux-x86_64",
    ] {
        if let Some(version) = name.strip_suffix(suffix) {
            return safe_version(version).then(|| version.to_owned());
        }
    }
    None
}

/// Resolve the same textual root for discovery and read-only history.
/// Does not inspect the Agent entry or require a verified write contract.
pub(super) fn configuration_directory(
    id: AgentId,
    environment: &DiscoveryEnvironment,
) -> (Option<PathBuf>, DirectorySource) {
    match id {
        AgentId::ClaudeCode => claude_config_root(environment),
        AgentId::Codex => codex_config_root(environment),
        AgentId::OpenCode => {
            // This is a candidate config root, not a claim about effective config.
            // OPENCODE_CONFIG_DIR differs between v1 and v2; file/inline/project
            // overrides are deliberately not interpreted by read-only discovery.
            if environment.opencode_directory.is_some() {
                config_directory(
                    &environment.home,
                    environment.opencode_directory.as_ref(),
                    ".config/opencode",
                )
            } else if let Some(base) = environment
                .xdg_config_directory
                .as_ref()
                .filter(|p| !p.is_empty())
            {
                let root = PathBuf::from(base).join("opencode");
                if display_path(&root).is_some() {
                    (Some(root), DirectorySource::Environment)
                } else {
                    (None, DirectorySource::Unavailable)
                }
            } else {
                config_directory(&environment.home, None, ".config/opencode")
            }
        }
        AgentId::Hermes => {
            if environment.hermes_directory.is_some() {
                config_directory(
                    &environment.home,
                    environment.hermes_directory.as_ref(),
                    ".hermes",
                )
            } else if cfg!(windows) {
                match environment.local_app_data.as_ref() {
                    Some(base) => config_directory(&PathBuf::from(base), None, "hermes"),
                    None => (None, DirectorySource::Unavailable),
                }
            } else {
                config_directory(&environment.home, None, ".hermes")
            }
        }
        AgentId::Pi => config_directory(
            &environment.home,
            environment.pi_directory.as_ref(),
            ".pi/agent",
        ),
        AgentId::GrokBuild => config_directory(
            &environment.home,
            environment.grok_directory.as_ref(),
            ".grok",
        ),
    }
}

fn observe(
    id: AgentId,
    manual: Option<PathBuf>,
    environment: &DiscoveryEnvironment,
) -> AgentObservation {
    let (directory, directory_source) = configuration_directory(id, environment);
    let names: &[&str] = match id {
        AgentId::ClaudeCode => &["settings.json"],
        AgentId::Codex => &["config.toml", "auth.json"],
        AgentId::OpenCode => &["opencode.json", "opencode.jsonc"],
        AgentId::Hermes => &["config.yaml", ".env", "auth.json"],
        AgentId::Pi => &["settings.json", "models.json", "mcp.json"],
        AgentId::GrokBuild => &["config.toml", "managed_config.toml", "requirements.toml"],
    };
    let files = names
        .iter()
        .map(|&name| ConfigurationFile {
            name,
            state: directory
                .as_ref()
                .map_or(FileState::Unavailable, |root| file_state(&root.join(name))),
        })
        .collect();
    let (entry, source) = candidate(id, manual, environment, directory.as_deref());
    let mut observation = AgentObservation {
        id,
        state: EntryState::NotFound,
        source,
        entry_path: entry.as_deref().and_then(display_path),
        resolved_path: None,
        version: None,
        version_evidence: VersionEvidence::None,
        capability_evidence: CapabilityEvidence::Unverified,
        config_directory: directory.as_deref().and_then(display_path),
        directory_source,
        files,
    };
    if let Some(entry) = entry {
        match fs::metadata(&entry) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // Distinguish an explicit absent path from a broken symlink.
                if has_entry(&entry) {
                    observation.state = EntryState::Unavailable;
                }
            }
            Ok(metadata) if metadata.is_file() => {
                if let Ok(resolved) = fs::canonicalize(&entry)
                    && let Some(path) = display_path(&resolved)
                    && observation.entry_path.is_some()
                {
                    observation.state = EntryState::Found;
                    observation.resolved_path = Some(path);
                    observation.version = match id {
                        AgentId::Pi => pi_version(&resolved),
                        AgentId::GrokBuild => grok_version(&resolved),
                        // Native binaries and launchers have no safely linked
                        // version evidence here. Never run --version or inspect
                        // wrapper contents merely to fill a display field.
                        _ => None,
                    };
                    if observation.version.is_some() {
                        observation.version_evidence = if id == AgentId::Pi {
                            VersionEvidence::PackageMetadata
                        } else {
                            VersionEvidence::Filename
                        };
                    }
                    if id == AgentId::Pi && observation.version.as_deref() == Some("1.1.0") {
                        observation.capability_evidence = CapabilityEvidence::Documented;
                    }
                } else {
                    observation.state = EntryState::Unavailable;
                }
            }
            _ => observation.state = EntryState::Unavailable,
        }
    }
    observation
}

/// Observe six Agent entries in catalog order using a supplied environment.
/// Only regular Pi package metadata is read; Agent configs and executables are
/// never opened for content or run. Invalid manual paths reject before any scan.
pub fn discover_agents(
    environment: &DiscoveryEnvironment,
    input: &DiscoveryInput,
) -> Result<Vec<AgentObservation>, DiscoveryError> {
    if display_path(&environment.home).is_none() {
        return Err(DiscoveryError::LocationUnavailable);
    }
    // Validate every input before observing any filesystem candidate.
    let claude = manual_path(input.claude_path.as_deref())?;
    let codex = manual_path(input.codex_path.as_deref())?;
    let opencode = manual_path(input.opencode_path.as_deref())?;
    let pi = manual_path(input.pi_path.as_deref())?;
    let grok = manual_path(input.grok_path.as_deref())?;
    let hermes = manual_path(input.hermes_path.as_deref())?;
    Ok(vec![
        observe(AgentId::ClaudeCode, claude, environment),
        observe(AgentId::Codex, codex, environment),
        observe(AgentId::OpenCode, opencode, environment),
        observe(AgentId::Pi, pi, environment),
        observe(AgentId::GrokBuild, grok, environment),
        observe(AgentId::Hermes, hermes, environment),
    ])
}
