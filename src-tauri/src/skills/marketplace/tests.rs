//! Marketplace catalog parsing and GitHub-only install mapping.

use super::{
    filter_github, install_source, is_github_owner_repo, parse_catalog_install_command,
    parse_leaderboard_html, parse_skill_page_description, parse_skill_page_detail,
    parse_skill_page_summary, preview_marketplace_install, sanitize_marketplace_description,
};
use crate::skills::types::{MarketplaceInstallRequest, MarketplaceSkill, SkillError};

#[test]
fn parses_escaped_rsc_leaderboard_payload() {
    let html = r#"
        <script>self.__next_f.push([1,"...\n[{\"source\":\"anthropics/skills\",\"skillId\":\"template-skill\",\"name\":\"template-skill\",\"installs\":238},{\"source\":\"vercel/ai\",\"skillId\":\"ai-sdk\",\"name\":\"ai-sdk\",\"installs\":265},{\"source\":\"open.feishu.cn\",\"skillId\":\"lark-doc\",\"name\":\"lark-doc\",\"installs\":10}]...\n"])</script>
        "#;
    let skills = parse_leaderboard_html(html).expect("parse");
    assert_eq!(skills.len(), 3);
    assert_eq!(skills[0].id, "anthropics/skills/template-skill");
    assert_eq!(skills[0].installs, Some(238));
    let github = filter_github(skills);
    assert_eq!(github.len(), 2);
    assert!(github.iter().all(|row| is_github_owner_repo(&row.source)));
}

#[test]
fn refuses_non_github_marketplace_sources() {
    let error = preview_marketplace_install(MarketplaceInstallRequest {
        source: "open.feishu.cn".to_string(),
        skill_id: "lark-doc".to_string(),
    })
    .expect_err("site source");
    assert_eq!(error, SkillError::MarketplaceSourceUnsupported);

    let preview = preview_marketplace_install(MarketplaceInstallRequest {
        source: "mattpocock/skills".to_string(),
        skill_id: "tdd".to_string(),
    })
    .expect("github source");
    assert_eq!(preview.source_url, "https://github.com/mattpocock/skills");
    assert_eq!(preview.revision.as_deref(), Some("HEAD"));
    assert!(preview.command.contains("git fetch --depth 1"));
    assert!(preview.command.contains("HEAD"));
}

#[test]
fn install_source_requires_skill_id() {
    assert_eq!(
        install_source(&MarketplaceInstallRequest {
            source: "a/b".to_string(),
            skill_id: "  ".to_string(),
        })
        .unwrap_err(),
        SkillError::InvalidRequest
    );
}

#[test]
fn filter_github_deduplicates_ids() {
    let rows = vec![
        MarketplaceSkill {
            id: "a/b/s".to_string(),
            source: "a/b".to_string(),
            skill_id: "s".to_string(),
            name: "s".to_string(),
            installs: Some(1),
            description: None,
            summary: None,
            catalog_install_command: None,
        },
        MarketplaceSkill {
            id: "a/b/s".to_string(),
            source: "a/b".to_string(),
            skill_id: "s".to_string(),
            name: "s".to_string(),
            installs: Some(2),
            description: None,
            summary: None,
            catalog_install_command: None,
        },
    ];
    assert_eq!(filter_github(rows).len(), 1);
}

#[test]
fn marketplace_http_client_builds_with_rustls_provider() {
    // Regression: rustls-no-provider panics on Client::build without a provider,
    // which left the Skills marketplace page stuck on loading.
    super::http_client().expect("marketplace HTTP client");
}

#[test]
fn parses_meta_name_description() {
    let html = r#"<!doctype html><html><head>
        <meta charset="utf-8"/>
        <meta name="description" content="React and Next.js performance guidelines."/>
        <meta property="og:description" content="OG fallback should not win."/>
        </head><body></body></html>"#;
    let description = parse_skill_page_description(html).expect("meta description");
    assert_eq!(description, "React and Next.js performance guidelines.");
}

#[test]
fn falls_back_to_og_description_property() {
    let html = r#"<html><head>
        <meta property="og:description" content="About the skill from Open Graph."/>
        </head></html>"#;
    let description = parse_skill_page_description(html).expect("og description");
    assert_eq!(description, "About the skill from Open Graph.");
}

#[test]
fn falls_back_to_og_description_name() {
    let html = r#"<html><head>
        <meta name="og:description" content="Name-attr og description."/>
        </head></html>"#;
    let description = parse_skill_page_description(html).expect("name og");
    assert_eq!(description, "Name-attr og description.");
}

#[test]
fn meta_content_before_name_still_parses() {
    let html = r#"<meta content="Content comes first." name="description">"#;
    let description = parse_skill_page_description(html).expect("attr order");
    assert_eq!(description, "Content comes first.");
}

#[test]
fn decodes_basic_entities_in_description() {
    let html = r#"<meta name="description" content="A &amp; B &lt;C&gt; &quot;quote&quot;"/>"#;
    let description = parse_skill_page_description(html).expect("entities");
    assert_eq!(description, "A & B <C> \"quote\"");
}

