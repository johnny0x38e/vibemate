//! Browse and install skills from the skills.sh catalog.
//!
//! Search uses the documented JSON API. Leaderboards read the public HTML pages
//! and extract the embedded skill objects; that adapter is isolated so a site
//! redesign fails with a clear error instead of inventing rows. Only public
//! GitHub `owner/repo` sources are installable; site-only hosts are refused.
//! Install reuses the Git acquisition path at `HEAD` for the named skill.

#[cfg(test)]
mod tests;

use super::SkillLibrary;
use super::install::{self, preview_skill_install};
use super::types::{
    GitInstallSource, MarketplaceBoard, MarketplaceInstallRequest, MarketplaceSkill, SkillError,
    SkillInstallPreview, SkillInstallReport, SkillInstallSource,
};
use super::validation;
use crate::storage::Storage;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Once;
use std::time::Duration;

const SEARCH_URL: &str = "https://skills.sh/api/search";
const USER_AGENT: &str = "vibemate/0.1 (+https://github.com/johnnyhuang/vibemate)";
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_SEARCH_LIMIT: usize = 60;
const MAX_LEADERBOARD: usize = 100;
const MAX_HTML_BYTES: usize = 2 * 1024 * 1024;
/// Longer Summary / body excerpt from a skill page (FE detail scroll).
const MAX_SUMMARY_LENGTH: usize = 8192;
/// Catalog `npx skills add …` hint length bound.
const MAX_CATALOG_INSTALL_COMMAND: usize = 512;
/// Marketplace installs follow the repository default branch tip.
const MARKETPLACE_REVISION: &str = "HEAD";

impl MarketplaceBoard {
    fn url(self) -> &'static str {
        match self {
            Self::AllTime => "https://www.skills.sh/",
            Self::Trending => "https://www.skills.sh/trending",
            Self::Hot => "https://www.skills.sh/hot",
        }
    }
}

/// Search the skills.sh catalog. The query must be at least two characters.
///
/// # Errors
/// Rejects a short query, network failures, timeouts, and unrecognized JSON.
pub async fn search_marketplace_skills(
    query: &str,
    limit: usize,
) -> Result<Vec<MarketplaceSkill>, SkillError> {
    let trimmed = query.trim();
    if trimmed.chars().count() < 2 || trimmed.len() > 200 {
        return Err(SkillError::InvalidRequest);
    }
    let limit = limit.clamp(1, MAX_SEARCH_LIMIT);
    let client = http_client()?;
    // Build the URL by hand so we do not depend on reqwest's optional query
    // serializer while default features stay off.
    let url = format!(
        "{SEARCH_URL}?q={}&limit={limit}",
        urlencoding_encode(trimmed)
    );
    let response = client.get(url).send().await.map_err(map_http_error)?;
    if !response.status().is_success() {
        return Err(SkillError::MarketplaceUnavailable);
    }
    let body = response.bytes().await.map_err(map_http_error)?;
    if body.len() > MAX_HTML_BYTES {
        return Err(SkillError::MarketplaceResponseInvalid);
    }
    let parsed: SearchResponse =
        serde_json::from_slice(&body).map_err(|_| SkillError::MarketplaceResponseInvalid)?;
    Ok(filter_github(
        parsed.skills.into_iter().filter_map(catalog_row),
    ))
}

/// Load one skills.sh leaderboard page and return GitHub-installable rows.
///
/// # Errors
/// Rejects network failures, timeouts, and pages with no parseable skills.
pub async fn list_marketplace_skills(
    board: MarketplaceBoard,
) -> Result<Vec<MarketplaceSkill>, SkillError> {
    let client = http_client()?;
    let response = client
        .get(board.url())
        .send()
        .await
        .map_err(map_http_error)?;
    if !response.status().is_success() {
        return Err(SkillError::MarketplaceUnavailable);
    }
    let body = response.text().await.map_err(map_http_error)?;
    if body.len() > MAX_HTML_BYTES {
        return Err(SkillError::MarketplaceResponseInvalid);
    }
    let skills = parse_leaderboard_html(&body)?;
    let filtered = filter_github(skills);
    if filtered.is_empty() {
        return Err(SkillError::MarketplaceResponseInvalid);
    }
    Ok(filtered.into_iter().take(MAX_LEADERBOARD).collect())
}

