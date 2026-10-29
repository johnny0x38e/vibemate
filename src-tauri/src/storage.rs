//! Private configuration storage backed by SQLite.
//!
//! This module owns the database file, its schema version, and the one connection
//! the app shares. It does not depend on Tauri, so its behavior can be tested with
//! temporary folders. Secrets never belong here: API keys go to the OS credential
//! store, and this database keeps only non-secret settings and credential references.
//!
//! SQLite keeps the whole database in one file (the default rollback journal). That
//! makes later backup and restore steps simpler than with a write-ahead log.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, TransactionBehavior};

/// File name of the database inside the app-data folder.
const DATABASE_FILE_NAME: &str = "vibemate.sqlite3";

/// How long a connection waits for another process's write lock before failing.
///
/// Two app instances can open the same file. Waiting briefly lets the other
/// instance finish its write instead of failing with "database is locked".
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// One ordered schema change.
///
/// `sql` and the version bump run in one transaction. If the SQL fails, SQLite
/// rolls back everything, so the previous schema and its data stay readable.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// Schema version reached after this migration succeeds. Versions start at 1.
    pub version: u32,
    /// SQL statements executed as one batch.
    pub sql: &'static str,
}

/// Schema changes shipped with this build, oldest first.
///
/// Version 1 only marks the database as versioned. Business tables arrive with
/// the task that first needs them; each new migration appends one version.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: "",
    },
    Migration {
        version: 2,
        // A singleton table makes the only current preference explicit instead of
        // introducing an unvalidated key/value settings store before it is needed.
        sql: "CREATE TABLE locale_preference (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            preference TEXT NOT NULL CHECK (preference IN ('system', 'zh-CN', 'en'))
        ) STRICT;",
    },
    Migration {
        version: 3,
        sql: "CREATE TABLE appearance_preference (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            appearance TEXT NOT NULL CHECK (appearance IN ('system', 'light', 'dark')),
            theme TEXT NOT NULL CHECK (theme IN ('forest', 'graphite', 'linen', 'iris', 'ocean'))
        ) STRICT;",
    },
    Migration {
        version: 4,
        // SQLite cannot extend a CHECK constraint in place. Rebuild only this
        // owned table and copy the saved pair inside the migration transaction.
        sql: "CREATE TABLE appearance_preference_v4 (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            appearance TEXT NOT NULL CHECK (appearance IN ('system', 'light', 'dark')),
            theme TEXT NOT NULL CHECK (theme IN ('forest', 'graphite', 'linen', 'iris', 'ocean', 'notion'))
        ) STRICT;
        INSERT INTO appearance_preference_v4 SELECT id, appearance, theme FROM appearance_preference;
        DROP TABLE appearance_preference;
        ALTER TABLE appearance_preference_v4 RENAME TO appearance_preference;",
    },
    Migration {
        version: 5,
        // Non-secret provider instances (see `crate::providers`). The CHECKs repeat
        // the Rust validation so a row written by other means cannot hold an unknown
        // kind or protocol. Which protocols a kind may use is decided in Rust, so it
        // can widen without rebuilding this table. The index serves the stable
        // `ORDER BY created_at, id` list order.
        sql: "CREATE TABLE provider_instance (
            id TEXT PRIMARY KEY CHECK (length(id) = 32 AND id NOT GLOB '*[^0-9a-f]*'),
            kind TEXT NOT NULL CHECK (kind IN ('command-code', 'deepseek', 'openrouter')),
            display_name TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 64),
            base_url TEXT NOT NULL CHECK (length(base_url) BETWEEN 1 AND 2048),
            protocol TEXT NOT NULL CHECK (protocol IN ('chat_completions', 'responses', 'anthropic_messages')),
            revision INTEGER NOT NULL CHECK (revision >= 1),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK (updated_at >= created_at)
        ) STRICT;
        CREATE INDEX provider_instance_list_order ON provider_instance (created_at, id);",
    },
    Migration {
        version: 6,
        // One row per provider whose API key is in the OS credential store. The row
        // holds only the non-secret entry name, never the key. Status reads come from
        // this table, so showing a page never touches the credential store.
        // - The CHECK ties the name to the ID (`provider-<id>`), never to a display name.
        // - Deleting a provider removes its row (`foreign_keys` is on); the
        //   credential entry itself must be deleted by Rust before that.
        // Existing provider rows are left alone. Rows saved before keys existed have
        // no row here and report a missing key. A constraint cannot require a child
        // row, so "every provider has a key" is kept by the create flow instead.
        sql: "CREATE TABLE provider_credential (
            provider_id TEXT PRIMARY KEY
                REFERENCES provider_instance(id) ON DELETE CASCADE,
            credential_ref TEXT NOT NULL UNIQUE
                CHECK (credential_ref = 'provider-' || provider_id),
            updated_at INTEGER NOT NULL
        ) STRICT;",
    },
    Migration {
        version: 7,
        // Models per provider instance (see `crate::models`), fetched from the
        // vendor's list or added by hand, and the outcome of the last fetch.
        // - The primary key is the pair (provider, model ID); `WITHOUT ROWID` stores
        //   rows in that order, which also serves the `ORDER BY model_id` list.
        // - `model_id` keeps the vendor's exact text, including `/`, `.` and `:`.
        // - `selected` is the user's choice; merging fetched lists never changes it.
        // - `last_seen_at` is set when a complete fetch listed the model and
        //   `missing_since` when a later complete fetch no longer did.
        // - The JSON columns hold short string arrays validated in Rust.
        // - Deleting a provider deletes its models and fetch record.
        sql: "CREATE TABLE provider_model (
            provider_id TEXT NOT NULL
                REFERENCES provider_instance(id) ON DELETE CASCADE,
            model_id TEXT NOT NULL CHECK (length(model_id) BETWEEN 1 AND 256),
            source TEXT NOT NULL CHECK (source IN ('fetched', 'manual')),
            selected INTEGER NOT NULL CHECK (selected IN (0, 1)),
            alias TEXT CHECK (alias IS NULL OR length(alias) BETWEEN 1 AND 64),
            upstream_name TEXT CHECK (upstream_name IS NULL OR length(upstream_name) BETWEEN 1 AND 256),
            context_window INTEGER CHECK (context_window IS NULL OR context_window > 0),
            max_output_tokens INTEGER CHECK (max_output_tokens IS NULL OR max_output_tokens > 0),
            input_modalities TEXT CHECK (input_modalities IS NULL OR json_valid(input_modalities)),
            output_modalities TEXT CHECK (output_modalities IS NULL OR json_valid(output_modalities)),
            supported_endpoints TEXT CHECK (supported_endpoints IS NULL OR json_valid(supported_endpoints)),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            last_seen_at INTEGER,
            missing_since INTEGER,
            PRIMARY KEY (provider_id, model_id),
            CHECK (updated_at >= created_at),
            CHECK (missing_since IS NULL OR last_seen_at IS NOT NULL)
        ) STRICT, WITHOUT ROWID;
        CREATE TABLE provider_model_fetch (
            provider_id TEXT PRIMARY KEY
                REFERENCES provider_instance(id) ON DELETE CASCADE,
            fetched_at INTEGER NOT NULL,
            complete INTEGER NOT NULL CHECK (complete IN (0, 1)),
            listed_count INTEGER NOT NULL CHECK (listed_count >= 0)
        ) STRICT;",
    },
    Migration {
        version: 8,
        // Central definitions contain transport metadata only. Every env/header
        // value is an OS entry; old entries are queued in the metadata transaction
        // so failed post-commit cleanup can be retried without exposing values.
        sql: "CREATE TABLE mcp_definition (
            id TEXT PRIMARY KEY CHECK(length(id)=32 AND id NOT GLOB '*[^0-9a-f]*'),
            display_name TEXT NOT NULL CHECK(length(display_name) BETWEEN 1 AND 64),
            server_name TEXT NOT NULL CHECK(length(server_name) BETWEEN 1 AND 64),
            namespace TEXT NOT NULL UNIQUE,
            enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
            revision INTEGER NOT NULL CHECK(revision >= 1),
            connection TEXT NOT NULL CHECK(json_valid(connection)),
            created_at INTEGER NOT NULL CHECK(created_at >= 0),
            updated_at INTEGER NOT NULL CHECK(updated_at >= created_at)
        ) STRICT;
        CREATE TABLE mcp_secret (
            definition_id TEXT NOT NULL REFERENCES mcp_definition(id) ON DELETE CASCADE,
            kind TEXT NOT NULL CHECK(kind IN ('env','header')),
            name TEXT NOT NULL,
            credential_ref TEXT NOT NULL UNIQUE,
            PRIMARY KEY(definition_id,kind,name)
        ) STRICT, WITHOUT ROWID;
        CREATE TABLE mcp_cleanup (
            definition_id TEXT NOT NULL,
            credential_ref TEXT PRIMARY KEY
        ) STRICT;",
    },
    Migration {
        version: 9,
        // Same rebuild pattern as v4: extend the theme CHECK for macos/windows.
        sql: "CREATE TABLE appearance_preference_v9 (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            appearance TEXT NOT NULL CHECK (appearance IN ('system', 'light', 'dark')),
            theme TEXT NOT NULL CHECK (theme IN (
                'forest', 'graphite', 'linen', 'iris', 'ocean', 'notion', 'macos', 'windows'
            ))
        ) STRICT;
        INSERT INTO appearance_preference_v9 SELECT id, appearance, theme FROM appearance_preference;
        DROP TABLE appearance_preference;
        ALTER TABLE appearance_preference_v9 RENAME TO appearance_preference;",
    },
    Migration {
        version: 10,
        // Central skill metadata. Content lives in the private `skills/<id>` folder,
        // so the row keeps identity, provenance and the digest baseline without
        // storing file paths of individual files. The last CHECK states that every
        // record explains where the content came from: a local import path or a
        // user-recorded upstream source.
        sql: "CREATE TABLE skill (
            id TEXT PRIMARY KEY CHECK(length(id)=32 AND id NOT GLOB '*[^0-9a-f]*'),
            name TEXT NOT NULL UNIQUE CHECK(length(name) BETWEEN 1 AND 64),
            description TEXT NOT NULL CHECK(length(description) BETWEEN 1 AND 1024),
            origin_path TEXT CHECK(origin_path IS NULL OR length(origin_path) BETWEEN 1 AND 4096),
            source_url TEXT CHECK(source_url IS NULL OR length(source_url) BETWEEN 1 AND 512),
            source_revision TEXT CHECK(source_revision IS NULL OR length(source_revision) BETWEEN 1 AND 128),
            content_digest TEXT NOT NULL CHECK(length(content_digest)=64 AND content_digest NOT GLOB '*[^0-9a-f]*'),
            file_count INTEGER NOT NULL CHECK(file_count BETWEEN 1 AND 512),
            byte_size INTEGER NOT NULL CHECK(byte_size BETWEEN 1 AND 8388608),
            revision INTEGER NOT NULL CHECK(revision >= 1),
            imported_at INTEGER NOT NULL CHECK(imported_at >= 0),
            updated_at INTEGER NOT NULL CHECK(updated_at >= imported_at),
            CHECK(origin_path IS NOT NULL OR source_url IS NOT NULL)
        ) STRICT;",
    },
    Migration {
        version: 11,
        // Rebuild pattern, as in v4 and v9: the per-skill size and file-count
        // bounds are CHECK constraints, so raising them means recreating the
        // table. Published skills with examples and test fixtures exceed the
        // original 8 MiB total, which made them impossible to import.
        sql: "CREATE TABLE skill_v11 (
            id TEXT PRIMARY KEY CHECK(length(id)=32 AND id NOT GLOB '*[^0-9a-f]*'),
            name TEXT NOT NULL UNIQUE CHECK(length(name) BETWEEN 1 AND 64),
            description TEXT NOT NULL CHECK(length(description) BETWEEN 1 AND 1024),
            origin_path TEXT CHECK(origin_path IS NULL OR length(origin_path) BETWEEN 1 AND 4096),
            source_url TEXT CHECK(source_url IS NULL OR length(source_url) BETWEEN 1 AND 512),
            source_revision TEXT CHECK(source_revision IS NULL OR length(source_revision) BETWEEN 1 AND 128),
            content_digest TEXT NOT NULL CHECK(length(content_digest)=64 AND content_digest NOT GLOB '*[^0-9a-f]*'),
            file_count INTEGER NOT NULL CHECK(file_count BETWEEN 1 AND 2048),
            byte_size INTEGER NOT NULL CHECK(byte_size BETWEEN 1 AND 67108864),
            revision INTEGER NOT NULL CHECK(revision >= 1),
            imported_at INTEGER NOT NULL CHECK(imported_at >= 0),
            updated_at INTEGER NOT NULL CHECK(updated_at >= imported_at),
            CHECK(origin_path IS NOT NULL OR source_url IS NOT NULL)
        ) STRICT;
        INSERT INTO skill_v11 SELECT id, name, description, origin_path, source_url,
            source_revision, content_digest, file_count, byte_size, revision,
            imported_at, updated_at FROM skill;
        DROP TABLE skill;
        ALTER TABLE skill_v11 RENAME TO skill;",
    },
    Migration {
        version: 12,
        // Singleton row for the Agent CLI auto-version-probe preference. The
        // value is only informed consent to run `--version` without a per-run
        // click; it never stores a version number. Auto-probe results stay in
        // window memory (see `docs/agents.md`).
        sql: "CREATE TABLE agent_version_preference (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            auto_probe INTEGER NOT NULL CHECK (auto_probe IN (0, 1))
        ) STRICT;",
    },
    Migration {
        version: 13,
        // Capability values carry their own per-field evidence source. Existing
        // saved models begin unknown; provider refreshes can replace that metadata.
        sql: "ALTER TABLE provider_model ADD COLUMN capabilities TEXT NOT NULL
            DEFAULT '{\"thinking\":{\"state\":\"unknown\",\"source\":null},\"imageInput\":{\"state\":\"unknown\",\"source\":null},\"toolCalls\":{\"state\":\"unknown\",\"source\":null}}'
            CHECK(json_valid(capabilities));",
    },
    Migration {
        version: 14,
        // Per-field user capability declarations (manual capability editing).
        // `NULL` (the default for existing rows) means no declaration: the field
        // follows the catalog evidence in `capabilities`. Declarations live in
        // their own column so a catalog refresh can replace `capabilities`
        // without touching what the user stated about this model.
        sql: "ALTER TABLE provider_model ADD COLUMN capability_overrides TEXT
            DEFAULT NULL
            CHECK(capability_overrides IS NULL OR json_valid(capability_overrides));",
    },
    Migration {
        version: 15,
        // The model's stated thinking-effort options (an explicit level list, or
        // every gateway level). NULL means no statement. Context and output
        // limits already have columns; a catalog refresh overwrites any of the
        // three only when the catalog states a value, and a manual save writes
        // the submitted final state, including a clear.
        sql: "ALTER TABLE provider_model ADD COLUMN thinking_effort TEXT
            DEFAULT NULL
            CHECK(thinking_effort IS NULL OR json_valid(thinking_effort));",
    },
    Migration {
        version: 16,
        // Operation log for Agent configuration writes (see `crate::config_apply`).
        // One operation covers the files of a single apply step; every file is a
        // step row recorded before the first replacement, so a crash halfway
        // leaves a `planned` operation whose applied steps are visible after a
        // restart. Rows keep paths, digests and stable error codes only: file
        // content and credential values live in the private backup directory,
        // which a step references by id, path and digest.
        // - `kind` is validated in Rust so new features can reuse the log without
        //   rebuilding this table; `state` is a closed set, as in other modules.
        // - A step that is not applied holds no applied digest, only a failed step
        //   carries an error code, and the three backup columns appear together.
        // - Deleting an operation removes its steps with it.
        sql: "CREATE TABLE config_operation (
            id TEXT PRIMARY KEY CHECK(length(id)=32 AND id NOT GLOB '*[^0-9a-f]*'),
            kind TEXT NOT NULL CHECK(length(kind) BETWEEN 1 AND 64),
            state TEXT NOT NULL CHECK(state IN ('planned','applied','failed','partial','unknown','compensated')),
            created_at INTEGER NOT NULL CHECK(created_at >= 0),
            updated_at INTEGER NOT NULL CHECK(updated_at >= created_at)
        ) STRICT;
        CREATE INDEX config_operation_unfinished ON config_operation (state, created_at, id);
        CREATE TABLE config_operation_step (
            operation_id TEXT NOT NULL REFERENCES config_operation(id) ON DELETE CASCADE,
            ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
            target_path TEXT NOT NULL CHECK(length(target_path) BETWEEN 1 AND 4096),
            expected_digest TEXT CHECK(expected_digest IS NULL OR (
                length(expected_digest)=64 AND expected_digest NOT GLOB '*[^0-9a-f]*')),
            state TEXT NOT NULL CHECK(state IN ('pending','applied','failed')),
            error_code TEXT CHECK(error_code IS NULL OR length(error_code) BETWEEN 1 AND 64),
            backup_id TEXT CHECK(backup_id IS NULL OR length(backup_id) BETWEEN 1 AND 128),
            backup_path TEXT CHECK(backup_path IS NULL OR length(backup_path) BETWEEN 1 AND 4096),
            backup_digest TEXT CHECK(backup_digest IS NULL OR (
                length(backup_digest)=64 AND backup_digest NOT GLOB '*[^0-9a-f]*')),
            applied_digest TEXT CHECK(applied_digest IS NULL OR (
                length(applied_digest)=64 AND applied_digest NOT GLOB '*[^0-9a-f]*')),
            updated_at INTEGER NOT NULL CHECK(updated_at >= 0),
            PRIMARY KEY (operation_id, ordinal),
            CHECK (error_code IS NULL OR state = 'failed'),
            CHECK (applied_digest IS NULL OR state = 'applied'),
            CHECK (backup_id IS NULL OR state = 'applied'),
            CHECK ((backup_id IS NULL AND backup_path IS NULL AND backup_digest IS NULL)
                OR (backup_id IS NOT NULL AND backup_path IS NOT NULL AND backup_digest IS NOT NULL))
        ) STRICT, WITHOUT ROWID;",
    },
    Migration {
        version: 17,
        // Restore bookkeeping for the operation log (P21).
        // - `owner_ref` is the Provider ID an operation wrote configuration for,
        //   optionally absent when an operation writes no credential. The log only
        //   checks the text shape; each feature decides what the value means, so a
        //   later adapter can store its own reference kind without a new column.
        // - `restore_state` records what a rollback did to one applied step:
        //   `restored` (previous content is back, or a created file is gone),
        //   `conflict` (changed after the write, left untouched), `unavailable`
        //   (stored copy missing or damaged), `failed` (write-back incomplete) or
        //   `unknown` (outcome unverifiable). NULL means never attempted. Without
        //   this column a partly rolled back operation could not be retried: the
        //   second attempt would treat already-restored files as conflicts.
        // - `previous_existed` states whether a step found its file already on
        //   disk. A step keeps no copy when it created the file and when the
        //   planned content was already there; a restore that confused the two
        //   would delete a settings file the user already had.
        sql: "ALTER TABLE config_operation ADD COLUMN owner_ref TEXT
            DEFAULT NULL CHECK(owner_ref IS NULL OR (length(owner_ref) BETWEEN 1 AND 64
                AND owner_ref NOT GLOB '*[^ -~]*'));
        ALTER TABLE config_operation_step ADD COLUMN previous_existed INTEGER
            DEFAULT NULL CHECK(previous_existed IS NULL OR
                (state = 'applied' AND previous_existed IN (0, 1)));
        ALTER TABLE config_operation_step ADD COLUMN restore_state TEXT
            DEFAULT NULL CHECK(restore_state IS NULL OR
                (state = 'applied' AND restore_state IN
                    ('restored','conflict','unavailable','failed','unknown')));",
    },
    Migration {
        version: 18,
        // Pi shows provider keys directly. Remember readable aliases separately
        // from central IDs, including former aliases needed after a restore.
        // No FK: an external Agent configuration can outlive a central record.
        sql: "CREATE TABLE pi_provider_binding (
            config_root TEXT NOT NULL,
            provider_key TEXT NOT NULL CHECK(length(provider_key) BETWEEN 1 AND 160),
            provider_id TEXT NOT NULL CHECK(length(provider_id) = 32 AND provider_id NOT GLOB '*[^0-9a-f]*'),
            PRIMARY KEY(config_root, provider_key)
        ) STRICT, WITHOUT ROWID;",
    },
    Migration {
        version: 19,
        // Grok identifies a model by its TOML table alias, and the same Provider
        // can own several aliases. Record which central pair each alias belongs
        // to so a preview never rewrites an entry the maintainer wrote by hand.
        // No FK: an external Agent configuration can outlive a central record.
        sql: "CREATE TABLE grok_model_binding (
            config_root TEXT NOT NULL,
            model_alias TEXT NOT NULL CHECK(length(model_alias) BETWEEN 1 AND 160),
            provider_id TEXT NOT NULL CHECK(length(provider_id) = 32 AND provider_id NOT GLOB '*[^0-9a-f]*'),
            model_id TEXT NOT NULL CHECK(length(model_id) BETWEEN 1 AND 160),
            PRIMARY KEY(config_root, model_alias)
        ) STRICT, WITHOUT ROWID;",
    },
    Migration {
        version: 20,
        // Pre-mutation snapshots are independent of the old compensation queue.
        // A prepared row survives an interrupted write without inviting P34 to
        // undo a user-requested restore. File content stays in private backups.
        sql: "CREATE TABLE config_file_revision (
            id TEXT PRIMARY KEY CHECK(length(id) BETWEEN 1 AND 128),
            batch_id TEXT NOT NULL CHECK(length(batch_id) BETWEEN 1 AND 128),
            operation_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL,
            intent TEXT NOT NULL CHECK(intent IN ('apply','restore','compensation')),
            target_path TEXT NOT NULL CHECK(length(target_path) BETWEEN 1 AND 4096),
            previous_digest TEXT CHECK(previous_digest IS NULL OR (
                length(previous_digest)=64 AND previous_digest NOT GLOB '*[^0-9a-f]*')),
            next_digest TEXT CHECK(next_digest IS NULL OR (
                length(next_digest)=64 AND next_digest NOT GLOB '*[^0-9a-f]*')),
            backup_path TEXT CHECK(backup_path IS NULL OR length(backup_path) BETWEEN 1 AND 4096),
            state TEXT NOT NULL CHECK(state IN ('prepared','completed','failed','unknown')),
            error_code TEXT CHECK(error_code IS NULL OR length(error_code) BETWEEN 1 AND 64),
            created_at INTEGER NOT NULL CHECK(created_at >= 0),
            updated_at INTEGER NOT NULL CHECK(updated_at >= created_at),
            FOREIGN KEY(operation_id, ordinal)
                REFERENCES config_operation_step(operation_id, ordinal) ON DELETE RESTRICT,
            CHECK ((previous_digest IS NULL AND backup_path IS NULL)
                OR (previous_digest IS NOT NULL AND backup_path IS NOT NULL))
        ) STRICT;
        CREATE INDEX config_file_revision_source
            ON config_file_revision(operation_id, ordinal, created_at, id);
        CREATE INDEX config_file_revision_batch ON config_file_revision(batch_id);",
    },
    Migration {
        version: 21,
        // A user-approved observation is not proof of the original write's
        // execution. Preserve that distinction alongside the repaired log.
        sql: "CREATE TABLE config_revision_resolution (
            revision_id TEXT PRIMARY KEY REFERENCES config_file_revision(id) ON DELETE RESTRICT,
            previous_state TEXT NOT NULL CHECK(previous_state IN ('prepared','unknown','completed')),
            observed_digest TEXT CHECK(observed_digest IS NULL OR (
                length(observed_digest)=64 AND observed_digest NOT GLOB '*[^0-9a-f]*')),
            matched_state TEXT NOT NULL CHECK(matched_state IN ('previous','intended')),
            resolved_at INTEGER NOT NULL CHECK(resolved_at >= 0)
        ) STRICT;",
    },
    Migration {
        version: 22,
        // H2 restores any retained version directly. It is stored as a restore
        // revision because it changes a file the same way, but `direct = 1`
        // says it is not the legacy rollback of the source operation: it must
        // not update the source step's restore bookkeeping, and a completed row
        // must not look like a missing legacy step result. Interrupted direct
        // rows still block recovery until the user inspects them.
        sql: "ALTER TABLE config_file_revision ADD COLUMN direct INTEGER NOT NULL
            DEFAULT 0 CHECK(direct IN (0, 1));
        CREATE INDEX config_file_revision_direct
            ON config_file_revision(operation_id, ordinal, direct);",
    },
    Migration {
        version: 23,
        // H3 user-requested history deletion is not atomic across SQLite and the
        // private backup store. A durable delete row records prepared / failed /
        // completed so an interrupted attempt can retry without first hiding the
        // journal while secret-bearing copies remain unmanaged. Targets list only
        // backup identities this entry exclusively owns; shared refs stay.
        sql: "CREATE TABLE config_history_delete (
            id TEXT PRIMARY KEY CHECK(length(id)=32 AND id NOT GLOB '*[^0-9a-f]*'),
            entry_key TEXT NOT NULL CHECK(length(entry_key) BETWEEN 7 AND 160),
            operation_id TEXT NOT NULL CHECK(length(operation_id)=32
                AND operation_id NOT GLOB '*[^0-9a-f]*'),
            scope_kind TEXT NOT NULL CHECK(length(scope_kind) BETWEEN 1 AND 64),
            state TEXT NOT NULL CHECK(state IN ('prepared','completed','failed')),
            leftovers TEXT CHECK(leftovers IS NULL OR json_valid(leftovers)),
            error_code TEXT CHECK(error_code IS NULL OR length(error_code) BETWEEN 1 AND 64),
            created_at INTEGER NOT NULL CHECK(created_at >= 0),
            updated_at INTEGER NOT NULL CHECK(updated_at >= created_at),
            CHECK (
                (state = 'completed' AND leftovers IS NULL AND error_code IS NULL)
                OR (state = 'prepared' AND leftovers IS NULL)
                OR state = 'failed'
            )
        ) STRICT;
        CREATE UNIQUE INDEX config_history_delete_open
            ON config_history_delete(entry_key) WHERE state IN ('prepared', 'failed');
        CREATE TABLE config_history_delete_backup (
            delete_id TEXT NOT NULL REFERENCES config_history_delete(id) ON DELETE CASCADE,
            backup_id TEXT NOT NULL CHECK(length(backup_id) BETWEEN 1 AND 128),
            backup_dir TEXT NOT NULL CHECK(length(backup_dir) BETWEEN 1 AND 4096),
            PRIMARY KEY (delete_id, backup_id)
        ) STRICT, WITHOUT ROWID;",
    },
    Migration {
        version: 24,
        // Grok Build owns `[model.<alias>]` entries, while `[models].default`
        // belongs to the user and Grok's own picker. Remember when vibemate
        // last wrote each alias as write bookkeeping; the active-model summary
        // reads the live default alias instead. Rows migrated from older builds
        // keep 0 because no write time was recorded.
        sql: "ALTER TABLE grok_model_binding ADD COLUMN last_written_at INTEGER NOT NULL
            DEFAULT 0 CHECK(last_written_at >= 0);
        CREATE INDEX grok_model_binding_last_written
            ON grok_model_binding(config_root, last_written_at DESC);",
    },
    Migration {
        version: 25,
        // A configuration version owns complete result copies. It deliberately
        // has no foreign key to another version or to the recovery journal.
        // Legacy bytes are copied by Rust under the configuration lock; SQL
        // cannot verify private files or reconstruct missing content.
        sql: "ALTER TABLE config_history_delete ADD COLUMN version_only INTEGER NOT NULL
            DEFAULT 0 CHECK(version_only IN (0,1));
        CREATE TABLE config_version (
            id TEXT PRIMARY KEY CHECK(length(id)=32 AND id NOT GLOB '*[^0-9a-f]*'),
            state TEXT NOT NULL CHECK(state IN ('legacy','migrating','prepared','available','unavailable','deleted'))
        ) STRICT;
        CREATE TABLE config_version_file (
            version_id TEXT NOT NULL REFERENCES config_version(id) ON DELETE RESTRICT,
            ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 15),
            target_path TEXT NOT NULL CHECK(length(target_path) BETWEEN 1 AND 4096),
            digest TEXT NOT NULL CHECK(length(digest)=64 AND digest NOT GLOB '*[^0-9a-f]*'),
            copy_id TEXT NOT NULL UNIQUE CHECK(length(copy_id) BETWEEN 1 AND 128),
            copy_path TEXT NOT NULL CHECK(length(copy_path) BETWEEN 1 AND 4096),
            PRIMARY KEY(version_id, ordinal),
            UNIQUE(version_id, target_path)
        ) STRICT, WITHOUT ROWID;
        INSERT INTO config_version(id, state) SELECT id, 'legacy' FROM config_operation;",
    },
    Migration {
        version: 26,
        // This is a reusable UI starting choice, not a native model namespace.
        // Retain a row when clearing effort so save-and-clear cannot hide a
        // concurrent edit from a window that still holds the earlier revision.
        sql: "CREATE TABLE claude_effort_preference (
            provider_id TEXT NOT NULL,
            model_id TEXT NOT NULL,
            revision TEXT NOT NULL CHECK(length(revision)=32 AND revision NOT GLOB '*[^0-9a-f]*'),
            effort TEXT CHECK(effort IN ('low','medium','high','xhigh','max')),
            PRIMARY KEY(provider_id, model_id),
            FOREIGN KEY(provider_id, model_id)
                REFERENCES provider_model(provider_id, model_id) ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;",
    },
    Migration {
        version: 27,
        // P26 records how a source is fetched so P29 can decide whether an
        // update is possible. Existing rows keep NULL: a hand-entered address
        // is not an update method. The command is safe display text generated
        // by Rust, never an arbitrary script supplied by the interface.
        sql: "ALTER TABLE skill ADD COLUMN fetch_method TEXT
                CHECK(fetch_method IS NULL OR fetch_method IN ('git','npx'));
            ALTER TABLE skill ADD COLUMN installer_version TEXT
                CHECK(installer_version IS NULL OR length(installer_version) BETWEEN 1 AND 32);
            ALTER TABLE skill ADD COLUMN installer_command TEXT
                CHECK(installer_command IS NULL OR length(installer_command) BETWEEN 1 AND 1024);",
    },
    Migration {
        version: 28,
        // P27: one owned deploy target per Agent config root and skill name.
        // Deleting a central skill refuses while any row remains (RESTRICT).
        // agent_id is the camelCase AgentId wire value; only "pi" is written now.
        sql: "CREATE TABLE skill_deployment (
            skill_id TEXT NOT NULL REFERENCES skill(id) ON DELETE RESTRICT,
            agent_id TEXT NOT NULL CHECK(agent_id IN ('pi')),
            config_root TEXT NOT NULL CHECK(length(config_root) BETWEEN 1 AND 4096),
            target_name TEXT NOT NULL CHECK(length(target_name) BETWEEN 1 AND 64),
            mode TEXT NOT NULL CHECK(mode IN ('symlink','copy')),
            skill_revision INTEGER NOT NULL CHECK(skill_revision >= 1),
            content_digest TEXT NOT NULL
                CHECK(length(content_digest)=64 AND content_digest NOT GLOB '*[^0-9a-f]*'),
            deployed_at INTEGER NOT NULL CHECK(deployed_at >= 0),
            PRIMARY KEY(agent_id, config_root, target_name),
            UNIQUE(skill_id, agent_id, config_root)
        ) STRICT, WITHOUT ROWID;",
    },
    Migration {
        version: 29,
        // P28: allow Grok Build deployments beside Pi. SQLite cannot widen a
        // CHECK in place, so the table is rebuilt and existing Pi rows kept.
        sql: "CREATE TABLE skill_deployment_v29 (
            skill_id TEXT NOT NULL REFERENCES skill(id) ON DELETE RESTRICT,
            agent_id TEXT NOT NULL CHECK(agent_id IN ('pi','grokBuild')),
            config_root TEXT NOT NULL CHECK(length(config_root) BETWEEN 1 AND 4096),
            target_name TEXT NOT NULL CHECK(length(target_name) BETWEEN 1 AND 64),
            mode TEXT NOT NULL CHECK(mode IN ('symlink','copy')),
            skill_revision INTEGER NOT NULL CHECK(skill_revision >= 1),
            content_digest TEXT NOT NULL
                CHECK(length(content_digest)=64 AND content_digest NOT GLOB '*[^0-9a-f]*'),
            deployed_at INTEGER NOT NULL CHECK(deployed_at >= 0),
            PRIMARY KEY(agent_id, config_root, target_name),
            UNIQUE(skill_id, agent_id, config_root)
        ) STRICT, WITHOUT ROWID;
        INSERT INTO skill_deployment_v29
            SELECT skill_id, agent_id, config_root, target_name, mode,
                   skill_revision, content_digest, deployed_at
            FROM skill_deployment;
        DROP TABLE skill_deployment;
        ALTER TABLE skill_deployment_v29 RENAME TO skill_deployment;",
    },
    Migration {
        version: 30,
        // Lightweight Git revision check: ls-remote HEAD vs recorded
        // source_revision. Does not bump skill.revision; apply remains P30.
        sql: "ALTER TABLE skill ADD COLUMN remote_revision TEXT
                CHECK(remote_revision IS NULL OR length(remote_revision) BETWEEN 1 AND 128);
            ALTER TABLE skill ADD COLUMN update_check_status TEXT
                CHECK(update_check_status IS NULL OR update_check_status IN
                    ('up_to_date','update_available','error'));
            ALTER TABLE skill ADD COLUMN last_checked_at INTEGER
                CHECK(last_checked_at IS NULL OR last_checked_at >= 0);
            ALTER TABLE skill ADD COLUMN last_check_error TEXT
                CHECK(last_check_error IS NULL OR length(last_check_error) BETWEEN 1 AND 64);",
    },
    Migration {
        version: 31,
        // Claude Code is the third Skill deployment target. SQLite cannot widen
        // a CHECK in place, so the table is rebuilt and existing Pi/Grok rows
        // are kept.
        sql: "CREATE TABLE skill_deployment_v31 (
            skill_id TEXT NOT NULL REFERENCES skill(id) ON DELETE RESTRICT,
            agent_id TEXT NOT NULL CHECK(agent_id IN ('pi','grokBuild','claudeCode')),
            config_root TEXT NOT NULL CHECK(length(config_root) BETWEEN 1 AND 4096),
            target_name TEXT NOT NULL CHECK(length(target_name) BETWEEN 1 AND 64),
            mode TEXT NOT NULL CHECK(mode IN ('symlink','copy')),
            skill_revision INTEGER NOT NULL CHECK(skill_revision >= 1),
            content_digest TEXT NOT NULL
                CHECK(length(content_digest)=64 AND content_digest NOT GLOB '*[^0-9a-f]*'),
            deployed_at INTEGER NOT NULL CHECK(deployed_at >= 0),
            PRIMARY KEY(agent_id, config_root, target_name),
            UNIQUE(skill_id, agent_id, config_root)
        ) STRICT, WITHOUT ROWID;
        INSERT INTO skill_deployment_v31
            SELECT skill_id, agent_id, config_root, target_name, mode,
                   skill_revision, content_digest, deployed_at
            FROM skill_deployment;
        DROP TABLE skill_deployment;
        ALTER TABLE skill_deployment_v31 RENAME TO skill_deployment;",
    },
    Migration {
        version: 32,
        // Pi MCP owns entries, never whole files. Intents bridge the filesystem
        // and the SQLite commit; history keeps only references and digests.
        //
        // A pre-merge build created these tables as schema 31 and stamped
        // `user_version` 31. Main then used 31 for the Claude Code skill check
        // and moved this DDL to 32. Those databases already have the tables, so
        // a plain CREATE fails and leaves storage closed. They also skipped the
        // Claude Code rebuild, so this step repeats that rebuild (a no-op copy
        // when version 31 already did it) and creates the MCP tables only when
        // they are absent. Existing rows stay in place.
        sql: "CREATE TABLE skill_deployment_v32 (
            skill_id TEXT NOT NULL REFERENCES skill(id) ON DELETE RESTRICT,
            agent_id TEXT NOT NULL CHECK(agent_id IN ('pi','grokBuild','claudeCode')),
            config_root TEXT NOT NULL CHECK(length(config_root) BETWEEN 1 AND 4096),
            target_name TEXT NOT NULL CHECK(length(target_name) BETWEEN 1 AND 64),
            mode TEXT NOT NULL CHECK(mode IN ('symlink','copy')),
            skill_revision INTEGER NOT NULL CHECK(skill_revision >= 1),
            content_digest TEXT NOT NULL
                CHECK(length(content_digest)=64 AND content_digest NOT GLOB '*[^0-9a-f]*'),
            deployed_at INTEGER NOT NULL CHECK(deployed_at >= 0),
            PRIMARY KEY(agent_id, config_root, target_name),
            UNIQUE(skill_id, agent_id, config_root)
        ) STRICT, WITHOUT ROWID;
        INSERT INTO skill_deployment_v32
            SELECT skill_id, agent_id, config_root, target_name, mode,
                   skill_revision, content_digest, deployed_at
            FROM skill_deployment;
        DROP TABLE skill_deployment;
        ALTER TABLE skill_deployment_v32 RENAME TO skill_deployment;
        CREATE TABLE IF NOT EXISTS pi_mcp_deployment (
            config_root TEXT NOT NULL CHECK(length(config_root) BETWEEN 1 AND 4096),
            definition_id TEXT NOT NULL REFERENCES mcp_definition(id) ON DELETE RESTRICT,
            server_name TEXT NOT NULL CHECK(length(server_name) BETWEEN 1 AND 64),
            namespace TEXT NOT NULL CHECK(length(namespace) BETWEEN 1 AND 64),
            definition_revision INTEGER NOT NULL CHECK(definition_revision>=1),
            entry_digest TEXT NOT NULL CHECK(length(entry_digest)=64 AND entry_digest NOT GLOB '*[^0-9a-f]*'),
            generation TEXT NOT NULL CHECK(length(generation)=32 AND generation NOT GLOB '*[^0-9a-f]*'),
            enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
            PRIMARY KEY(config_root,definition_id),
            UNIQUE(config_root,namespace)
        ) STRICT, WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS pi_mcp_change (
            id TEXT PRIMARY KEY CHECK(length(id)=32 AND id NOT GLOB '*[^0-9a-f]*'),
            config_root TEXT NOT NULL CHECK(length(config_root) BETWEEN 1 AND 4096),
            definition_id TEXT NOT NULL CHECK(length(definition_id)=32),
            server_name TEXT NOT NULL CHECK(length(server_name) BETWEEN 1 AND 64),
            definition_revision INTEGER NOT NULL CHECK(definition_revision>=1),
            action TEXT NOT NULL CHECK(action IN ('deploy','enable','disable','revoke','restore')),
            state TEXT NOT NULL CHECK(state IN ('prepared','completed','aborted')),
            before_digest TEXT CHECK(before_digest IS NULL OR length(before_digest)=64),
            intended_digest TEXT CHECK(intended_digest IS NULL OR length(intended_digest)=64),
            before_owner TEXT CHECK(before_owner IS NULL OR json_valid(before_owner)),
            after_owner TEXT CHECK(after_owner IS NULL OR json_valid(after_owner)),
            operation_id TEXT REFERENCES config_operation(id) ON DELETE RESTRICT,
            created_at INTEGER NOT NULL CHECK(created_at>=0)
        ) STRICT;
        CREATE UNIQUE INDEX IF NOT EXISTS pi_mcp_one_pending_root ON pi_mcp_change(config_root) WHERE state='prepared';
        CREATE INDEX IF NOT EXISTS pi_mcp_history ON pi_mcp_change(config_root,definition_id,created_at DESC,id DESC);",
    },
    Migration {
        version: 33,
        // Grok MCP owns exact server names under user-level config.toml. There is
        // no Pi-style hyphen/underscore namespace; server_name is the unique key.
        sql: "CREATE TABLE grok_mcp_deployment (
            config_root TEXT NOT NULL CHECK(length(config_root) BETWEEN 1 AND 4096),
            definition_id TEXT NOT NULL REFERENCES mcp_definition(id) ON DELETE RESTRICT,
            server_name TEXT NOT NULL CHECK(length(server_name) BETWEEN 1 AND 64),
            definition_revision INTEGER NOT NULL CHECK(definition_revision>=1),
            entry_digest TEXT NOT NULL CHECK(length(entry_digest)=64 AND entry_digest NOT GLOB '*[^0-9a-f]*'),
            generation TEXT NOT NULL CHECK(length(generation)=32 AND generation NOT GLOB '*[^0-9a-f]*'),
            enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
            PRIMARY KEY(config_root,definition_id),
            UNIQUE(config_root,server_name)
        ) STRICT, WITHOUT ROWID;
        CREATE TABLE grok_mcp_change (
            id TEXT PRIMARY KEY CHECK(length(id)=32 AND id NOT GLOB '*[^0-9a-f]*'),
            config_root TEXT NOT NULL CHECK(length(config_root) BETWEEN 1 AND 4096),
            definition_id TEXT NOT NULL CHECK(length(definition_id)=32),
            server_name TEXT NOT NULL CHECK(length(server_name) BETWEEN 1 AND 64),
            definition_revision INTEGER NOT NULL CHECK(definition_revision>=1),
            action TEXT NOT NULL CHECK(action IN ('deploy','enable','disable','revoke','restore')),
            state TEXT NOT NULL CHECK(state IN ('prepared','completed','aborted')),
            before_digest TEXT CHECK(before_digest IS NULL OR length(before_digest)=64),
            intended_digest TEXT CHECK(intended_digest IS NULL OR length(intended_digest)=64),
            before_owner TEXT CHECK(before_owner IS NULL OR json_valid(before_owner)),
            after_owner TEXT CHECK(after_owner IS NULL OR json_valid(after_owner)),
            operation_id TEXT REFERENCES config_operation(id) ON DELETE RESTRICT,
            created_at INTEGER NOT NULL CHECK(created_at>=0)
        ) STRICT;
        CREATE UNIQUE INDEX grok_mcp_one_pending_root ON grok_mcp_change(config_root) WHERE state='prepared';
        CREATE INDEX grok_mcp_history ON grok_mcp_change(config_root,definition_id,created_at DESC,id DESC);",
    },
    Migration {
        version: 34,
        // P34: unfinished Skill deploy/disable journal. Rows survive crashes so
        // recovery can classify the live target against ownership without
        // replaying filesystem writes. mode is null only for malformed rows.
        sql: "CREATE TABLE skill_deploy_operation (
            id TEXT PRIMARY KEY CHECK(length(id)=32 AND id NOT GLOB '*[^0-9a-f]*'),
            kind TEXT NOT NULL CHECK(kind IN ('apply','disable')),
            state TEXT NOT NULL CHECK(state IN ('prepared','unknown')),
            skill_id TEXT NOT NULL CHECK(length(skill_id)=32 AND skill_id NOT GLOB '*[^0-9a-f]*'),
            agent_id TEXT NOT NULL CHECK(agent_id IN ('pi','grokBuild','claudeCode')),
            config_root TEXT NOT NULL CHECK(length(config_root) BETWEEN 1 AND 4096),
            target_name TEXT NOT NULL CHECK(length(target_name) BETWEEN 1 AND 64),
            target_path TEXT NOT NULL CHECK(length(target_path) BETWEEN 1 AND 4096),
            mode TEXT CHECK(mode IS NULL OR mode IN ('symlink','copy')),
            skill_revision INTEGER NOT NULL CHECK(skill_revision >= 1),
            content_digest TEXT NOT NULL
                CHECK(length(content_digest)=64 AND content_digest NOT GLOB '*[^0-9a-f]*'),
            created_at INTEGER NOT NULL CHECK(created_at >= 0),
            updated_at INTEGER NOT NULL CHECK(updated_at >= 0)
        ) STRICT;
        CREATE INDEX skill_deploy_operation_skill
            ON skill_deploy_operation(skill_id);
        CREATE INDEX skill_deploy_operation_list
            ON skill_deploy_operation(created_at ASC, id ASC);",
    },
    Migration {
        version: 35,
        // P33.b: Claude Code user-scope MCP entries own exact top-level
        // `mcpServers` names in `<root>/.claude.json`. There is no native
        // per-server enable flag, so ownership has no `enabled` column and the
        // journal never records enable/disable actions.
        sql: "CREATE TABLE claude_mcp_deployment (
            config_root TEXT NOT NULL CHECK(length(config_root) BETWEEN 1 AND 4096),
            definition_id TEXT NOT NULL REFERENCES mcp_definition(id) ON DELETE RESTRICT,
            server_name TEXT NOT NULL CHECK(length(server_name) BETWEEN 1 AND 64),
            definition_revision INTEGER NOT NULL CHECK(definition_revision>=1),
            entry_digest TEXT NOT NULL CHECK(length(entry_digest)=64 AND entry_digest NOT GLOB '*[^0-9a-f]*'),
            generation TEXT NOT NULL CHECK(length(generation)=32 AND generation NOT GLOB '*[^0-9a-f]*'),
            PRIMARY KEY(config_root,definition_id),
            UNIQUE(config_root,server_name)
        ) STRICT, WITHOUT ROWID;
        CREATE TABLE claude_mcp_change (
            id TEXT PRIMARY KEY CHECK(length(id)=32 AND id NOT GLOB '*[^0-9a-f]*'),
            config_root TEXT NOT NULL CHECK(length(config_root) BETWEEN 1 AND 4096),
            definition_id TEXT NOT NULL CHECK(length(definition_id)=32),
            server_name TEXT NOT NULL CHECK(length(server_name) BETWEEN 1 AND 64),
            definition_revision INTEGER NOT NULL CHECK(definition_revision>=1),
            action TEXT NOT NULL CHECK(action IN ('deploy','revoke','restore')),
            state TEXT NOT NULL CHECK(state IN ('prepared','completed','aborted')),
            before_digest TEXT CHECK(before_digest IS NULL OR length(before_digest)=64),
            intended_digest TEXT CHECK(intended_digest IS NULL OR length(intended_digest)=64),
            before_owner TEXT CHECK(before_owner IS NULL OR json_valid(before_owner)),
            after_owner TEXT CHECK(after_owner IS NULL OR json_valid(after_owner)),
            operation_id TEXT REFERENCES config_operation(id) ON DELETE RESTRICT,
            created_at INTEGER NOT NULL CHECK(created_at>=0)
        ) STRICT;
        CREATE UNIQUE INDEX claude_mcp_one_pending_root ON claude_mcp_change(config_root) WHERE state='prepared';
        CREATE INDEX claude_mcp_history ON claude_mcp_change(config_root,definition_id,created_at DESC,id DESC);",
    },
];