#[test]
fn rejects_missing_or_empty_description_meta() {
    assert_eq!(
        parse_skill_page_description("<html><head></head></html>").unwrap_err(),
        SkillError::MarketplaceResponseInvalid
    );
    assert_eq!(
        parse_skill_page_description(r#"<meta name="description" content="   "/>"#).unwrap_err(),
        SkillError::MarketplaceResponseInvalid
    );
    assert_eq!(
        parse_skill_page_description(&format!(
            r#"<meta name="description" content="{}"/>"#,
            "x".repeat(1025)
        ))
        .unwrap_err(),
        SkillError::MarketplaceResponseInvalid
    );
}

#[test]
fn sanitize_strips_controls_and_bounds_length() {
    assert_eq!(
        sanitize_marketplace_description("  hello\u{0001} world  ").as_deref(),
        Some("hello world")
    );
    assert_eq!(sanitize_marketplace_description(""), None);
    assert_eq!(
        sanitize_marketplace_description(&"y".repeat(1024)).as_deref(),
        Some(&*"y".repeat(1024))
    );
    assert_eq!(sanitize_marketplace_description(&"y".repeat(1025)), None);
}

#[test]
fn parses_enriched_skill_page_detail_fixture() {
    let html = r#"<!doctype html><html><head>
        <meta name="description" content="Test-driven development. Use when the user wants to build features or fix bugs test-first."/>
        <meta property="og:title" content="tdd — mattpocock/skills"/>
        <meta property="og:description" content="OG should not win."/>
        <script type="application/ld+json">{"@context":"https://schema.org","@type":"SoftwareApplication","name":"tdd","description":"Test-driven development.","url":"https://www.skills.sh/mattpocock/skills/tdd","interactionStatistic":{"@type":"InteractionCounter","interactionType":"https://schema.org/InstallAction","userInteractionCount":1161230}}</script>
        </head><body>
        <div class="text-sm font-mono uppercase">Summary</div><div class="mb-8 rounded-lg border"><div class="prose"><p><strong>Test-driven development with vertical slices, behavior-focused tests, and incremental red-green-refactor cycles.</strong></p>
<ul>
<li>Emphasizes integration-style tests that verify behavior through public APIs</li>
<li>Requires vertical slicing instead of horizontal slicing</li>
</ul></div></div><div><span>SKILL.md</span></div>
        <code><span>$</span> npx skills add https://github.com/mattpocock/skills --skill tdd</code>
        </body></html>"#;
    let skill = parse_skill_page_detail("mattpocock/skills", "tdd", html).expect("detail");
    assert_eq!(skill.id, "mattpocock/skills/tdd");
    assert_eq!(skill.name, "tdd");
    assert_eq!(skill.installs, Some(1_161_230));
    assert_eq!(
        skill.description.as_deref(),
        Some("Test-driven development. Use when the user wants to build features or fix bugs test-first.")
    );
    let summary = skill.summary.expect("summary");
    assert!(summary.contains("vertical slices"));
    assert!(summary.contains("Emphasizes integration-style tests"));
    assert_eq!(
        skill.catalog_install_command.as_deref(),
        Some("npx skills add https://github.com/mattpocock/skills --skill tdd")
    );
}

#[test]
fn summary_parser_reads_visible_section_only() {
    // Needs the >Summary</div> marker used by the live page.
    let html = r#"<div class="x">Summary</div><div class="mb-8"><p>Short summary body.</p></div><span>SKILL.md</span>"#;
    let summary = parse_skill_page_summary(html).expect("summary");
    assert_eq!(summary, "Short summary body.");
}

#[test]
fn catalog_install_command_rejects_shell_metacharacters() {
    assert_eq!(
        parse_catalog_install_command(
            r#"npx skills add https://github.com/a/b --skill x; rm -rf /"#
        ),
        None
    );
    assert_eq!(
        parse_catalog_install_command(
            r#"prefix npx skills add https://github.com/a/b --skill x</code>"#
        )
        .as_deref(),
        Some("npx skills add https://github.com/a/b --skill x")
    );
}

#[test]
fn detail_falls_back_to_og_title_and_omits_unknown_installs() {
    let html = r#"<!doctype html><html><head>
        <meta name="description" content="A real meta description for the skill."/>
        <meta property="og:title" content="Fancy Name — owner/repo"/>
        </head><body></body></html>"#;
    let skill = parse_skill_page_detail("owner/repo", "fancy-name", html).expect("detail");
    assert_eq!(skill.name, "Fancy Name");
    assert_eq!(skill.installs, None);
    assert_eq!(skill.summary, None);
    assert_eq!(skill.catalog_install_command, None);
}

#[test]
fn detail_keeps_skill_id_name_when_title_missing() {
    let html = r#"<meta name="description" content="Only a description meta tag is present here."/>"#;
    let skill = parse_skill_page_detail("a/b", "my-skill", html).expect("detail");
    assert_eq!(skill.name, "my-skill");
    assert_eq!(skill.installs, None);
}
