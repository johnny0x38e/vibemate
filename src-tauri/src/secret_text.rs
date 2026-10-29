//! Masking of credentials embedded in otherwise public strings: URL query
//! parameters and userinfo, and command-line arguments of MCP servers.
//!
//! Key-level masking (`version_text::is_secret_key`) hides whole values such as
//! `env.API_KEY` or `headers.Authorization`. These helpers cover the values
//! that stay visible because the key itself is public (`url`, `base_url`,
//! `args`) but may still carry a secret:
//!
//! - URL query parameters whose name is credential-like (`?api_key=…`,
//!   `?token=…`, `?access_token=…`, `?key=…`, `?sig=…`, `?signature=…`,
//!   `X-Amz-Credential`) and the password in `scheme://user:password@host`;
//! - arguments `--api-key VALUE`, `--token=VALUE`, `-p VALUE` is NOT guessed:
//!   only flags whose name is credential-like are masked;
//! - `Bearer VALUE` (inline or as two arguments), `Name: value` header
//!   arguments with a credential-like name (`Authorization: …`,
//!   `X-API-Key: …`) and `NAME=value` assignments with a credential-like name;
//! - URLs inside arguments, as above.
//!
//! Output is for display, approval digests and history only. It is never
//! written to an Agent file: the writers keep the original values.

use crate::config_apply::version_text::is_secret_key;
use crate::config_plan::SECRET_PLACEHOLDER;
use serde_json::{Map, Value};

/// Whether a URL query parameter name carries a credential.
fn is_secret_query_key(name: &str) -> bool {
    let compact: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect();
    is_secret_key(name)
        || matches!(compact.as_str(), "key" | "sig" | "code")
        || compact.ends_with("signature")
        || compact.ends_with("credential")
}

/// Mask credential query values and a userinfo password in one URL. Text
/// without `?` or userinfo is returned unchanged.
pub(crate) fn mask_url(url: &str) -> String {
    let (head, fragment) = match url.split_once('#') {
        Some((head, fragment)) => (head, Some(fragment)),
        None => (url, None),
    };
    let (base, query) = match head.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (head, None),
    };
    let mut out = mask_userinfo(base);
    if let Some(query) = query {
        out.push('?');
        let pairs: Vec<String> = query
            .split('&')
            .map(|pair| match pair.split_once('=') {
                Some((name, value)) if !value.is_empty() && is_secret_query_key(name) => {
                    format!("{name}={SECRET_PLACEHOLDER}")
                }
                _ => pair.to_owned(),
            })
            .collect();
        out.push_str(&pairs.join("&"));
    }
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

fn mask_userinfo(base: &str) -> String {
    let Some((scheme, rest)) = base.split_once("://") else {
        return base.to_owned();
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, path) = rest.split_at(authority_end);
    match authority.rsplit_once('@') {
        Some((userinfo, host)) => {
            let user = userinfo.split_once(':').map_or(userinfo, |(user, _)| user);
            let masked = if userinfo.contains(':') {
                format!("{user}:{SECRET_PLACEHOLDER}")
            } else {
                // A lone userinfo part is often a token (`https://TOKEN@host`).
                SECRET_PLACEHOLDER.to_owned()
            };
            format!("{scheme}://{masked}@{host}{path}")
        }
        None => base.to_owned(),
    }
}

/// Mask credentials in an argument list (see the module docs for the rules).
pub(crate) fn mask_args<'a>(args: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out = Vec::new();
    let mut mask_next = false;
    for arg in args {
        if mask_next {
            mask_next = false;
            // `--token --other` keeps the following flag visible.
            if !arg.starts_with('-') {
                out.push(SECRET_PLACEHOLDER.to_owned());
                continue;
            }
        }
        out.push(mask_arg(arg, &mut mask_next));
    }
    out
}

fn mask_arg(arg: &str, mask_next: &mut bool) -> String {
    if arg.starts_with('-') && !arg.contains(char::is_whitespace) {
        let flag = arg.trim_start_matches('-');
        return match flag.split_once('=') {
            Some((name, value)) => {
                let prefix = &arg[..arg.len() - value.len()];
                if is_secret_key(name) {
                    format!("{prefix}{SECRET_PLACEHOLDER}")
                } else {
                    format!("{prefix}{}", mask_value(value))
                }
            }
            None => {
                *mask_next = is_secret_key(flag);
                arg.to_owned()
            }
        };
    }
    if arg.eq_ignore_ascii_case("bearer") || arg.eq_ignore_ascii_case("basic") {
        *mask_next = true;
        return arg.to_owned();
    }
    mask_value(arg)
}