/// Errors from opening or migrating the private database.
///
/// The `Display` text is safe to show to users: it never includes file paths, SQL,
/// or secret values. SQLite details remain available through `source()` for logs.
#[derive(Debug)]
pub enum StorageError {
    /// The app-data folder could not be created.
    DataDirectory(std::io::Error),
    /// Tauri could not resolve the app-data folder, so no database path exists.
    LocateDataDirectory,
    /// SQLite could not open the file or read its schema version.
    Database(rusqlite::Error),
    /// A migration failed and rolled back; the database stays at the previous version.
    MigrationFailed {
        version: u32,
        source: rusqlite::Error,
    },
    /// The file was written by a newer build, so this build must not modify it.
    NewerSchema { found: u32, supported: u32 },
    /// Another operation panicked while holding the connection.
    Unavailable,
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataDirectory(_) => {
                formatter.write_str("vibemate could not create its private data folder.")
            }
            Self::LocateDataDirectory => formatter
                .write_str("vibemate could not find its private data folder on this device."),
            Self::Database(_) => {
                formatter.write_str("vibemate could not open its private configuration database.")
            }
            Self::MigrationFailed { version, .. } => write!(
                formatter,
                "vibemate could not upgrade its configuration database to schema version {version}. The existing data was left unchanged."
            ),
            Self::NewerSchema { found, supported } => write!(
                formatter,
                "The configuration database uses schema version {found}, but this vibemate build supports up to version {supported}. Install a newer vibemate build."
            ),
            Self::Unavailable => formatter.write_str(
                "The configuration database is unavailable after an internal error. Restart vibemate.",
            ),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DataDirectory(error) => Some(error),
            Self::Database(error) | Self::MigrationFailed { source: error, .. } => Some(error),
            Self::LocateDataDirectory | Self::NewerSchema { .. } | Self::Unavailable => None,
        }
    }
}

