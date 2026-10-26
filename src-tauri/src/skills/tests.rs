//! Behaviour tests for local skill imports: copying, digests, limits, sources.

use super::manifest::{MANIFEST_FILE_NAME, MAX_CANDIDATES, MAX_FILE_BYTES, MAX_FILES};
use super::repository::lock;
use super::service::{copy, import_one};
use super::validation::cursor_for;
use super::*;
use crate::storage::Storage;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const NOW: i64 = 1_700_000_000_000;

/// Temporary database, library and source directories for one test.
struct Fixture {
    root: PathBuf,
    storage: Storage,
    library: SkillLibrary,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "vibemate-skill-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let storage = Storage::open_in_directory(&root).expect("storage");
        let library = SkillLibrary::new(root.join("skills"));
        Self {
            root,
            storage,
            library,
        }
    }

    /// Create an empty source directory outside the library.
    fn source(&self, name: &str) -> PathBuf {
        let directory = self.root.join("sources").join(name);
        fs::create_dir_all(&directory).expect("source directory");
        directory
    }

    /// Import one discovered skill directory, as a folder import does.
    fn import(&self, path: &Path) -> Result<SkillRecord, SkillError> {
        import_one(&self.library, &self.storage, path, NOW)
    }

    /// Choose a folder the way the picker does: the folder or its skill folders.
    fn import_folder(&self, path: &Path) -> Result<ImportFolderReport, SkillError> {
        import_folder(
            &self.library,
            &self.storage,
            ImportFolderRequest {
                path: path.to_string_lossy().into_owned(),
            },
            NOW,
        )
    }

    /// Import a one-file skill source and return its record.
    fn import_named(&self, directory_name: &str, skill_name: &str) -> SkillRecord {
        let source = self.source(directory_name);
        write_manifest(&source, &valid_manifest(skill_name, "A test skill."));
        self.import(&source).expect("import")
    }

    fn set_source(
        &self,
        id: &str,
        expected_revision: i64,
        url: Option<&str>,
        revision: Option<&str>,
    ) -> Result<SkillRecord, SkillError> {
        set_skill_source(
            &self.storage,
            SetSkillSourceRequest {
                id: id.to_string(),
                expected_revision,
                url: url.map(str::to_string),
                revision: revision.map(str::to_string),
                fetch_method: None,
                installer_version: None,
            },
            NOW,
        )
    }

    /// Delete one skill at the revision the user reviewed.
    fn delete(&self, id: &str, expected_revision: i64) -> Result<DeletedSkill, SkillError> {
        delete_skill(
            &self.library,
            &self.storage,
            DeleteSkillRequest {
                id: id.to_string(),
                expected_revision,
            },
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write_manifest(directory: &Path, text: &str) {
    fs::write(directory.join(MANIFEST_FILE_NAME), text).expect("manifest");
}

fn valid_manifest(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n")
}

/// Relative paths and sizes, used to prove a directory was not modified.
fn tree_snapshot(root: &Path) -> Vec<(String, u64)> {
    fn collect(directory: &Path, prefix: &str, entries: &mut Vec<(String, u64)>) {
        for entry in fs::read_dir(directory).expect("read directory") {
            let entry = entry.expect("entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let metadata = fs::symlink_metadata(entry.path()).expect("metadata");
            if metadata.is_dir() {
                collect(&entry.path(), &relative, entries);
            } else {
                entries.push((relative, metadata.len()));
            }
        }
    }
    let mut entries = Vec::new();
    collect(root, "", &mut entries);
    entries.sort();
    entries
}

fn names(page: &SkillPage) -> Vec<&str> {
    page.items.iter().map(|row| row.name.as_str()).collect()
}

#[test]
fn import_copies_content_and_leaves_the_source_unchanged() {
    let fixture = Fixture::new();
    let source = fixture.source("demo");
    let manifest = valid_manifest("demo-skill", "Explains the demo skill.");
    write_manifest(&source, &manifest);
    fs::create_dir(source.join("references")).expect("nested directory");
    fs::write(source.join("references/notes.md"), b"notes").expect("nested file");
    let before = tree_snapshot(&source);

    let record = fixture.import(&source).expect("import");
    assert_eq!(record.name, "demo-skill");
    assert_eq!(record.description, "Explains the demo skill.");
    assert_eq!(record.file_count, 2);
    assert_eq!(record.byte_size, manifest.len() as i64 + 5);
    assert_eq!(record.content_digest.len(), 64);
    assert!(
        record
            .content_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
    assert_eq!(record.revision, 1);
    assert_eq!(record.imported_at_ms, NOW);
    assert_eq!(record.updated_at_ms, NOW);
    assert_eq!(record.source_url, None);
    assert_eq!(record.source_revision, None);
    let canonical = fs::canonicalize(&source).expect("canonical source");
    assert_eq!(record.origin_path.as_deref(), canonical.to_str());

    // The managed copy holds the same bytes and sits under the record ID.
    let managed = fixture.library.root().join(&record.id);
    assert_eq!(
        fs::read(managed.join(MANIFEST_FILE_NAME)).expect("copied manifest"),
        manifest.as_bytes()
    );
    assert_eq!(
        fs::read(managed.join("references/notes.md")).expect("copied nested file"),
        b"notes"
    );
    // Reading the source must not have changed it, and no temporary directory stays.
    assert_eq!(tree_snapshot(&source), before);
    let leftovers = fs::read_dir(fixture.library.root())
        .expect("library root")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".import-"))
        .count();
    assert_eq!(leftovers, 0);

    // The name is unique inside the library, so a second import is refused.
    assert_eq!(fixture.import(&source).unwrap_err(), SkillError::NameTaken);
}

#[test]
fn import_digest_is_deterministic_and_content_dependent() {
    let fixture = Fixture::new();
    let first = fixture.source("first");
    write_manifest(&first, &valid_manifest("first-skill", "First skill."));
    let second = fixture.source("second");
    write_manifest(&second, &valid_manifest("second-skill", "Second skill."));

    let first_record = fixture.import(&first).expect("first import");
    let second_record = fixture.import(&second).expect("second import");
    // Different names and descriptions must not share a digest.
    assert_ne!(first_record.content_digest, second_record.content_digest);

    // The same bytes hash to the same digest on a repeated copy.
    let plan = super::manifest::inspect(&first).expect("inspect");
    let target = fixture.root.join("recheck");
    fs::create_dir(&target).expect("target");
    assert_eq!(
        copy(&first, &plan, &target).expect("copy"),
        first_record.content_digest
    );
}

#[test]
fn import_rejects_paths_that_cannot_be_sources() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture
            .import_folder(Path::new("relative/directory"))
            .unwrap_err(),
        SkillError::SourcePathInvalid
    );
    assert_eq!(
        fixture.import_folder(Path::new("")).unwrap_err(),
        SkillError::SourcePathInvalid
    );
    assert_eq!(
        fixture
            .import_folder(&fixture.root.join("missing"))
            .unwrap_err(),
        SkillError::SourceNotFound
    );

    let file = fixture.source("file-source");
    write_manifest(&file, &valid_manifest("file-source", "Not a directory."));
    let file_path = file.join(MANIFEST_FILE_NAME);
    assert_eq!(
        fixture.import_folder(&file_path).unwrap_err(),
        SkillError::SourceNotDirectory
    );

    // The library itself and anything inside it are not import sources.
    fs::create_dir_all(fixture.library.root()).expect("library root");
    assert_eq!(
        fixture.import_folder(fixture.library.root()).unwrap_err(),
        SkillError::SourcePathInvalid
    );
    let inside = fixture.library.root().join("inner");
    fs::create_dir_all(&inside).expect("inner directory");
    write_manifest(&inside, &valid_manifest("inner", "Inside the library."));
    assert_eq!(
        fixture.import_folder(&inside).unwrap_err(),
        SkillError::SourcePathInvalid
    );
}