/// Fetch one skill's public skills.sh page and return enriched detail fields.
///
/// List/search rows leave page-only fields empty; the interface loads this when
/// a detail view opens. Never invents text when the page or meta tags are
/// missing. `catalog_install_command` is the skills.sh `npx skills add …` hint
/// when present — not vibemate's Git `preview_marketplace_install` command.
///
/// # Errors
/// Rejects an empty source/skill id, unsupported hosts, network failures,
/// timeouts, and pages without a usable description meta tag.
pub async fn get_marketplace_skill_detail(
    source: &str,
    skill_id: &str,
) -> Result<MarketplaceSkill, SkillError> {
    let source = source.trim();
    let skill_id = skill_id.trim();
    if source.is_empty() || skill_id.is_empty() {
        return Err(SkillError::InvalidRequest);
    }
    if !is_github_owner_repo(source) {
        return Err(SkillError::MarketplaceSourceUnsupported);
    }
    let skill = validation::skill_selector(Some(skill_id))?.ok_or(SkillError::InvalidRequest)?;
    let url = skill_page_url(source, skill)?;
    let client = http_client()?;
    let response = client.get(url).send().await.map_err(map_http_error)?;
    if !response.status().is_success() {
        return Err(SkillError::MarketplaceUnavailable);
    }
    let body = response.text().await.map_err(map_http_error)?;
    if body.len() > MAX_HTML_BYTES {
        return Err(SkillError::MarketplaceResponseInvalid);
    }
    Ok(parse_skill_page_detail(source, skill, &body)?)
}

/// Validate a marketplace row and show the Git command that would run.
///
/// No network request or library write happens here.
pub fn preview_marketplace_install(
    request: MarketplaceInstallRequest,
) -> Result<SkillInstallPreview, SkillError> {
    preview_skill_install(install_source(&request)?)
}

/// Install one marketplace skill into the central library from GitHub at HEAD.
///
/// # Errors
/// Same as a Git install, plus refusal of non-GitHub marketplace sources.
pub fn install_marketplace_skill(
    library: &SkillLibrary,
    storage: &Storage,
    request: MarketplaceInstallRequest,
    now_ms: i64,
) -> Result<SkillInstallReport, SkillError> {
    install::install_skill(library, storage, install_source(&request)?, now_ms)
}

fn install_source(request: &MarketplaceInstallRequest) -> Result<SkillInstallSource, SkillError> {
    let source = request.source.trim();
    let skill_id = request.skill_id.trim();
    if source.is_empty() || skill_id.is_empty() {
        return Err(SkillError::InvalidRequest);
    }
    if !is_github_owner_repo(source) {
        return Err(SkillError::MarketplaceSourceUnsupported);
    }
    // Canonicalize through the same GitHub validator the Git installer uses.
    let repository = validation::github_repository(source)?;
    let skill = validation::skill_selector(Some(skill_id))?.map(str::to_string);
    let skill = skill.ok_or(SkillError::InvalidRequest)?;
    Ok(SkillInstallSource::Git(GitInstallSource {
        repository,
        revision: MARKETPLACE_REVISION.to_string(),
        skill: Some(skill),
    }))
}

fn http_client() -> Result<reqwest::Client, SkillError> {
    // reqwest is built with rustls-no-provider; without this, Client::build panics
    // and the Tauri invoke never settles, leaving the marketplace UI on loading.
    install_crypto_provider();
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(HTTP_TIMEOUT)
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|_| SkillError::MarketplaceUnavailable)
}

/// Install rustls' ring provider once per process (same requirement as model_http).
fn install_crypto_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Percent-encode a query value with the same set `application/x-www-form-urlencoded` uses.
fn urlencoding_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

fn map_http_error(error: reqwest::Error) -> SkillError {
    if error.is_timeout() {
        SkillError::MarketplaceTimedOut
    } else {
        SkillError::MarketplaceUnavailable
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    skills: Vec<CatalogRow>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogRow {
    source: Option<String>,
    skill_id: Option<String>,
    name: Option<String>,
    installs: Option<u64>,
}

fn catalog_row(row: CatalogRow) -> Option<MarketplaceSkill> {
    let source = row.source?.trim().to_string();
    let skill_id = row.skill_id?.trim().to_string();
    if source.is_empty() || skill_id.is_empty() {
        return None;
    }
    let name = row
        .name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| skill_id.clone());
    Some(MarketplaceSkill {
        id: format!("{source}/{skill_id}"),
        source,
        skill_id,
        name,
        installs: row.installs,
        description: None,
        summary: None,
        catalog_install_command: None,
    })
}

fn filter_github(skills: impl IntoIterator<Item = MarketplaceSkill>) -> Vec<MarketplaceSkill> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for skill in skills {
        if !is_github_owner_repo(&skill.source) {
            continue;
        }
        if !seen.insert(skill.id.clone()) {
            continue;
        }
        out.push(skill);
    }
    out
}