/// The app's private SQLite database.
///
/// Access is serialized by one `Mutex<Connection>`. Each method locks it for a
/// short operation, so commands on different threads never use the connection at
/// the same time. SQLite also serializes writes between processes, and
/// `BUSY_TIMEOUT` lets a second app instance wait for its turn.
pub struct Storage {
    connection: Mutex<Connection>,
    configuration_lock_path: PathBuf,
}

impl Storage {
    /// Open the database in `directory`, creating the folder and file when needed.
    ///
    /// Migrations finish before this returns, so the caller receives either a
    /// database at the current schema version or an error. Only the given folder
    /// is touched; no project folder or credential store is used.
    ///
    /// # Errors
    ///
    /// Returns `DataDirectory` if the folder cannot be created, `Database` if SQLite
    /// cannot open the file, `NewerSchema` if the file came from a newer build, and
    /// `MigrationFailed` if an upgrade rolled back.
    pub fn open_in_directory(directory: &Path) -> Result<Self, StorageError> {
        fs::create_dir_all(directory).map_err(StorageError::DataDirectory)?;
        Self::open_file(&directory.join(DATABASE_FILE_NAME), MIGRATIONS)
    }

    /// Return the schema version currently recorded in the database.
    ///
    /// # Errors
    ///
    /// Returns `Unavailable` after an earlier panic, or `Database` if SQLite cannot
    /// read the version.
    pub fn schema_version(&self) -> Result<u32, StorageError> {
        let connection = self.lock()?;
        read_schema_version(&connection)
    }