#[test]
fn folder_import_takes_the_folder_or_its_skill_subfolders() {
    let fixture = Fixture::new();
    let collection = fixture.root.join("collection");
    fs::create_dir_all(&collection).expect("collection");
    // Two skills, one folder without a manifest, one plain file and a hidden folder.
    fs::create_dir_all(collection.join("beta")).expect("beta");
    write_manifest(
        &collection.join("beta"),
        &valid_manifest("beta-skill", "Beta."),
    );
    fs::create_dir_all(collection.join("alpha")).expect("alpha");
    write_manifest(
        &collection.join("alpha"),
        &valid_manifest("alpha-skill", "Alpha."),
    );
    fs::create_dir_all(collection.join("notes")).expect("notes");
    fs::write(collection.join("README.md"), b"not a skill").expect("file");
    fs::create_dir_all(collection.join(".git")).expect(".git");
    write_manifest(&collection.join(".git"), &valid_manifest("git", "Hidden."));

    let report = fixture.import_folder(&collection).expect("batch import");
    assert_eq!(report.skipped.len(), 0);
    let names: Vec<&str> = report
        .imported
        .iter()
        .map(|row| row.name.as_str())
        .collect();
    // Subfolders are imported in name order, and only real skills are taken.
    assert_eq!(names, ["alpha-skill", "beta-skill"]);

    // Choosing one of those skill folders again is reported, not thrown: the
    // library already holds the name, so nothing is copied.
    let repeat = fixture
        .import_folder(&collection.join("alpha"))
        .expect("repeat import");
    assert_eq!(repeat.imported.len(), 0);
    assert_eq!(
        repeat.skipped,
        [SkippedSkill {
            folder: "alpha".to_string(),
            code: SkillError::NameTaken,
        }]
    );

    let other = fixture.root.join("single");
    fs::create_dir_all(&other).expect("single");
    write_manifest(&other, &valid_manifest("single-skill", "Single."));
    let report = fixture.import_folder(&other).expect("single import");
    assert_eq!(report.imported.len(), 1);
    assert_eq!(report.imported[0].name, "single-skill");
    assert_eq!(report.skipped.len(), 0);
}