/// skills.sh GitHub rows use `owner/repo` with exactly one slash.
fn is_github_owner_repo(source: &str) -> bool {
    validation::github_repository(source).is_ok() && source.matches('/').count() == 1
}

/// Extract skill objects from skills.sh HTML (RSC / escaped JSON payloads).
fn parse_leaderboard_html(html: &str) -> Result<Vec<MarketplaceSkill>, SkillError> {
    let mut skills = Vec::new();
    let mut seen = HashSet::new();
    let mut rest = html;
    while let Some(start) = rest.find(r#"\"source\":\""#) {
        rest = &rest[start + r#"\"source\":\""#.len()..];
        let Some((source, after_source)) = split_escaped(rest) else {
            break;
        };
        rest = after_source;
        let Some(skill_mark) = rest.find(r#"\"skillId\":\""#) else {
            continue;
        };
        // Refuse a huge gap so an unrelated later skillId is not paired wrongly.
        if skill_mark > 400 {
            continue;
        }
        rest = &rest[skill_mark + r#"\"skillId\":\""#.len()..];
        let Some((skill_id, after_skill)) = split_escaped(rest) else {
            break;
        };
        rest = after_skill;
        let name = take_escaped_field(rest, r#"\"name\":\""#).unwrap_or_else(|| skill_id.clone());
        let installs = take_installs(rest);
        let id = format!("{source}/{skill_id}");
        if !seen.insert(id.clone()) {
            continue;
        }
        skills.push(MarketplaceSkill {
            id,
            source,
            skill_id,
            name,
            installs,
            description: None,
            summary: None,
            catalog_install_command: None,
        });
    }
    if skills.is_empty() {
        return Err(SkillError::MarketplaceResponseInvalid);
    }
    Ok(skills)
}

fn split_escaped(input: &str) -> Option<(String, &str)> {
    let end = input.find(r#"\""#)?;
    let value = input[..end].replace(r#"\""#, "\"");
    if value.is_empty() || value.contains('\\') {
        return None;
    }
    Some((value, &input[end + 2..]))
}

fn take_escaped_field(input: &str, marker: &str) -> Option<String> {
    let start = input.find(marker)?;
    if start > 400 {
        return None;
    }
    let rest = &input[start + marker.len()..];
    split_escaped(rest).map(|(value, _)| value)
}

fn take_installs(input: &str) -> Option<u64> {
    let marker = r#"\"installs\":"#;
    let start = input.find(marker)?;
    if start > 400 {
        return None;
    }
    let rest = &input[start + marker.len()..];
    let digits: String = rest
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn skill_page_url(source: &str, skill_id: &str) -> Result<String, SkillError> {
    let (owner, repo) = source.split_once('/').ok_or(SkillError::InvalidRequest)?;
    if owner.is_empty() || repo.is_empty() || skill_id.is_empty() {
        return Err(SkillError::InvalidRequest);
    }
    // Path segments only: do not turn spaces into `+` (query encoding).
    Ok(format!(
        "https://www.skills.sh/{}/{}/{}",
        path_encode(owner),
        path_encode(repo),
        path_encode(skill_id)
    ))
}

fn path_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// Build a detail row from one skills.sh skill-page HTML body.
///
/// Requires a usable meta description (same success gate as before). Name,
/// installs, summary, and catalog install hint are best-effort and stay `None`
/// when absent — never invented.
fn parse_skill_page_detail(
    source: &str,
    skill_id: &str,
    html: &str,
) -> Result<MarketplaceSkill, SkillError> {
    let description = parse_skill_page_description(html)?;
    let (json_name, json_installs) = parse_json_ld_software_application(html);
    let name = json_name
        .or_else(|| parse_og_title_name(html))
        .filter(|value| !value.is_empty() && value.chars().count() <= 128)
        .unwrap_or_else(|| skill_id.to_string());
    Ok(MarketplaceSkill {
        id: format!("{source}/{skill_id}"),
        source: source.to_string(),
        skill_id: skill_id.to_string(),
        name,
        installs: json_installs,
        description: Some(description),
        summary: parse_skill_page_summary(html),
        catalog_install_command: parse_catalog_install_command(html),
    })
}

/// Read `<meta name="description">`, then `og:description` fallbacks.
fn parse_skill_page_description(html: &str) -> Result<String, SkillError> {
    let mut name_description = None;
    let mut og_property = None;
    let mut og_name = None;
    let lower = html.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..].find("<meta") {
        let start = search_from + rel;
        let after_meta = start + 5;
        let Some(rel_end) = lower[after_meta..].find('>') else {
            break;
        };
        let end = after_meta + rel_end;
        let tag = &html[start..end];
        search_from = end + 1;
        let attrs = meta_attributes(tag);
        let Some(content) = attrs.content else {
            continue;
        };
        let name = attrs.name.as_deref();
        let property = attrs.property.as_deref();
        if name == Some("description") && name_description.is_none() {
            name_description = Some(content);
        } else if property == Some("og:description") && og_property.is_none() {
            og_property = Some(content);
        } else if name == Some("og:description") && og_name.is_none() {
            og_name = Some(content);
        }
    }
    let raw = name_description
        .or(og_property)
        .or(og_name)
        .ok_or(SkillError::MarketplaceResponseInvalid)?;
    sanitize_marketplace_description(&raw).ok_or(SkillError::MarketplaceResponseInvalid)
}

/// Longer Summary section text between the Summary heading and SKILL.md.
fn parse_skill_page_summary(html: &str) -> Option<String> {
    // Visible HTML first (SSR).
    if let Some(text) = summary_from_visible_html(html) {
        return Some(text);
    }
    // Escaped RSC payload with dangerouslySetInnerHTML.__html.
    summary_from_rsc_payload(html)
}

fn summary_from_visible_html(html: &str) -> Option<String> {
    let marker = ">Summary</div>";
    let start = html.find(marker)?;
    let after = &html[start + marker.len()..];
    let end = after
        .find("<span>SKILL.md")
        .or_else(|| after.find(">SKILL.md</"))
        .or_else(|| after.find(">SKILL.md<"))
        .or_else(|| after.find("SKILL.md"))?;
    // Bound the window so a missing close does not scan the whole document.
    if end > 64 * 1024 {
        return None;
    }
    let chunk = &after[..end];
    let text = html_fragment_to_text(chunk);
    sanitize_marketplace_summary(&text)
}

fn summary_from_rsc_payload(html: &str) -> Option<String> {
    let marker = r#"\"children\":\"Summary\""#;
    let start = html.find(marker)?;
    let after = &html[start..];
    let html_mark = r#"\"__html\":\""#;
    let html_at = after.find(html_mark)?;
    if html_at > 8 * 1024 {
        return None;
    }
    let rest = &after[html_at + html_mark.len()..];
    let end = rest.find(r#"\""#)?;
    if end > 64 * 1024 {
        return None;
    }
    let escaped = &rest[..end];
    let decoded = decode_json_string_fragment(escaped);
    let text = html_fragment_to_text(&decoded);
    sanitize_marketplace_summary(&text)
}

/// skills.sh catalog install hint: `npx skills add https://github.com/… --skill …`.
///
/// Distinct from vibemate `preview_marketplace_install` (Git HEAD clone command).
fn parse_catalog_install_command(html: &str) -> Option<String> {
    let marker = "npx skills add ";
    let start = html.find(marker)?;
    let rest = &html[start..];
    let end = rest
        .find(|character: char| {
            character == '<'
                || character == '"'
                || character == '\\'
                || character == '\n'
                || character == '\r'
        })
        .unwrap_or(rest.len());
    if end > MAX_CATALOG_INSTALL_COMMAND {
        return None;
    }
    let raw = rest[..end].trim();
    sanitize_catalog_install_command(raw)
}

fn sanitize_catalog_install_command(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_CATALOG_INSTALL_COMMAND {
        return None;
    }
    if trimmed.chars().any(|character| character.is_control()) {
        return None;
    }
    if !trimmed.starts_with("npx skills add https://github.com/") {
        return None;
    }
    if !trimmed.contains(" --skill ") {
        return None;
    }
    // Refuse shell metacharacters so the FE can show this as copy text safely.
    if trimmed.chars().any(|character| {
        matches!(
            character,
            ';' | '|' | '&' | '`' | '$' | '(' | ')' | '<' | '>' | '\n' | '\r'
        )
    }) {
        return None;
    }
    Some(trimmed.to_string())
}

/// Display name from `og:title` (`skill — owner/repo`), before the separator.
fn parse_og_title_name(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..].find("<meta") {
        let start = search_from + rel;
        let after_meta = start + 5;
        let Some(rel_end) = lower[after_meta..].find('>') else {
            break;
        };
        let end = after_meta + rel_end;
        let tag = &html[start..end];
        search_from = end + 1;
        let attrs = meta_attributes(tag);
        let property = attrs.property.as_deref();
        let name = attrs.name.as_deref();
        let is_og_title = property == Some("og:title") || name == Some("og:title");
        if !is_og_title {
            continue;
        }
        let content = attrs.content?;
        let base = og_title_base(&content);
        if base.is_empty() {
            return None;
        }
        return Some(base.to_string());
    }
    None
}

fn og_title_base(content: &str) -> &str {
    for separator in [" — ", " – ", " - "] {
        if let Some((left, _)) = content.split_once(separator) {
            return left.trim();
        }
    }
    content.trim()
}

/// `SoftwareApplication` JSON-LD: real `name` and install `userInteractionCount`.
fn parse_json_ld_software_application(html: &str) -> (Option<String>, Option<u64>) {
    let lower = html.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..].find("<script") {
        let start = search_from + rel;
        let Some(tag_end_rel) = lower[start..].find('>') else {
            break;
        };
        let tag_end = start + tag_end_rel;
        let tag = &lower[start..=tag_end];
        search_from = tag_end + 1;
        if !tag.contains("application/ld+json") {
            continue;
        }
        let Some(close_rel) = lower[search_from..].find("</script>") else {
            break;
        };
        let json = html[search_from..search_from + close_rel].trim();
        search_from += close_rel + "</script>".len();
        let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
            continue;
        };
        if value.get("@type").and_then(|entry| entry.as_str()) != Some("SoftwareApplication") {
            continue;
        }
        let name = value
            .get("name")
            .and_then(|entry| entry.as_str())
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(str::to_string);
        let installs = value
            .pointer("/interactionStatistic/userInteractionCount")
            .and_then(|entry| {
                entry
                    .as_u64()
                    .or_else(|| entry.as_i64().and_then(|n| u64::try_from(n).ok()))
                    .or_else(|| {
                        entry.as_str().and_then(|digits| {
                            digits
                                .chars()
                                .filter(|character| character.is_ascii_digit())
                                .collect::<String>()
                                .parse()
                                .ok()
                        })
                    })
            });
        return (name, installs);
    }
    (None, None)
}

fn html_fragment_to_text(fragment: &str) -> String {
    let mut out = String::with_capacity(fragment.len());
    let mut rest = fragment;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else {
            // Incomplete trailing tag — drop it rather than leaking "<span".
            return decode_basic_entities(&out);
        };
        let tag = &after[..end];
        // Turn block-ish tags into newlines so list items stay readable.
        let name = tag
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(
            name.as_str(),
            "p" | "div" | "li" | "br" | "h1" | "h2" | "h3" | "tr" | "ul" | "ol"
        ) {
            out.push('\n');
        } else {
            out.push(' ');
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    decode_basic_entities(&out)
}

fn decode_json_string_fragment(escaped: &str) -> String {
    // skills.sh RSC uses JSON-style escapes inside the quoted __html value.
    let mut out = String::with_capacity(escaped.len());
    let mut chars = escaped.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('u') => {
                let mut hex = String::new();
                for _ in 0..4 {
                    match chars.next() {
                        Some(digit) => hex.push(digit),
                        None => break,
                    }
                }
                if let Ok(code) = u32::from_str_radix(&hex, 16) {
                    if let Some(decoded) = char::from_u32(code) {
                        out.push(decoded);
                        continue;
                    }
                }
                out.push('u');
                out.push_str(&hex);
            }
            Some(other) => {
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

struct MetaAttrs {
    name: Option<String>,
    property: Option<String>,
    content: Option<String>,
}

fn meta_attributes(tag: &str) -> MetaAttrs {
    let mut attrs = MetaAttrs {
        name: None,
        property: None,
        content: None,
    };
    let mut rest = tag;
    // Skip the "<meta" prefix already included; walk attribute="value" pairs.
    while let Some(next) = next_attr(rest) {
        let (key, value, after) = next;
        rest = after;
        match key.as_str() {
            "name" if attrs.name.is_none() => attrs.name = Some(value.to_ascii_lowercase()),
            "property" if attrs.property.is_none() => {
                attrs.property = Some(value.to_ascii_lowercase())
            }
            "content" if attrs.content.is_none() => {
                attrs.content = Some(decode_basic_entities(&value))
            }
            _ => {}
        }
    }
    attrs
}

fn next_attr(input: &str) -> Option<(String, String, &str)> {
    let trimmed = input.trim_start_matches(|c: char| c.is_whitespace() || c == '/' || c == '<');
    // Find an identifier=
    let mut key_end = 0;
    for (idx, character) in trimmed.char_indices() {
        if character.is_ascii_alphanumeric() || character == '-' || character == ':' {
            key_end = idx + character.len_utf8();
        } else {
            break;
        }
    }
    if key_end == 0 {
        return None;
    }
    let key = trimmed[..key_end].to_ascii_lowercase();
    let after_key = trimmed[key_end..].trim_start();
    if !after_key.starts_with('=') {
        // Boolean attribute or junk; skip this token.
        return next_attr(&trimmed[key_end..]);
    }
    let after_eq = after_key[1..].trim_start();
    let (value, after_value) = parse_attr_value(after_eq)?;
    Some((key, value, after_value))
}

fn parse_attr_value(input: &str) -> Option<(String, &str)> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    if matches!(bytes[0], b'"' | b'\'') {
        let quote = bytes[0] as char;
        let rest = &input[1..];
        let end = rest.find(quote)?;
        Some((rest[..end].to_string(), &rest[end + 1..]))
    } else {
        let end = input
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(input.len());
        Some((input[..end].to_string(), &input[end..]))
    }
}

fn decode_basic_entities(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[..=end];
        if entity == "&amp;" {
            out.push('&');
        } else if entity == "&lt;" {
            out.push('<');
        } else if entity == "&gt;" {
            out.push('>');
        } else if entity == "&quot;" {
            out.push('"');
        } else if entity == "&apos;" || entity == "&#39;" {
            out.push('\'');
        } else if entity == "&nbsp;" {
            out.push(' ');
        } else if let Some(character) = decode_numeric_entity(entity) {
            out.push(character);
        } else {
            out.push_str(entity);
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn decode_numeric_entity(entity: &str) -> Option<char> {
    if entity.starts_with("&#x") || entity.starts_with("&#X") {
        let hex = &entity[3..entity.len() - 1];
        let code = u32::from_str_radix(hex, 16).ok()?;
        return char::from_u32(code);
    }
    if entity.starts_with("&#") {
        let digits = &entity[2..entity.len() - 1];
        let code = digits.parse::<u32>().ok()?;
        return char::from_u32(code);
    }
    None
}

/// Align with library description bounds: 1–1024 chars, no controls.
fn sanitize_marketplace_description(raw: &str) -> Option<String> {
    sanitize_marketplace_text(raw, validation::MAX_DESCRIPTION_LENGTH, false)
}

fn sanitize_marketplace_summary(raw: &str) -> Option<String> {
    sanitize_marketplace_text(raw, MAX_SUMMARY_LENGTH, true)
}

fn sanitize_marketplace_text(raw: &str, max_chars: usize, keep_newlines: bool) -> Option<String> {
    let stripped: String = raw
        .chars()
        .filter(|character| {
            if keep_newlines && (*character == '\n' || *character == '\t') {
                return true;
            }
            !character.is_control()
        })
        .collect();
    let collapsed = if keep_newlines {
        collapse_summary_whitespace(&stripped)
    } else {
        stripped
    };
    let trimmed = collapsed.trim();
    let length = trimmed.chars().count();
    if (1..=max_chars).contains(&length) {
        Some(trimmed.to_string())
    } else if keep_newlines && length > max_chars {
        // Summary may truncate; meta description still rejects over-long values.
        let truncated: String = trimmed.chars().take(max_chars).collect();
        Some(truncated.trim_end().to_string())
    } else {
        None
    }
}

fn collapse_summary_whitespace(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut newline_run = 0usize;
    let mut space_pending = false;
    for character in raw.chars() {
        if character == '\n' {
            space_pending = false;
            newline_run += 1;
            if newline_run <= 2 {
                out.push('\n');
            }
            continue;
        }
        newline_run = 0;
        if character.is_whitespace() {
            space_pending = true;
            continue;
        }
        if space_pending && !out.is_empty() && !out.ends_with('\n') {
            out.push(' ');
        }
        space_pending = false;
        out.push(character);
    }
    out
}