/// Mask one free-form value: a header line, an assignment, a bearer token or
/// a URL.
fn mask_value(text: &str) -> String {
    if let Some((name, _)) = text.split_once(':')
        && !name.is_empty()
        && !name.contains(char::is_whitespace)
        && !text[name.len()..].starts_with("://")
        && is_secret_key(name)
    {
        return format!("{name}: {SECRET_PLACEHOLDER}");
    }
    if let Some((name, value)) = text.split_once('=')
        && !value.is_empty()
        && !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
        && is_secret_key(name)
    {
        return format!("{name}={SECRET_PLACEHOLDER}");
    }
    let lower = text.to_ascii_lowercase();
    for scheme in ["bearer ", "basic "] {
        if let Some(index) = lower.find(scheme) {
            let start = index + scheme.len();
            return format!("{}{SECRET_PLACEHOLDER}", &text[..start]);
        }
    }
    if text.contains("://") {
        return text
            .split(' ')
            .map(|word| {
                if word.contains("://") {
                    mask_url(word)
                } else {
                    word.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
    }
    text.to_owned()
}

/// Whether a key holds a URL whose query or userinfo may carry a credential.
pub(crate) fn is_url_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.ends_with("url") || lower.ends_with("uri") || lower == "endpoint"
}

/// Mask URL-valued and `args` fields of one JSON object in place. Nested
/// objects are not visited; callers handle their own nesting.
pub(crate) fn mask_public_fields(map: &mut Map<String, Value>) {
    for (key, value) in map.iter_mut() {
        mask_public_field(key, value);
    }
}

/// Mask one field by its key: a URL string under a URL key, or the string
/// items of an `args` array. Other values are left alone.
pub(crate) fn mask_public_field(key: &str, value: &mut Value) {
    if key == "args"
        && let Some(items) = value.as_array_mut()
    {
        let texts: Vec<Option<String>> = items
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect();
        let masked = mask_args(texts.iter().flatten().map(String::as_str));
        let mut masked = masked.into_iter();
        for (item, text) in items.iter_mut().zip(texts) {
            if text.is_some()
                && let Some(next) = masked.next()
            {
                *item = Value::String(next);
            }
        }
    } else if is_url_key(key)
        && let Some(text) = value.as_str()
    {
        *value = Value::String(mask_url(text));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const P: &str = SECRET_PLACEHOLDER;

    #[test]
    fn url_query_and_userinfo_credentials_are_masked() {
        assert_eq!(
            mask_url("https://h/x?api_key=abc&page=2&token=t#top"),
            format!("https://h/x?api_key={P}&page=2&token={P}#top")
        );
        assert_eq!(
            mask_url("https://h/?access_token=a&key=k&sig=s&X-Amz-Credential=c&max_tokens=9"),
            format!(
                "https://h/?access_token={P}&key={P}&sig={P}&X-Amz-Credential={P}&max_tokens=9"
            )
        );
        assert_eq!(
            mask_url("https://user:pw@h:1/p?q=1"),
            format!("https://user:{P}@h:1/p?q=1")
        );
        assert_eq!(mask_url("https://tok@h/p"), format!("https://{P}@h/p"));
        assert_eq!(mask_url("https://h/p?q=1"), "https://h/p?q=1");
        assert_eq!(mask_url("https://h/p"), "https://h/p");
    }

    #[test]
    fn argument_credentials_are_masked() {
        let args = [
            "--api-key",
            "sk-1",
            "--token=t-2",
            "--header",
            "Authorization: Bearer b-3",
            "-H",
            "X-API-Key: k-4",
            "Bearer",
            "b-5",
            "API_TOKEN=e-6",
            "--url=https://h/?token=u-7",
            "https://h/?key=u-8",
            "--port",
            "8080",
            "--verbose",
            "server.js",
            "--password",
            "--next",
        ];
        assert_eq!(
            mask_args(args),
            [
                "--api-key".to_owned(),
                P.to_owned(),
                format!("--token={P}"),
                "--header".to_owned(),
                format!("Authorization: {P}"),
                "-H".to_owned(),
                format!("X-API-Key: {P}"),
                "Bearer".to_owned(),
                P.to_owned(),
                format!("API_TOKEN={P}"),
                format!("--url=https://h/?token={P}"),
                format!("https://h/?key={P}"),
                "--port".to_owned(),
                "8080".to_owned(),
                "--verbose".to_owned(),
                "server.js".to_owned(),
                "--password".to_owned(),
                "--next".to_owned(),
            ]
        );
        let joined = mask_args(args).join(" ");
        for secret in ["sk-1", "t-2", "b-3", "k-4", "b-5", "e-6", "u-7", "u-8"] {
            assert!(!joined.contains(secret), "{secret} leaked");
        }
    }

    #[test]
    fn object_fields_mask_urls_and_args_only() {
        let mut entry = json!({
            "command": "npx",
            "args": ["-y", "tool", "--token", "s", 3],
            "url": "https://h/?token=s",
            "base_url": "https://h/v1?api_key=s",
            "name": "https://h/?token=visible-name"
        });
        mask_public_fields(entry.as_object_mut().unwrap());
        assert_eq!(entry["args"], json!(["-y", "tool", "--token", P, 3]));
        assert_eq!(entry["url"], format!("https://h/?token={P}"));
        assert_eq!(entry["base_url"], format!("https://h/v1?api_key={P}"));
        assert_eq!(entry["name"], "https://h/?token=visible-name");
    }
}