#[test]
fn folder_import_names_every_refused_folder() {
    let fixture = Fixture::new();
    let collection = fixture.root.join("mixed");
    for directory in ["first", "second", "broken"] {
        fs::create_dir_all(collection.join(directory)).expect("directory");
    }
    write_manifest(
        &collection.join("first"),
        &valid_manifest("shared-name", "Imports fine."),
    );
    // Folders are imported in name order, so the later folder of the same skill
    // name is refused instead of silently replacing the first one.
    write_manifest(
        &collection.join("second"),
        &valid_manifest("shared-name", "Same name."),
    );
    write_manifest(&collection.join("broken"), "no frontmatter here");

    let report = fixture.import_folder(&collection).expect("batch import");
    assert_eq!(report.imported.len(), 1);
    assert_eq!(report.imported[0].name, "shared-name");
    let skipped: Vec<(&str, SkillError)> = report
        .skipped
        .iter()
        .map(|row| (row.folder.as_str(), row.code))
        .collect();
    assert_eq!(
        skipped,
        [
            ("broken", SkillError::ManifestInvalid),
            ("second", SkillError::NameTaken),
        ]
    );
}

#[test]
fn folder_import_reports_a_folder_without_skills() {
    let fixture = Fixture::new();
    let empty = fixture.root.join("nothing");
    fs::create_dir_all(&empty).expect("directory");
    assert_eq!(
        fixture.import_folder(&empty).unwrap_err(),
        SkillError::NoSkillsFound
    );

    // A folder whose subfolders are not skills is refused the same way.
    fs::create_dir_all(empty.join("notes")).expect("notes");
    fs::write(empty.join("notes/readme.md"), b"text").expect("file");
    assert_eq!(
        fixture.import_folder(&empty).unwrap_err(),
        SkillError::NoSkillsFound
    );
}

#[cfg(unix)]
#[test]
fn folder_import_follows_linked_skill_folders() {
    let fixture = Fixture::new();
    let real = fixture.source("linked-target");
    write_manifest(&real, &valid_manifest("linked-skill", "Linked."));
    let collection = fixture.root.join("links");
    fs::create_dir_all(&collection).expect("collection");
    std::os::unix::fs::symlink(&real, collection.join("linked")).expect("symlink");

    let report = fixture.import_folder(&collection).expect("batch import");
    assert_eq!(report.imported.len(), 1);
    // The recorded origin is the real directory, not the link.
    assert_eq!(
        report.imported[0].origin_path.as_deref(),
        fs::canonicalize(&real).expect("canonical").to_str()
    );
}

#[test]
fn folder_import_is_bounded_by_the_candidate_limit() {
    let fixture = Fixture::new();
    let collection = fixture.root.join("many");
    fs::create_dir_all(&collection).expect("collection");
    for index in 0..=MAX_CANDIDATES {
        let directory = collection.join(format!("skill-{index:03}"));
        fs::create_dir_all(&directory).expect("directory");
        write_manifest(
            &directory,
            &valid_manifest(&format!("skill-{index:03}"), "One of many."),
        );
    }
    assert_eq!(
        fixture.import_folder(&collection).unwrap_err(),
        SkillError::SourceTooLarge
    );
}

#[test]
fn import_rejects_invalid_manifests() {
    let fixture = Fixture::new();
    let cases: Vec<(&str, String, SkillError)> = vec![
        (
            "no-frontmatter",
            "# Title\n".to_string(),
            SkillError::ManifestInvalid,
        ),
        (
            "unterminated",
            "---\nname: demo\ndescription: Demo.\n".to_string(),
            SkillError::ManifestInvalid,
        ),
        (
            "broken-yaml",
            "---\nname: [\n---\n".to_string(),
            SkillError::ManifestInvalid,
        ),
        (
            "missing-name",
            "---\ndescription: Demo.\n---\n".to_string(),
            SkillError::ManifestInvalid,
        ),
        (
            "missing-description",
            "---\nname: demo\n---\n".to_string(),
            SkillError::ManifestInvalid,
        ),
        (
            "name-with-space",
            "---\nname: demo skill\ndescription: Demo.\n---\n".to_string(),
            SkillError::ManifestInvalid,
        ),
        (
            "numeric-name",
            "---\nname: 2024\ndescription: Demo.\n---\n".to_string(),
            SkillError::ManifestInvalid,
        ),
        (
            "long-description",
            format!("---\nname: demo\ndescription: {}\n---\n", "x".repeat(1025)),
            SkillError::ManifestInvalid,
        ),
    ];
    for (directory, manifest, expected) in cases {
        let source = fixture.source(directory);
        write_manifest(&source, &manifest);
        assert_eq!(
            fixture.import(&source).unwrap_err(),
            expected,
            "{directory}"
        );
    }

    // A directory without SKILL.md is reported separately from a bad manifest.
    let empty = fixture.source("empty");
    assert_eq!(
        fixture.import(&empty).unwrap_err(),
        SkillError::ManifestMissing
    );
}

#[test]
fn import_reads_quoted_and_folded_frontmatter() {
    let fixture = Fixture::new();
    let source = fixture.source("quoted");
    write_manifest(
        &source,
        "---\nname: \"quoted-skill\"\ndescription: >\n  A folded description\n  across two lines.\nlicense: MIT\nmetadata:\n  version: \"2.0\"\n---\n\nBody\n",
    );
    let record = fixture.import(&source).expect("import");
    assert_eq!(record.name, "quoted-skill");
    assert_eq!(record.description, "A folded description across two lines.");
}