    /// Open `path` and bring it up to `migrations`.
    ///
    /// Tests pass their own migrations to exercise failure paths; the app always
    /// passes `MIGRATIONS`.
    fn open_file(path: &Path, migrations: &[Migration]) -> Result<Self, StorageError> {
        let mut connection = Connection::open(path).map_err(StorageError::Database)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(StorageError::Database)?;
        // SQLite ignores foreign keys unless every connection turns enforcement on.
        // This must happen before migrations, because it cannot change mid-transaction.
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(StorageError::Database)?;
        apply_migrations(&mut connection, migrations)?;
        Ok(Self {
            connection: Mutex::new(connection),
            configuration_lock_path: path.with_extension("config-lock"),
        })
    }

    /// Serialize configuration mutations across threads and app instances.
    ///
    /// Opens a persistent companion file beside SQLite, creating it privately
    /// on Unix. The returned handle owns the exclusive OS lock until dropped.
    /// The file must never be unlinked during normal use: replacing its inode
    /// would let another instance acquire a different lock at the same path.
    /// Always acquire this before the database mutex; it is not reentrant.
    /// This coordinates vibemate, not external Agent/editor writes.
    ///
    /// # Errors
    /// Returns an I/O error when the companion path is linked/non-regular,
    /// unavailable, or locking is unsupported. Callers must refuse mutation.
    pub(crate) fn lock_configuration(&self) -> std::io::Result<fs::File> {
        match fs::symlink_metadata(&self.configuration_lock_path) {
            Ok(metadata) if !metadata.is_file() || metadata.is_symlink() => {
                return Err(std::io::Error::other("Invalid configuration lock file"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&self.configuration_lock_path)?;
        // File::lock uses flock on macOS/Linux and LockFileEx on Windows.
        // Separate handles contend even when two Storage values share a DB.
        file.lock()?;
        Ok(file)
    }

    /// Lock the shared connection for one short operation.
    ///
    /// A poisoned lock means another thread panicked while using the connection.
    /// The app then refuses further database work instead of continuing with an
    /// operation that may have stopped halfway.
    pub(crate) fn lock(&self) -> Result<MutexGuard<'_, Connection>, StorageError> {
        self.connection
            .lock()
            .map_err(|_| StorageError::Unavailable)
    }
}

/// The outcome of opening private storage at startup.
///
/// The app opens even when storage is unavailable. Keeping the failure in managed
/// state lets commands report it, and lets a later backup restore rebuild the
/// database, instead of hiding the whole window. Opening never deletes, renames,
/// or rewrites a file that cannot be read.
pub enum StorageStatus {
    /// The database is open and at the current schema version.
    Ready(Storage),
    /// The database could not be opened or upgraded. The file was left unchanged.
    Unavailable(StorageError),
}

impl StorageStatus {
    /// Open the database in `directory` and keep the outcome instead of returning an error.
    pub fn open_in_directory(directory: &Path) -> Self {
        match Storage::open_in_directory(directory) {
            Ok(storage) => Self::Ready(storage),
            Err(error) => Self::Unavailable(error),
        }
    }

    /// Borrow the open database, or the reason it could not be opened.
    ///
    /// # Errors
    ///
    /// Returns the stored `StorageError` when the database is unavailable.
    pub fn storage(&self) -> Result<&Storage, &StorageError> {
        match self {
            Self::Ready(storage) => Ok(storage),
            Self::Unavailable(error) => Err(error),
        }
    }
}

/// Apply each migration newer than the database's current version, in order.
///
/// The current version comes from SQLite's `user_version` header field. A version
/// above the newest migration in this build means a newer app wrote the file.
/// Refusing to touch it avoids guessing at tables this build does not understand.
///
/// This migrator is intentionally small: the tests cover rollback, refusal of newer
/// files, and concurrent openers. Consider a migration crate only if the project
/// needs down migrations or Rust-side data rewrites.
fn apply_migrations(
    connection: &mut Connection,
    migrations: &[Migration],
) -> Result<(), StorageError> {
    let supported = latest_version(migrations);
    // Check before changing anything, so a newer file is untouched even when no
    // migration is pending.
    let current = read_schema_version(connection)?;
    if current > supported {
        return Err(StorageError::NewerSchema {
            found: current,
            supported,
        });
    }
    // Each step re-reads the version under its own write lock; see `apply_one`.
    for migration in migrations {
        apply_one(connection, *migration, supported)?;
    }
    Ok(())
}

/// Run one migration unless the database already reached its version.
///
/// The version is read inside an immediate transaction. SQLite takes the write lock
/// when such a transaction starts, so a second app instance waits here (see
/// `BUSY_TIMEOUT`) and then sees the upgrade that the first instance committed.
/// Reading the version before taking the lock could let two instances run the same
/// `CREATE TABLE`. A failing `?` drops the transaction, which rolls back this step.
fn apply_one(
    connection: &mut Connection,
    migration: Migration,
    supported: u32,
) -> Result<(), StorageError> {
    let failed = |source: rusqlite::Error| StorageError::MigrationFailed {
        version: migration.version,
        source,
    };
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(StorageError::Database)?;
    let current = read_schema_version(&transaction)?;
    if current > supported {
        return Err(StorageError::NewerSchema {
            found: current,
            supported,
        });
    }
    if current >= migration.version {
        // Another app instance, or an earlier run, already applied this version.
        return Ok(());
    }
    transaction.execute_batch(migration.sql).map_err(failed)?;
    // `user_version` is stored in the database header, so this update commits with
    // the schema change instead of becoming visible on its own.
    transaction
        .pragma_update(None, "user_version", migration.version)
        .map_err(failed)?;
    transaction.commit().map_err(failed)
}

/// Read the schema number from SQLite's `user_version` header field.
fn read_schema_version(connection: &Connection) -> Result<u32, StorageError> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
        .map_err(StorageError::Database)
}

/// The schema version this build migrates to, for tests in other modules that
/// should keep passing when a later migration is added.
#[cfg(test)]
pub(crate) fn latest_schema_version() -> u32 {
    latest_version(MIGRATIONS)
}

/// Return the version reached after all of `migrations` have run (0 when empty).
fn latest_version(migrations: &[Migration]) -> u32 {
    migrations.last().map_or(0, |migration| migration.version)
}

#[cfg(test)]
mod tests;