#[test]
fn a_long_skill_document_imports_and_only_the_header_is_bounded() {
    let fixture = Fixture::new();
    // A published SKILL.md document can be far larger than its YAML header; the
    // limit applies to the header that is parsed, not to the whole file.
    let long = fixture.source("long-document");
    let body = "x".repeat(96 * 1024);
    write_manifest(
        &long,
        &format!("---\nname: long-document\ndescription: Long body.\n---\n\n{body}\n"),
    );
    let record = fixture.import(&long).expect("import");
    assert_eq!(record.name, "long-document");
    assert!(record.byte_size > 96 * 1024);

    // A header that itself exceeds the bound is still refused.
    let header = fixture.source("huge-header");
    let extra_keys = "note: value\n".repeat(8 * 1024);
    write_manifest(
        &header,
        &format!("---\nname: huge-header\ndescription: Huge header.\n{extra_keys}---\n"),
    );
    assert_eq!(
        fixture.import(&header).unwrap_err(),
        SkillError::ManifestInvalid
    );
}

#[test]
fn import_skips_git_metadata() {
    let fixture = Fixture::new();
    let source = fixture.source("with-git");
    write_manifest(&source, &valid_manifest("with-git", "Has git metadata."));
    fs::create_dir_all(source.join(".git")).expect(".git directory");
    fs::write(source.join(".git/config"), b"[core]\n").expect(".git file");

    let record = fixture.import(&source).expect("import");
    assert_eq!(record.file_count, 1);
    assert!(
        !fixture
            .library
            .root()
            .join(&record.id)
            .join(".git")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn import_rejects_symbolic_links() {
    let fixture = Fixture::new();
    let source = fixture.source("with-link");
    write_manifest(&source, &valid_manifest("with-link", "Has a link."));
    let outside = fixture.root.join("outside.md");
    fs::write(&outside, b"outside").expect("outside file");
    std::os::unix::fs::symlink(&outside, source.join("link.md")).expect("symlink");

    assert_eq!(
        fixture.import(&source).unwrap_err(),
        SkillError::SourceUnsupportedEntry
    );
}

#[cfg(unix)]
#[test]
fn import_preserves_executable_files() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let source = fixture.source("scripts");
    write_manifest(&source, &valid_manifest("scripts", "Has a script."));
    let script = source.join("run.sh");
    fs::write(&script, b"#!/bin/sh\n").expect("script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("mode");

    let record = fixture.import(&source).expect("import");
    let mode = fs::metadata(fixture.library.root().join(&record.id).join("run.sh"))
        .expect("copied script")
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111);
}

#[test]
fn a_published_skill_with_examples_imports() {
    let fixture = Fixture::new();
    let source = fixture.source("with-examples");
    write_manifest(&source, &valid_manifest("with-examples", "Ships examples."));
    // The shape of a real published skill: hundreds of example files and more
    // than 8 MiB in total. The original limits refused exactly this.
    fs::create_dir_all(source.join("examples")).expect("examples");
    let payload = vec![b'x'; 36 * 1024];
    for index in 0..315 {
        fs::write(
            source.join(format!("examples/example-{index:03}.html")),
            &payload,
        )
        .expect("example file");
    }

    let record = fixture.import(&source).expect("import");
    assert_eq!(record.file_count, 316);
    assert!(record.byte_size > 11 * 1024 * 1024);
    assert_eq!(
        fs::read(
            fixture
                .library
                .root()
                .join(&record.id)
                .join("examples/example-000.html")
        )
        .expect("copied example"),
        payload
    );
}

#[test]
fn import_enforces_size_limits() {
    let fixture = Fixture::new();
    let big = fixture.source("too-big-file");
    write_manifest(&big, &valid_manifest("too-big-file", "Has a large file."));
    fs::write(big.join("big.bin"), vec![0u8; MAX_FILE_BYTES as usize + 1]).expect("large file");
    assert_eq!(
        fixture.import(&big).unwrap_err(),
        SkillError::SourceTooLarge
    );

    let many = fixture.source("too-many-files");
    write_manifest(&many, &valid_manifest("too-many-files", "Has many files."));
    for index in 0..MAX_FILES {
        fs::write(many.join(format!("file-{index}.md")), b"x").expect("file");
    }
    assert_eq!(
        fixture.import(&many).unwrap_err(),
        SkillError::SourceTooLarge
    );
}

#[test]
fn import_stops_when_the_source_changes_during_the_copy() {
    let fixture = Fixture::new();
    let source = fixture.source("changing");
    write_manifest(
        &source,
        &valid_manifest("changing", "Changes while copying."),
    );
    let extra = source.join("data.txt");
    fs::write(&extra, b"one").expect("file");

    let plan = super::manifest::inspect(&source).expect("inspect");
    fs::write(&extra, b"longer content").expect("rewrite");
    let target = fixture.root.join("partial");
    fs::create_dir(&target).expect("target");
    assert_eq!(
        copy(&source, &plan, &target).unwrap_err(),
        SkillError::SourceChanged
    );
}

#[test]
fn source_records_can_be_recorded_and_cleared() {
    let fixture = Fixture::new();
    let record = fixture.import_named("github-skill", "github-skill");

    let recorded = fixture
        .set_source(
            &record.id,
            record.revision,
            Some("https://github.com/example/skills"),
            Some("v1.2.0"),
        )
        .expect("record source");
    assert_eq!(
        recorded.source_url.as_deref(),
        Some("https://github.com/example/skills")
    );
    assert_eq!(recorded.source_revision.as_deref(), Some("v1.2.0"));
    assert_eq!(recorded.revision, 2);
    assert_eq!(recorded.origin_path, record.origin_path);

    // Recording a source does not change the imported content digest.
    assert_eq!(recorded.content_digest, record.content_digest);

    assert_eq!(
        fixture
            .set_source(&record.id, 1, Some("https://example.com/other"), None)
            .unwrap_err(),
        SkillError::RevisionConflict
    );
    assert_eq!(
        fixture
            .set_source(&record.id, 2, None, Some("v2"))
            .unwrap_err(),
        SkillError::InvalidRequest
    );
    assert_eq!(
        fixture
            .set_source(&record.id, 2, Some("ftp://example.com/skills"), None)
            .unwrap_err(),
        SkillError::SourceUrlInvalid
    );
    assert_eq!(
        fixture
            .set_source(
                &record.id,
                2,
                Some("https://user:token@example.com/skills"),
                None
            )
            .unwrap_err(),
        SkillError::SourceUrlInvalid
    );
    assert_eq!(
        fixture
            .set_source(
                &record.id,
                2,
                Some("https://example.com/skills"),
                Some("has space")
            )
            .unwrap_err(),
        SkillError::SourceRevisionInvalid
    );

    let cleared = fixture
        .set_source(&record.id, 2, None, None)
        .expect("clear");
    assert_eq!(cleared.source_url, None);
    assert_eq!(cleared.source_revision, None);
    assert_eq!(cleared.origin_path, record.origin_path);
    assert_eq!(cleared.revision, 3);

    let unknown = "f".repeat(32);
    assert_eq!(
        fixture
            .set_source(&unknown, 1, Some("https://example.com/skills"), None)
            .unwrap_err(),
        SkillError::NotFound
    );
}

#[test]
fn list_skills_pages_in_name_order() {
    let fixture = Fixture::new();
    for name in ["beta", "alpha", "gamma"] {
        fixture.import_named(name, name);
    }

    let first = list_skills(&fixture.storage, None, 2).expect("first page");
    assert_eq!(names(&first), ["alpha", "beta"]);
    // The total describes the whole library, not the page that was read.
    assert_eq!(first.total, 3);
    let cursor = first.next_cursor.clone().expect("cursor");
    assert_eq!(cursor, cursor_for("beta", &first.items[1].id));

    let second = list_skills(&fixture.storage, Some(&cursor), 2).expect("second page");
    assert_eq!(names(&second), ["gamma"]);
    assert_eq!(second.next_cursor, None);
    assert_eq!(second.total, 3);

    let all = list_skills(&fixture.storage, None, 100).expect("all rows");
    assert_eq!(names(&all), ["alpha", "beta", "gamma"]);
    assert_eq!(all.total, 3);

    let empty = Fixture::new();
    assert_eq!(
        list_skills(&empty.storage, None, 20)
            .expect("empty page")
            .total,
        0
    );

    assert_eq!(
        list_skills(&fixture.storage, Some("no-cursor"), 2).unwrap_err(),
        SkillError::InvalidRequest
    );
    assert_eq!(
        list_skills(&fixture.storage, None, 0).unwrap_err(),
        SkillError::InvalidRequest
    );
    assert_eq!(
        list_skills(&fixture.storage, None, 101).unwrap_err(),
        SkillError::InvalidRequest
    );
}

#[test]
fn ipc_payloads_match_the_frontend_contract() {
    // Field names are the interface contract: the UI validates exact keys and
    // rejects a response whose shape differs from these camelCase names.
    let record = SkillRecord {
        id: "0".repeat(32),
        name: "demo-skill".to_string(),
        description: "Explains the demo skill.".to_string(),
        origin_path: Some("/tmp/sources/demo".to_string()),
        source_url: None,
        source_revision: None,
        fetch_method: None,
        installer_version: None,
        installer_command: None,
        remote_revision: None,
        update_check_status: None,
        last_checked_at_ms: None,
        last_check_error: None,
        content_digest: "ab".repeat(32),
        file_count: 2,
        byte_size: 128,
        revision: 1,
        imported_at_ms: NOW,
        updated_at_ms: NOW,
    };
    let json = serde_json::to_value(&record).expect("serialize");
    let object = json.as_object().expect("object");
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "byteSize",
            "contentDigest",
            "description",
            "fetchMethod",
            "fileCount",
            "id",
            "importedAtMs",
            "installerCommand",
            "installerVersion",
            "lastCheckError",
            "lastCheckedAtMs",
            "name",
            "originPath",
            "remoteRevision",
            "revision",
            "sourceRevision",
            "sourceUrl",
            "updateCheckStatus",
            "updatedAtMs",
        ]
    );

    // Requests arrive exactly as the UI builds them.
    let import: ImportFolderRequest =
        serde_json::from_str(r#"{"path":"/tmp/sources/demo"}"#).expect("import request");
    assert_eq!(import.path, "/tmp/sources/demo");
    assert!(
        serde_json::from_str::<ImportFolderRequest>(r#"{"path":"/tmp/x","extra":true}"#).is_err()
    );

    // Picker and report shapes are what the interface validates.
    assert_eq!(
        serde_json::to_value(PickedFolder::Cancelled).expect("cancelled"),
        serde_json::json!({ "kind": "cancelled" })
    );
    assert_eq!(
        serde_json::to_value(PickedFolder::Selected {
            path: "/tmp/sources/demo".to_string()
        })
        .expect("selected"),
        serde_json::json!({ "kind": "selected", "path": "/tmp/sources/demo" })
    );
    assert_eq!(
        serde_json::to_value(SkillPage {
            items: vec![record.clone()],
            next_cursor: None,
            total: 3,
        })
        .expect("page"),
        serde_json::json!({ "items": [record], "nextCursor": null, "total": 3 })
    );
    let report = ImportFolderReport {
        imported: vec![record.clone()],
        skipped: vec![SkippedSkill {
            folder: "broken".to_string(),
            code: SkillError::ManifestInvalid,
        }],
    };
    assert_eq!(
        serde_json::to_value(&report).expect("report"),
        serde_json::json!({
            "imported": [record],
            "skipped": [{ "folder": "broken", "code": "manifest_invalid" }],
        })
    );
    let source: SetSkillSourceRequest = serde_json::from_str(
        r#"{"id":"0123456789abcdef0123456789abcdef","expectedRevision":2,"url":null,"revision":null}"#,
    )
    .expect("source request");
    assert_eq!(source.expected_revision, 2);
    let git_install: SkillInstallSource = serde_json::from_str(
        r#"{"method":"git","source":{"repository":"example/skills","revision":"v1","skill":null}}"#,
    )
    .expect("git install request");
    assert_eq!(
        git_install,
        SkillInstallSource::Git(GitInstallSource {
            repository: "example/skills".to_string(),
            revision: "v1".to_string(),
            skill: None,
        })
    );
    let npx_install: SkillInstallSource = serde_json::from_str(
        r#"{"method":"npx","source":{"source":"example/skills","installerVersion":null,"skill":"demo"}}"#,
    )
    .expect("npx install request");
    assert_eq!(
        npx_install,
        SkillInstallSource::Npx(NpxInstallSource {
            source: "example/skills".to_string(),
            installer_version: None,
            skill: Some("demo".to_string()),
        })
    );
    assert!(
        serde_json::from_str::<SkillInstallSource>(
            r#"{"method":"git","source":{"repository":"a/b","revision":"v1","skill":null,"extra":true}}"#
        )
        .is_err()
    );
    let delete: DeleteSkillRequest =
        serde_json::from_str(r#"{"id":"0123456789abcdef0123456789abcdef","expectedRevision":3}"#)
            .expect("delete request");
    assert_eq!(delete.expected_revision, 3);
    assert!(
        serde_json::from_str::<DeleteSkillRequest>(
            r#"{"id":"0123456789abcdef0123456789abcdef","expectedRevision":3,"extra":true}"#
        )
        .is_err()
    );
    assert_eq!(
        serde_json::to_value(DeletedSkill {
            id: "0".repeat(32),
            cleanup_pending: true,
        })
        .expect("deleted"),
        serde_json::json!({ "id": "0".repeat(32), "cleanupPending": true })
    );

    // Every error code the interface translates keeps its documented spelling.
    let codes = [
        (SkillError::StorageUnavailable, "storage_unavailable"),
        (SkillError::ReadFailed, "read_failed"),
        (SkillError::WriteFailed, "write_failed"),
        (SkillError::InvalidRequest, "invalid_request"),
        (SkillError::SourcePathInvalid, "source_path_invalid"),
        (SkillError::SourceNotFound, "source_not_found"),
        (SkillError::SourceNotDirectory, "source_not_directory"),
        (
            SkillError::SourceUnsupportedEntry,
            "source_unsupported_entry",
        ),
        (SkillError::SourceTooLarge, "source_too_large"),
        (SkillError::SourceChanged, "source_changed"),
        (SkillError::ManifestMissing, "manifest_missing"),
        (SkillError::ManifestInvalid, "manifest_invalid"),
        (SkillError::NoSkillsFound, "no_skills_found"),
        (SkillError::NameTaken, "name_taken"),
        (SkillError::SourceUrlInvalid, "source_url_invalid"),
        (
            SkillError::SourceRepositoryInvalid,
            "source_repository_invalid",
        ),
        (SkillError::SourceRevisionInvalid, "source_revision_invalid"),
        (SkillError::GitRevisionInvalid, "git_revision_invalid"),
        (
            SkillError::SourceRevisionRequired,
            "source_revision_required",
        ),
        (
            SkillError::InstallerVersionInvalid,
            "installer_version_invalid",
        ),
        (SkillError::InstallerUnavailable, "installer_unavailable"),
        (SkillError::InstallerFailed, "installer_failed"),
        (
            SkillError::InstallerOutputInvalid,
            "installer_output_invalid",
        ),
        (SkillError::InstallTimedOut, "install_timed_out"),
        (
            SkillError::InstallWorkspaceFailed,
            "install_workspace_failed",
        ),
        (
            SkillError::SkillNotFoundInSource,
            "skill_not_found_in_source",
        ),
        (SkillError::NotFound, "not_found"),
        (SkillError::RevisionConflict, "revision_conflict"),
        (SkillError::InvalidStoredSkill, "invalid_stored_skill"),
        (SkillError::OutcomeUnknown, "outcome_unknown"),
        (SkillError::OperationFailed, "operation_failed"),
        (SkillError::SkillInUse, "skill_in_use"),
        (
            SkillError::AgentLocationUnavailable,
            "agent_location_unavailable",
        ),
        (SkillError::TargetConflict, "target_conflict"),
        (SkillError::TargetChanged, "target_changed"),
        (SkillError::SymlinkUnavailable, "symlink_unavailable"),
        (SkillError::DeployModeInvalid, "deploy_mode_invalid"),
        (SkillError::UpdateUnavailable, "update_unavailable"),
        (
            SkillError::MarketplaceUnavailable,
            "marketplace_unavailable",
        ),
        (
            SkillError::MarketplaceResponseInvalid,
            "marketplace_response_invalid",
        ),
        (
            SkillError::MarketplaceSourceUnsupported,
            "marketplace_source_unsupported",
        ),
        (SkillError::MarketplaceTimedOut, "marketplace_timed_out"),
        (SkillError::RevisionCheckFailed, "revision_check_failed"),
        (SkillError::UpdatePlanStale, "update_plan_stale"),
        (SkillError::UpdateStrategyInvalid, "update_strategy_invalid"),
        (SkillError::RecoveryConflict, "recovery_conflict"),
    ];
    for (error, expected) in codes {
        assert_eq!(
            serde_json::to_value(error).expect("serialize error"),
            serde_json::Value::String(expected.to_string())
        );
    }
}

#[test]
fn stored_metadata_is_revalidated_on_read() {
    let fixture = Fixture::new();
    let record = fixture.import_named("stored", "stored-skill");
    {
        let connection = lock(&fixture.storage).expect("connection");
        connection
            .execute("UPDATE skill SET name='Bad Name' WHERE id=?1", [&record.id])
            .expect("corrupt row");
    }
    assert_eq!(
        get_skill(&fixture.storage, &record.id).unwrap_err(),
        SkillError::InvalidStoredSkill
    );
    assert_eq!(
        get_skill(&fixture.storage, &"0".repeat(32)).unwrap_err(),
        SkillError::NotFound
    );
    assert_eq!(
        get_skill(&fixture.storage, "not-an-id").unwrap_err(),
        SkillError::InvalidRequest
    );
}

#[test]
fn delete_removes_the_record_and_copy_but_not_the_source() {
    let fixture = Fixture::new();
    let record = fixture.import_named("demo", "demo-skill");
    let source = fixture.root.join("sources").join("demo");
    let before = tree_snapshot(&source);

    let deleted = fixture.delete(&record.id, record.revision).expect("delete");
    assert_eq!(
        deleted,
        DeletedSkill {
            id: record.id.clone(),
            cleanup_pending: false,
        }
    );

    // The record and vibemate's managed copy are gone…
    assert_eq!(
        get_skill(&fixture.storage, &record.id).unwrap_err(),
        SkillError::NotFound
    );
    assert_eq!(
        list_skills(&fixture.storage, None, 20).expect("page").total,
        0
    );
    // …and a completed deletion leaves nothing parked in the library.
    assert_eq!(
        fs::read_dir(fixture.library.root())
            .expect("library directory")
            .count(),
        0
    );
    // The user's imported folder is never touched.
    assert_eq!(tree_snapshot(&source), before);
}

#[test]
fn delete_refuses_a_stale_revision_and_keeps_the_copy() {
    let fixture = Fixture::new();
    let record = fixture.import_named("demo", "demo-skill");
    // A recorded source edit moves the record on, so the reviewed revision is stale.
    fixture
        .set_source(
            &record.id,
            record.revision,
            Some("https://example.com/skills"),
            None,
        )
        .expect("record source");

    assert_eq!(
        fixture.delete(&record.id, record.revision).unwrap_err(),
        SkillError::RevisionConflict
    );
    let current = get_skill(&fixture.storage, &record.id).expect("still present");
    assert_eq!(current.revision, record.revision + 1);
    // The parked copy was renamed back: one skill, one copy, nothing lost.
    assert!(fixture.library.root().join(&record.id).exists());
    assert_eq!(
        fs::read_dir(fixture.library.root())
            .expect("library directory")
            .count(),
        1
    );
}

#[test]
fn delete_reports_a_missing_skill_and_refuses_invalid_requests() {
    let fixture = Fixture::new();
    let record = fixture.import_named("demo", "demo-skill");
    fixture.delete(&record.id, record.revision).expect("delete");

    assert_eq!(
        fixture.delete(&record.id, record.revision).unwrap_err(),
        SkillError::NotFound
    );
    assert_eq!(
        fixture.delete(&"f".repeat(32), 1).unwrap_err(),
        SkillError::NotFound
    );
    assert_eq!(
        fixture.delete("not-an-id", 1).unwrap_err(),
        SkillError::InvalidRequest
    );
    assert_eq!(
        fixture.delete(&record.id, 0).unwrap_err(),
        SkillError::InvalidRequest
    );
}

#[test]
fn delete_removes_the_record_when_the_managed_copy_is_already_gone() {
    let fixture = Fixture::new();
    let record = fixture.import_named("demo", "demo-skill");
    fs::remove_dir_all(fixture.library.root().join(&record.id)).expect("remove copy");

    // Removing the record is still what the user asked for, with nothing to clean.
    let deleted = fixture.delete(&record.id, record.revision).expect("delete");
    assert!(!deleted.cleanup_pending);
    assert_eq!(
        get_skill(&fixture.storage, &record.id).unwrap_err(),
        SkillError::NotFound
    );
}

#[test]
fn cleanup_takes_only_parked_copies_without_a_record() {
    let fixture = Fixture::new();
    let kept = fixture.import_named("demo", "demo-skill");
    let root = fixture.library.root();
    let orphan_id = "f".repeat(32);
    let blocked_id = "e".repeat(32);

    // An interrupted delete parked this copy before its record was removed.
    fs::rename(
        root.join(&kept.id),
        root.join(format!(".delete-{}", kept.id)),
    )
    .expect("park");
    let orphan = root.join(format!(".delete-{orphan_id}"));
    fs::create_dir(&orphan).expect("orphan");
    fs::write(orphan.join("SKILL.md"), b"leftover").expect("leftover file");
    // A foreign file in the parked namespace cannot be removed and is reported.
    fs::write(root.join(format!(".delete-{blocked_id}")), b"file").expect("blocked entry");

    assert!(!cleanup_deleted_skill_copies(&fixture.library, &fixture.storage).expect("cleanup"));
    // Only the parked copy without a record is removed.
    assert!(!orphan.exists());
    assert!(root.join(format!(".delete-{}", kept.id)).exists());
    assert!(root.join(format!(".delete-{blocked_id}")).exists());

    fs::remove_file(root.join(format!(".delete-{blocked_id}"))).expect("remove blocked");
    // The parked copy of a live record stays recoverable, so cleanup is done.
    assert!(cleanup_deleted_skill_copies(&fixture.library, &fixture.storage).expect("cleanup"));

    // Deleting that record finishes its parked copy as well.
    let deleted = fixture.delete(&kept.id, kept.revision).expect("delete");
    assert!(!deleted.cleanup_pending);
    assert!(!root.join(format!(".delete-{}", kept.id)).exists());
}

#[test]
fn source_fetch_methods_are_recorded_and_checked() {
    let fixture = Fixture::new();
    let record = fixture.import_named("demo", "demo-skill");
    let set = |revision: i64,
               url: Option<&str>,
               revision_text: Option<&str>,
               method: Option<FetchMethod>,
               version: Option<&str>| {
        set_skill_source(
            &fixture.storage,
            SetSkillSourceRequest {
                id: record.id.clone(),
                expected_revision: revision,
                url: url.map(str::to_string),
                revision: revision_text.map(str::to_string),
                fetch_method: method,
                installer_version: version.map(str::to_string),
            },
            NOW,
        )
    };

    let git = set(
        1,
        Some("https://github.com/example/skills"),
        Some("v2"),
        Some(FetchMethod::Git),
        None,
    )
    .expect("git source");
    assert_eq!(git.fetch_method, Some(FetchMethod::Git));
    assert_eq!(git.source_revision.as_deref(), Some("v2"));
    assert_eq!(git.installer_version, None);
    assert_eq!(
        git.installer_command.as_deref(),
        Some(
            "git fetch --depth 1 https://github.com/example/skills v2 && git checkout --detach FETCH_HEAD"
        )
    );

    let npx = set(
        2,
        Some("https://github.com/example/skills"),
        None,
        Some(FetchMethod::Npx),
        None,
    )
    .expect("npx source");
    assert_eq!(npx.fetch_method, Some(FetchMethod::Npx));
    assert_eq!(npx.source_revision, None);
    assert_eq!(npx.installer_version.as_deref(), Some("1.7.4"));
    assert_eq!(
        npx.installer_command.as_deref(),
        Some(
            "npx --yes skills@1.7.4 add https://github.com/example/skills --skill demo-skill --copy -g -a claude-code -y"
        )
    );

    // Each method enforces the fields it needs and refuses fields that do not
    // describe it.
    assert_eq!(
        set(
            3,
            Some("https://github.com/example/skills"),
            None,
            Some(FetchMethod::Git),
            None,
        )
        .unwrap_err(),
        SkillError::SourceRevisionRequired
    );
    assert_eq!(
        set(
            3,
            Some("https://github.com/example/skills"),
            Some("v2"),
            Some(FetchMethod::Npx),
            None,
        )
        .unwrap_err(),
        SkillError::InvalidRequest
    );
    assert_eq!(
        set(
            3,
            Some("https://github.com/example/skills"),
            None,
            Some(FetchMethod::Npx),
            Some("^1.7.4"),
        )
        .unwrap_err(),
        SkillError::InstallerVersionInvalid
    );
    assert_eq!(
        set(
            3,
            Some("https://example.com/skills"),
            None,
            None,
            Some("1.7.4"),
        )
        .unwrap_err(),
        SkillError::InvalidRequest
    );

    let cleared = set(3, None, None, None, None).expect("clear");
    assert_eq!(cleared.source_url, None);
    assert_eq!(cleared.source_revision, None);
    assert_eq!(cleared.fetch_method, None);
    assert_eq!(cleared.installer_version, None);
    assert_eq!(cleared.installer_command, None);
    assert!(cleared.origin_path.is_some());
}
