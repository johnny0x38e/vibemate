import { invoke, isTauri } from "@tauri-apps/api/core";
import { hasExactKeys, isObject } from "./providers";

/** Safe domain codes; no arbitrary backend message may reach the UI. */
const SKILL_CODES = [
    "storage_unavailable",
    "read_failed",
    "write_failed",
    "invalid_request",
    "source_path_invalid",
    "source_not_found",
    "source_not_directory",
    "source_unsupported_entry",
    "source_too_large",
    "source_changed",
    "manifest_missing",
    "manifest_invalid",
    "no_skills_found",
    "name_taken",
    "source_url_invalid",
    "source_repository_invalid",
    "source_revision_invalid",
    "git_revision_invalid",
    "source_revision_required",
    "installer_version_invalid",
    "installer_unavailable",
    "installer_failed",
    "installer_output_invalid",
    "install_timed_out",
    "install_workspace_failed",
    "skill_not_found_in_source",
    "not_found",
    "revision_conflict",
    "invalid_stored_skill",
    "outcome_unknown",
    "operation_failed",
    "skill_in_use",
    "agent_location_unavailable",
    "target_conflict",
    "target_changed",
    "symlink_unavailable",
    "deploy_mode_invalid",
    "update_unavailable",
    "marketplace_unavailable",
    "marketplace_response_invalid",
    "marketplace_source_unsupported",
    "marketplace_timed_out",
    "revision_check_failed",
    "update_plan_stale",
    "update_strategy_invalid",
    "recovery_conflict",
] as const;
/** Sanitized domain, preview or response-validation failure. */
export type SkillErrorCode =
    (typeof SKILL_CODES)[number] | "desktop_required" | "invalid_response";
/** Safe errors deliberately discard the original payload, including paths. */
export class SkillRequestError extends Error {
    readonly code: SkillErrorCode;
    constructor(code: SkillErrorCode) {
        super("Skill library request failed.");
        this.name = "SkillRequestError";
        this.code = code;
    }
}
/** How a recorded source is fetched for an update check. */
export type SkillFetchMethod = "git" | "npx";
/** One reviewed remote acquisition path. */
export type SkillInstallSource =
    | {
          method: "git";
          source: {
              repository: string;
              revision: string;
              skill: string | null;
          };
      }
    | {
          method: "npx";
          source: {
              source: string;
              /** Null asks Rust for the installer version it reviewed. */
              installerVersion: string | null;
              skill: string | null;
          };
      };
/** What the install dialog shows before the user confirms. */
export interface SkillInstallPreview {
    method: SkillFetchMethod;
    sourceUrl: string;
    revision: string | null;
    installer: string;
    installerVersion: string | null;
    command: string;
}
/** Safe metadata for one imported skill; no file content crosses IPC. */
/** Outcome of the last lightweight Git `ls-remote` check. */
export type SkillRevisionCheckStatus =
    "up_to_date" | "update_available" | "error";
export interface SkillRecord {
    id: string;
    name: string;
    description: string;
    originPath: string | null;
    sourceUrl: string | null;
    sourceRevision: string | null;
    fetchMethod: SkillFetchMethod | null;
    installerVersion: string | null;
    installerCommand: string | null;
    remoteRevision: string | null;
    updateCheckStatus: SkillRevisionCheckStatus | null;
    lastCheckedAtMs: number | null;
    lastCheckError: SkillErrorCode | null;
    contentDigest: string;
    fileCount: number;
    byteSize: number;
    revision: number;
    importedAtMs: number;
    updatedAtMs: number;
}
/** One bounded cursor page plus the library total, or an honest browser preview. */
export type SkillPageResult =
    | { kind: "preview" }
    | {
          kind: "desktop";
          page: {
              items: SkillRecord[];
              nextCursor: string | null;
              total: number;
          };
      };
/** The chosen folder, or the user closing the system dialog. */
export type PickedFolder =
    { kind: "cancelled" } | { kind: "selected"; path: string };
/** One skill folder that could not be imported, with a safe reason code. */
export interface SkippedSkill {
    folder: string;
    code: SkillErrorCode;
}
/** What one chosen folder produced: added skills and refused folders. */
export interface ImportReport {
    imported: SkillRecord[];
    skipped: SkippedSkill[];
}
/** What one reviewed remote install produced. */
export interface SkillInstallReport {
    method: SkillFetchMethod;
    sourceUrl: string;
    revision: string | null;
    installer: string;
    installerVersion: string | null;
    command: string;
    exitStatus: number;
    imported: SkillRecord[];
    skipped: SkippedSkill[];
}
/** skills.sh leaderboard page the catalog dialog can load. */
export type MarketplaceBoard = "all_time" | "trending" | "hot";
/** One GitHub-installable row from the skills.sh catalog. */
export interface MarketplaceSkill {
    id: string;
    source: string;
    skillId: string;
    name: string;
    installs: number;
}
/** Install one marketplace row at Git HEAD. */
export interface MarketplaceInstallRequest {
    source: string;
    skillId: string;
}

/** Skill names follow the library rules: letters, digits, dot, dash, underscore. */
const NAME_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;
const DIGEST_PATTERN = /^[0-9a-f]{64}$/;
const ID_PATTERN = /^[0-9a-f]{32}$/;
const INSTALLER_VERSION_PATTERN = /^\d{1,3}\.\d{1,3}\.\d{1,3}$/;
/** The only npx installer version this build displays or accepts. */
const SKILLS_INSTALLER_VERSION = "1.7.4";
/**
 * Import bounds, mirroring the Rust constants and the `skill` table constraints.
 * A response outside them is rejected: the backend would not have produced it.
 */
const MAX_FILES = 2048;
const MAX_BYTES = 64 * 1024 * 1024;
/** How many skills one chosen folder may contribute. */
const MAX_CANDIDATES = 64;
/** Sanity bound for the reported library total. */
const MAX_TOTAL = 100_000;
/** Catalog search and leaderboard bounds, mirroring the Rust marketplace module. */
const MAX_MARKETPLACE_ROWS = 100;
const MAX_MARKETPLACE_SEARCH = 60;
/** Public GitHub `owner/repo` as skills.sh returns it for installable rows. */
const GITHUB_OWNER_REPO =
    /^[A-Za-z0-9](?:[A-Za-z0-9]|-(?=[A-Za-z0-9])){0,38}\/[A-Za-z0-9._-]{1,100}$/;

function invalid(): never {
    throw new SkillRequestError("invalid_response");
}
/**
 * Whether text carries a control character. Such a value cannot be shown as a
 * path or a folder label, so a response that contains one is rejected instead of
 * being rendered. Written as a code-point loop because a control-character
 * regular expression is not allowed in this project.
 */
function hasControlCharacter(value: string): boolean {
    for (const character of value) {
        const code = character.codePointAt(0) ?? 0;
        if (code < 0x20 || code === 0x7f) return true;
    }
    return false;
}
function object(
    value: unknown,
    keys: readonly string[],
): Record<string, unknown> {
    if (!isObject(value) || !hasExactKeys(value, keys)) invalid();
    return value;
}
function string(value: unknown): string {
    if (typeof value !== "string") invalid();
    return value;
}
function optionalString(value: unknown): string | null {
    return value === null ? null : string(value);
}
function fetchMethod(value: unknown): SkillFetchMethod | null {
    if (value === null) return null;
    if (value === "git" || value === "npx") return value;
    invalid();
}
/** A reviewed command is bounded display text, never a shell script. */
function command(value: string): string {
    if (value === "" || value.length > 1024 || hasControlCharacter(value))
        invalid();
    return value;
}
function integer(value: unknown, minimum: number): number {
    if (
        typeof value !== "number" ||
        !Number.isSafeInteger(value) ||
        value < minimum
    )
        invalid();
    return value;
}
function boolean(value: unknown): boolean {
    if (typeof value !== "boolean") invalid();
    return value;
}
function revisionCheckStatus(value: unknown): SkillRevisionCheckStatus | null {
    if (value === null) return null;
    if (
        value === "up_to_date" ||
        value === "update_available" ||
        value === "error"
    )
        return value;
    invalid();
}

function record(value: unknown): SkillRecord {
    const data = object(value, [
        "id",
        "name",
        "description",
        "originPath",
        "sourceUrl",
        "sourceRevision",
        "fetchMethod",
        "installerVersion",
        "installerCommand",
        "remoteRevision",
        "updateCheckStatus",
        "lastCheckedAtMs",
        "lastCheckError",
        "contentDigest",
        "fileCount",
        "byteSize",
        "revision",
        "importedAtMs",
        "updatedAtMs",
    ]);
    const updateCheckStatus = revisionCheckStatus(data["updateCheckStatus"]);
    const lastCheckError =
        data["lastCheckError"] === null
            ? null
            : (SKILL_CODES.find((code) => code === data["lastCheckError"]) ??
              null);
    if (data["lastCheckError"] !== null && lastCheckError === null) invalid();
    const row: SkillRecord = {
        id: string(data["id"]),
        name: string(data["name"]),
        description: string(data["description"]),
        originPath: optionalString(data["originPath"]),
        sourceUrl: optionalString(data["sourceUrl"]),
        sourceRevision: optionalString(data["sourceRevision"]),
        fetchMethod: fetchMethod(data["fetchMethod"]),
        installerVersion: optionalString(data["installerVersion"]),
        installerCommand: optionalString(data["installerCommand"]),
        remoteRevision: optionalString(data["remoteRevision"]),
        updateCheckStatus,
        lastCheckedAtMs:
            data["lastCheckedAtMs"] === null
                ? null
                : integer(data["lastCheckedAtMs"], 0),
        lastCheckError,
        contentDigest: string(data["contentDigest"]),
        fileCount: integer(data["fileCount"], 1),
        byteSize: integer(data["byteSize"], 1),
        revision: integer(data["revision"], 1),
        importedAtMs: integer(data["importedAtMs"], 0),
        updatedAtMs: integer(data["updatedAtMs"], 0),
    };
    if (
        !ID_PATTERN.test(row.id) ||
        !NAME_PATTERN.test(row.name) ||
        row.description.trim() === "" ||
        row.description.length > 1024 ||
        !DIGEST_PATTERN.test(row.contentDigest) ||
        row.fileCount > MAX_FILES ||
        row.byteSize > MAX_BYTES ||
        row.updatedAtMs < row.importedAtMs ||
        // Every row explains where its content came from.
        (row.originPath === null && row.sourceUrl === null)
    )
        invalid();
    // A revision belongs to an address, and every row explains its origin.
    if (row.sourceRevision !== null && row.sourceUrl === null) invalid();
    // A fetch method and its metadata exist together and only with an address.
    if (row.fetchMethod === null) {
        if (row.installerVersion !== null || row.installerCommand !== null)
            invalid();
    } else if (row.fetchMethod === "git") {
        if (
            row.sourceUrl === null ||
            row.sourceRevision === null ||
            row.installerVersion !== null ||
            row.installerCommand === null
        )
            invalid();
    } else if (
        row.sourceUrl === null ||
        row.sourceRevision !== null ||
        row.installerVersion === null ||
        row.installerCommand === null
    ) {
        invalid();
    }
    if (
        row.installerVersion !== null &&
        !INSTALLER_VERSION_PATTERN.test(row.installerVersion)
    )
        invalid();
    if (row.installerCommand !== null) command(row.installerCommand);
    // Revision-check columns are either all empty or a consistent outcome.
    if (row.updateCheckStatus === null) {
        if (
            row.remoteRevision !== null ||
            row.lastCheckedAtMs !== null ||
            row.lastCheckError !== null
        )
            invalid();
    } else if (
        row.lastCheckedAtMs === null ||
        (row.updateCheckStatus === "error"
            ? row.lastCheckError === null
            : row.remoteRevision === null || row.lastCheckError !== null)
    ) {
        invalid();
    }
    if (
        row.remoteRevision !== null &&
        (row.remoteRevision.trim() === "" ||
            hasControlCharacter(row.remoteRevision))
    )
        invalid();
    return row;
}
/** The cursor carries both ordering parts: skill name, then identity. */
function cursor(value: unknown): { name: string; id: string } {
    const text = string(value);
    const separator = text.indexOf("\n");
    if (separator < 0) invalid();
    const name = text.slice(0, separator);
    const id = text.slice(separator + 1);
    if (!NAME_PATTERN.test(name) || !ID_PATTERN.test(id)) invalid();
    return { name, id };
}
function isAfter(
    row: SkillRecord,
    position: { name: string; id: string },
): boolean {
    return (
        row.name > position.name ||
        (row.name === position.name && row.id > position.id)
    );
}
async function call(
    command: string,
    args: Record<string, unknown>,
): Promise<unknown> {
    try {
        return await invoke<unknown>(command, args);
    } catch (error: unknown) {
        throw new SkillRequestError(
            SKILL_CODES.find((code) => code === error) ?? "operation_failed",
        );
    }
}
function desktop(): void {
    if (!isTauri()) throw new SkillRequestError("desktop_required");
}
/** Read bounded metadata; preview performs no IPC and reads no directory. */
export async function listSkills(
    after: string | null,
    limit = 20,
): Promise<SkillPageResult> {
    if (!isTauri()) return { kind: "preview" };
    const position = after === null ? null : cursor(after);
    const data = object(await call("list_skills", { after, limit }), [
        "items",
        "nextCursor",
        "total",
    ]);
    if (!Array.isArray(data["items"])) invalid();
    const items = data["items"].map((value: unknown) => record(value));
    const nextCursor =
        data["nextCursor"] === null ? null : string(data["nextCursor"]);
    // The total counts the whole library, so it can never be smaller than the
    // page that was read; a wilder value is a bug, not a number to display.
    const total = integer(data["total"], 0);
    if (
        items.length > limit ||
        total < items.length ||
        total > MAX_TOTAL ||
        items.some(
            (row, index) =>
                (position !== null && !isAfter(row, position)) ||
                (index > 0 &&
                    !isAfter(row, {
                        name: items[index - 1]?.name ?? "",
                        id: items[index - 1]?.id ?? "",
                    })),
        )
    )
        invalid();
    // The next cursor must repeat the last returned row, so a page can never be
    // resumed from a row the reader has not seen.
    const last = items[items.length - 1];
    if (nextCursor !== null) {
        const position = cursor(nextCursor);
        if (
            last === undefined ||
            position.name !== last.name ||
            position.id !== last.id
        )
            invalid();
    }
    return { kind: "desktop", page: { items, nextCursor, total } };
}
/**
 * Open the system folder picker through Rust. The dialog is owned by the
 * desktop app; closing it resolves as `cancelled` rather than an error.
 */
export async function pickSkillFolder(title: string): Promise<PickedFolder> {
    desktop();
    const data = await call("pick_skill_folder", { title });
    if (!isObject(data)) invalid();
    if (data["kind"] === "cancelled") {
        if (!hasExactKeys(data, ["kind"])) invalid();
        return { kind: "cancelled" };
    }
    const selected = object(data, ["kind", "path"]);
    if (selected["kind"] !== "selected") invalid();
    const path = string(selected["path"]);
    // A path with no characters or with control characters cannot be displayed
    // or sent back; Rust validates the value again before it is used.
    if (hasControlCharacter(path) || path.trim() === "") invalid();
    return { kind: "selected", path };
}
/**
 * Import every skill inside one chosen folder: the folder itself when it is a
 * skill, otherwise each of its immediate skill subfolders. Refused folders come
 * back in the report instead of failing the whole import; the source folders are
 * only read.
 */
export async function importSkillFolder(path: string): Promise<ImportReport> {
    desktop();
    const data = object(
        await call("import_skill_folder", { request: { path } }),
        ["imported", "skipped"],
    );
    if (!Array.isArray(data["imported"]) || !Array.isArray(data["skipped"]))
        invalid();
    const imported = data["imported"].map((value: unknown) => record(value));
    const skipped = data["skipped"].map((value: unknown) =>
        skippedSkill(value),
    );
    if (
        imported.length > MAX_CANDIDATES ||
        skipped.length > MAX_CANDIDATES ||
        imported.length + skipped.length === 0 ||
        new Set(imported.map((row) => row.id)).size !== imported.length
    )
        invalid();
    // An import never records an upstream source or fetch method for the user.
    if (
        imported.some(
            (row) =>
                row.sourceUrl !== null ||
                row.sourceRevision !== null ||
                row.fetchMethod !== null,
        )
    )
        invalid();
    return { imported, skipped };
}
/** Read one refused folder entry: a safe name and a translatable reason. */
function skippedSkill(value: unknown): SkippedSkill {
    const data = object(value, ["folder", "code"]);
    const folder = string(data["folder"]);
    const code = string(data["code"]);
    if (
        folder.trim() === "" ||
        folder.length > 128 ||
        hasControlCharacter(folder)
    )
        invalid();
    const known = SKILL_CODES.find((candidate) => candidate === code);
    if (known === undefined) invalid();
    return { folder, code: known };
}
/** Re-read one skill before editing its source or retrying an unknown result. */
export async function getSkill(id: string): Promise<SkillRecord> {
    desktop();
    const row = record(await call("get_skill", { id }));
    if (row.id !== id) invalid();
    return row;
}
/** Record or clear the upstream source of an existing skill. */
export async function setSkillSource(input: {
    id: string;
    expectedRevision: number;
    url: string | null;
    revision: string | null;
    fetchMethod: SkillFetchMethod | null;
    installerVersion: string | null;
}): Promise<SkillRecord> {
    desktop();
    const row = record(await call("set_skill_source", { request: input }));
    if (row.id !== input.id || row.revision !== input.expectedRevision + 1)
        invalid();
    return row;
}
/** Validate the command shown before any installer runs. */
function installPreview(value: unknown): SkillInstallPreview {
    const data = object(value, [
        "method",
        "sourceUrl",
        "revision",
        "installer",
        "installerVersion",
        "command",
    ]);
    const method = fetchMethod(data["method"]);
    if (method === null) invalid();
    const sourceUrl = string(data["sourceUrl"]);
    const revision = optionalString(data["revision"]);
    const installer = string(data["installer"]);
    const installerVersion = optionalString(data["installerVersion"]);
    const reviewed = command(string(data["command"]));
    if (
        sourceUrl === "" ||
        sourceUrl.length > 512 ||
        hasControlCharacter(sourceUrl) ||
        installer === "" ||
        installer.length > 64 ||
        hasControlCharacter(installer)
    )
        invalid();
    if (method === "git") {
        if (
            installer !== "git" ||
            revision === null ||
            installerVersion !== null ||
            !reviewed.startsWith("git fetch --depth 1 ")
        )
            invalid();
    } else if (
        installer !== "skills" ||
        revision !== null ||
        installerVersion !== SKILLS_INSTALLER_VERSION ||
        !INSTALLER_VERSION_PATTERN.test(installerVersion) ||
        !reviewed.startsWith("npx --yes skills@")
    ) {
        invalid();
    }
    return {
        method,
        sourceUrl,
        revision,
        installer,
        installerVersion,
        command: reviewed,
    };
}
/**
 * Review a source without network access or a process. Browser mode returns an
 * honest preview marker; the dialog never invents a command.
 */
export async function previewSkillInstall(
    source: SkillInstallSource,
): Promise<
    { kind: "preview" } | { kind: "desktop"; preview: SkillInstallPreview }
> {
    if (!isTauri()) return { kind: "preview" };
    const preview = installPreview(
        await call("preview_skill_install", { source }),
    );
    // A different acquisition path is a contract break, not a new choice.
    if (preview.method !== source.method) invalid();
    return { kind: "desktop", preview };
}
/** Run the reviewed installer and import every skill it acquired. */
export async function installSkill(
    source: SkillInstallSource,
): Promise<SkillInstallReport> {
    desktop();
    return installSkillReport(await call("install_skill", { source }), source);
}

function marketplaceSkill(value: unknown): MarketplaceSkill {
    const data = object(value, ["id", "source", "skillId", "name", "installs"]);
    const id = string(data["id"]);
    const source = string(data["source"]);
    const skillId = string(data["skillId"]);
    const name = string(data["name"]);
    const installs = integer(data["installs"], 0);
    // Catalog rows are display data: require a GitHub source and safe labels.
    // Install still re-validates the skill selector through the Git path.
    if (
        id === "" ||
        id.length > 256 ||
        hasControlCharacter(id) ||
        !GITHUB_OWNER_REPO.test(source) ||
        skillId === "" ||
        skillId.length > 64 ||
        skillId.includes("/") ||
        hasControlCharacter(skillId) ||
        name.trim() === "" ||
        name.length > 128 ||
        hasControlCharacter(name) ||
        id !== `${source}/${skillId}`
    )
        invalid();
    return { id, source, skillId, name, installs };
}

function marketplaceRows(value: unknown, limit: number): MarketplaceSkill[] {
    if (!Array.isArray(value) || value.length > limit) invalid();
    const rows = value.map((entry: unknown) => marketplaceSkill(entry));
    if (new Set(rows.map((row) => row.id)).size !== rows.length) invalid();
    return rows;
}

/**
 * Search the skills.sh catalog. Browser mode returns an honest preview marker;
 * it never invents rows.
 */
export async function searchMarketplaceSkills(
    query: string,
    limit = 40,
): Promise<
    { kind: "preview" } | { kind: "desktop"; skills: MarketplaceSkill[] }
> {
    if (!isTauri()) return { kind: "preview" };
    const trimmed = query.trim();
    if (
        trimmed.length < 2 ||
        trimmed.length > 200 ||
        limit < 1 ||
        limit > MAX_MARKETPLACE_SEARCH
    )
        throw new SkillRequestError("invalid_request");
    return {
        kind: "desktop",
        skills: marketplaceRows(
            await call("search_marketplace_skills", { query: trimmed, limit }),
            MAX_MARKETPLACE_SEARCH,
        ),
    };
}

/**
 * Load one skills.sh leaderboard. Browser mode returns an honest preview marker.
 */
export async function listMarketplaceSkills(
    board: MarketplaceBoard,
): Promise<
    { kind: "preview" } | { kind: "desktop"; skills: MarketplaceSkill[] }
> {
    if (!isTauri()) return { kind: "preview" };
    return {
        kind: "desktop",
        skills: marketplaceRows(
            await call("list_marketplace_skills", { board }),
            MAX_MARKETPLACE_ROWS,
        ),
    };
}

/** Show the Git HEAD command for one marketplace row before anything runs. */
export async function previewMarketplaceInstall(
    request: MarketplaceInstallRequest,
): Promise<
    { kind: "preview" } | { kind: "desktop"; preview: SkillInstallPreview }
> {
    if (!isTauri()) return { kind: "preview" };
    const source = request.source.trim();
    const skillId = request.skillId.trim();
    if (!GITHUB_OWNER_REPO.test(source) || !NAME_PATTERN.test(skillId))
        throw new SkillRequestError("invalid_request");
    const preview = installPreview(
        await call("preview_marketplace_install", {
            request: { source, skillId },
        }),
    );
    if (preview.method !== "git" || preview.revision !== "HEAD") invalid();
    return { kind: "desktop", preview };
}

/** Install one marketplace skill from GitHub HEAD into the central library. */
export async function installMarketplaceSkill(
    request: MarketplaceInstallRequest,
): Promise<SkillInstallReport> {
    desktop();
    const source = request.source.trim();
    const skillId = request.skillId.trim();
    if (!GITHUB_OWNER_REPO.test(source) || !NAME_PATTERN.test(skillId))
        throw new SkillRequestError("invalid_request");
    const report = installSkillReport(
        await call("install_marketplace_skill", {
            request: { source, skillId },
        }),
        {
            method: "git",
            source: {
                repository: source,
                revision: "HEAD",
                skill: skillId,
            },
        },
    );
    if (report.method !== "git" || report.revision === null) invalid();
    return report;
}

/**
 * Shared install-report validation used by both hand-entered and marketplace
 * installs. The expected source method must match what Rust recorded.
 */
function installSkillReport(
    value: unknown,
    source: SkillInstallSource,
): SkillInstallReport {
    const data = object(value, [
        "method",
        "sourceUrl",
        "revision",
        "installer",
        "installerVersion",
        "command",
        "exitStatus",
        "imported",
        "skipped",
    ]);
    const method = fetchMethod(data["method"]);
    if (method === null) invalid();
    const sourceUrl = string(data["sourceUrl"]);
    const revision = optionalString(data["revision"]);
    const installer = string(data["installer"]);
    const installerVersion = optionalString(data["installerVersion"]);
    const reviewed = command(string(data["command"]));
    const exitStatus = integer(data["exitStatus"], 0);
    if (method === "git") {
        if (
            installer !== "git" ||
            installerVersion !== null ||
            revision === null
        )
            invalid();
    } else if (
        installer !== "skills" ||
        installerVersion !== SKILLS_INSTALLER_VERSION ||
        revision !== null
    ) {
        invalid();
    }
    if (!Array.isArray(data["imported"]) || !Array.isArray(data["skipped"]))
        invalid();
    const imported = data["imported"].map((entry: unknown) => record(entry));
    const skipped = data["skipped"].map((entry: unknown) =>
        skippedSkill(entry),
    );
    if (
        method !== source.method ||
        exitStatus !== 0 ||
        imported.length > MAX_CANDIDATES ||
        skipped.length > MAX_CANDIDATES ||
        imported.length + skipped.length === 0 ||
        new Set(imported.map((row) => row.id)).size !== imported.length ||
        imported.some(
            (row) =>
                row.originPath !== null ||
                row.sourceUrl !== sourceUrl ||
                row.fetchMethod !== method ||
                row.installerVersion !== installerVersion ||
                row.installerCommand !== reviewed ||
                (method === "git"
                    ? row.sourceRevision !== revision
                    : row.sourceRevision !== null),
        )
    )
        invalid();
    return {
        method,
        sourceUrl,
        revision,
        installer,
        installerVersion,
        command: reviewed,
        exitStatus,
        imported,
        skipped,
    };
}

/**
 * Remove one reviewed skill record and vibemate's managed copy of it. The
 * folder the skill was imported from is never part of this request. A stale
 * confirmation is refused, and copied files that could not be removed come
 * back as `cleanupPending` instead of being reported as gone.
 */
export async function deleteSkill(request: {
    id: string;
    expectedRevision: number;
}): Promise<{ id: string; cleanupPending: boolean }> {
    desktop();
    const data = object(await call("delete_skill", { request }), [
        "id",
        "cleanupPending",
    ]);
    const id = string(data["id"]);
    if (id !== request.id || !ID_PATTERN.test(id)) invalid();
    return { id, cleanupPending: boolean(data["cleanupPending"]) };
}
/** Retry removing parked copies left by finished deletions; false means more. */
export async function cleanupDeletedSkillCopies(): Promise<boolean> {
    desktop();
    return boolean(await call("cleanup_deleted_skill_copies", {}));
}

/** How a skill is linked into an Agent skills directory. */
export type SkillDeployMode = "symlink" | "copy";
/** Filesystem classification of the deploy target at preview time. */
export type SkillDeployTargetState =
    "absent" | "owned_symlink" | "owned_copy" | "foreign";
/** Agents that accept central skill deployment in this build. */
export type SkillDeployAgent = "pi" | "grokBuild" | "claudeCode";
/** Preview of deploying one skill into an Agent user skills directory. */
export interface SkillDeployPreview {
    agentId: SkillDeployAgent;
    skillId: string;
    skillName: string;
    skillRevision: number;
    contentDigest: string;
    configRoot: string;
    targetPath: string;
    targetName: string;
    targetState: SkillDeployTargetState;
    symlinkAvailable: boolean;
    suggestedMode: SkillDeployMode;
    canApply: boolean;
}
/** Result of applying a reviewed skill deployment. */
export interface SkillDeployed {
    agentId: SkillDeployAgent;
    skillId: string;
    skillName: string;
    skillRevision: number;
    mode: SkillDeployMode;
    targetPath: string;
}
/** Outcome of disabling a skill deployment. */
export interface SkillDeployDisabled {
    agentId: SkillDeployAgent;
    skillId: string;
    targetPath: string;
    targetRemoved: boolean;
    leftForeign: boolean;
}
/** One recorded skill deployment for an Agent configuration root. */
export interface SkillDeploymentRecord {
    skillId: string;
    skillName: string;
    agentId: SkillDeployAgent;
    configRoot: string;
    mode: SkillDeployMode;
    skillRevision: number;
    contentDigest: string;
    inSync: boolean;
    deployedAtMs: number;
}

const DEPLOY_MODES = ["symlink", "copy"] as const;
const TARGET_STATES = [
    "absent",
    "owned_symlink",
    "owned_copy",
    "foreign",
] as const;
const DEPLOY_AGENTS = ["pi", "grokBuild", "claudeCode"] as const;

function deployMode(value: unknown): SkillDeployMode {
    const text = string(value);
    const known = DEPLOY_MODES.find((mode) => mode === text);
    if (known === undefined) invalid();
    return known;
}

function deployAgent(value: unknown): SkillDeployAgent {
    const text = string(value);
    const known = DEPLOY_AGENTS.find((agent) => agent === text);
    if (known === undefined) invalid();
    return known;
}

function targetState(value: unknown): SkillDeployTargetState {
    const text = string(value);
    const known = TARGET_STATES.find((state) => state === text);
    if (known === undefined) invalid();
    return known;
}

function deployPreview(value: unknown): SkillDeployPreview {
    const data = object(value, [
        "agentId",
        "skillId",
        "skillName",
        "skillRevision",
        "contentDigest",
        "configRoot",
        "targetPath",
        "targetName",
        "targetState",
        "symlinkAvailable",
        "suggestedMode",
        "canApply",
    ]);
    const skillId = string(data["skillId"]);
    const skillName = string(data["skillName"]);
    const contentDigest = string(data["contentDigest"]);
    const configRoot = string(data["configRoot"]);
    const targetPath = string(data["targetPath"]);
    const targetName = string(data["targetName"]);
    if (
        !ID_PATTERN.test(skillId) ||
        !NAME_PATTERN.test(skillName) ||
        !DIGEST_PATTERN.test(contentDigest) ||
        targetName !== skillName ||
        hasControlCharacter(configRoot) ||
        hasControlCharacter(targetPath) ||
        configRoot.trim() === "" ||
        targetPath.trim() === ""
    )
        invalid();
    return {
        agentId: deployAgent(data["agentId"]),
        skillId,
        skillName,
        skillRevision: integer(data["skillRevision"], 1),
        contentDigest,
        configRoot,
        targetPath,
        targetName,
        targetState: targetState(data["targetState"]),
        symlinkAvailable: boolean(data["symlinkAvailable"]),
        suggestedMode: deployMode(data["suggestedMode"]),
        canApply: boolean(data["canApply"]),
    };
}

function deploymentRecord(value: unknown): SkillDeploymentRecord {
    const data = object(value, [
        "skillId",
        "skillName",
        "agentId",
        "configRoot",
        "mode",
        "skillRevision",
        "contentDigest",
        "inSync",
        "deployedAtMs",
    ]);
    const skillId = string(data["skillId"]);
    const skillName = string(data["skillName"]);
    const configRoot = string(data["configRoot"]);
    const contentDigest = string(data["contentDigest"]);
    if (
        !ID_PATTERN.test(skillId) ||
        !NAME_PATTERN.test(skillName) ||
        !DIGEST_PATTERN.test(contentDigest) ||
        hasControlCharacter(configRoot) ||
        configRoot.trim() === ""
    )
        invalid();
    return {
        skillId,
        skillName,
        agentId: deployAgent(data["agentId"]),
        configRoot,
        mode: deployMode(data["mode"]),
        skillRevision: integer(data["skillRevision"], 1),
        contentDigest,
        inSync: boolean(data["inSync"]),
        deployedAtMs: integer(data["deployedAtMs"], 0),
    };
}

/**
 * Preview deploying one library skill into an Agent skills directory. Browser
 * mode reports that the desktop app is required; it never invents a path.
 */
export async function previewSkillDeploy(
    agentId: SkillDeployAgent,
    skillId: string,
): Promise<
    { kind: "preview" } | { kind: "desktop"; preview: SkillDeployPreview }
> {
    if (!isTauri()) return { kind: "preview" };
    const preview = deployPreview(
        await call("preview_skill_deploy", { agentId, skillId }),
    );
    if (preview.skillId !== skillId || preview.agentId !== agentId) invalid();
    return { kind: "desktop", preview };
}

/** Apply a reviewed skill deployment. */
export async function applySkillDeploy(input: {
    agentId: SkillDeployAgent;
    skillId: string;
    expectedSkillRevision: number;
    configRoot: string;
    mode: SkillDeployMode;
    targetState: SkillDeployTargetState;
}): Promise<SkillDeployed> {
    desktop();
    const data = object(await call("apply_skill_deploy", { reviewed: input }), [
        "agentId",
        "skillId",
        "skillName",
        "skillRevision",
        "mode",
        "targetPath",
    ]);
    const skillId = string(data["skillId"]);
    const skillName = string(data["skillName"]);
    const targetPath = string(data["targetPath"]);
    const agentId = deployAgent(data["agentId"]);
    if (
        skillId !== input.skillId ||
        agentId !== input.agentId ||
        !NAME_PATTERN.test(skillName) ||
        hasControlCharacter(targetPath) ||
        targetPath.trim() === ""
    )
        invalid();
    return {
        agentId,
        skillId,
        skillName,
        skillRevision: integer(data["skillRevision"], 1),
        mode: deployMode(data["mode"]),
        targetPath,
    };
}

/** Revoke one vibemate-managed skill deployment. */
export async function disableSkillDeploy(input: {
    agentId: SkillDeployAgent;
    skillId: string;
    configRoot: string;
}): Promise<SkillDeployDisabled> {
    desktop();
    const data = object(
        await call("disable_skill_deploy", { request: input }),
        ["agentId", "skillId", "targetPath", "targetRemoved", "leftForeign"],
    );
    const skillId = string(data["skillId"]);
    const targetPath = string(data["targetPath"]);
    const agentId = deployAgent(data["agentId"]);
    if (
        skillId !== input.skillId ||
        agentId !== input.agentId ||
        hasControlCharacter(targetPath) ||
        targetPath.trim() === ""
    )
        invalid();
    return {
        agentId,
        skillId,
        targetPath,
        targetRemoved: boolean(data["targetRemoved"]),
        leftForeign: boolean(data["leftForeign"]),
    };
}

/** List deployments for one resolved Agent configuration root. */
export async function listAgentSkillDeployments(
    agentId: SkillDeployAgent,
): Promise<
    { kind: "preview" } | { kind: "desktop"; items: SkillDeploymentRecord[] }
> {
    if (!isTauri()) return { kind: "preview" };
    const data = await call("list_agent_skill_deployments", { agentId });
    if (!Array.isArray(data)) invalid();
    const items = data.map((value: unknown) => deploymentRecord(value));
    if (items.some((row) => row.agentId !== agentId)) invalid();
    return { kind: "desktop", items };
}

/** List deployments recorded for one central skill. */
export async function listSkillDeployments(
    skillId: string,
): Promise<
    { kind: "preview" } | { kind: "desktop"; items: SkillDeploymentRecord[] }
> {
    if (!isTauri()) return { kind: "preview" };
    const data = await call("list_skill_deployments", { skillId });
    if (!Array.isArray(data)) invalid();
    const items = data.map((value: unknown) => deploymentRecord(value));
    if (items.some((row) => row.skillId !== skillId)) invalid();
    return { kind: "desktop", items };
}

/** Kind of unfinished skill deploy journal row. */
export type SkillDeployOperationKind = "apply" | "disable";
/** Durable journal state for one unfinished skill deploy operation. */
export type SkillDeployOperationState = "prepared" | "unknown";
/** Live classification of one unfinished skill deploy journal row. */
export type SkillDeployRecoveryClassification =
    | "completed"
    | "not_started"
    | "needs_record"
    | "needs_record_clear"
    | "conflict";
/** One unfinished skill deploy/disable operation for the recovery panel. */
export interface SkillDeployRecoveryItem {
    id: string;
    kind: SkillDeployOperationKind;
    state: SkillDeployOperationState;
    classification: SkillDeployRecoveryClassification;
    skillId: string;
    skillName: string;
    agentId: SkillDeployAgent;
    configRoot: string;
    targetPath: string;
    mode: SkillDeployMode | null;
    canFinish: boolean;
    canDismiss: boolean;
}
/** Outcome of finishing or dismissing one recovery item. */
export interface SkillDeployRecoveryResolved {
    id: string;
    classification: SkillDeployRecoveryClassification;
}

const OPERATION_KINDS = ["apply", "disable"] as const;
const OPERATION_STATES = ["prepared", "unknown"] as const;
const RECOVERY_CLASSIFICATIONS = [
    "completed",
    "not_started",
    "needs_record",
    "needs_record_clear",
    "conflict",
] as const;

function operationKind(value: unknown): SkillDeployOperationKind {
    const text = string(value);
    const known = OPERATION_KINDS.find((kind) => kind === text);
    if (known === undefined) invalid();
    return known;
}

function operationState(value: unknown): SkillDeployOperationState {
    const text = string(value);
    const known = OPERATION_STATES.find((state) => state === text);
    if (known === undefined) invalid();
    return known;
}

function recoveryClassification(
    value: unknown,
): SkillDeployRecoveryClassification {
    const text = string(value);
    const known = RECOVERY_CLASSIFICATIONS.find(
        (classification) => classification === text,
    );
    if (known === undefined) invalid();
    return known;
}

function recoveryItem(value: unknown): SkillDeployRecoveryItem {
    const data = object(value, [
        "id",
        "kind",
        "state",
        "classification",
        "skillId",
        "skillName",
        "agentId",
        "configRoot",
        "targetPath",
        "mode",
        "canFinish",
        "canDismiss",
    ]);
    const id = string(data["id"]);
    if (!ID_PATTERN.test(id)) invalid();
    const skillId = string(data["skillId"]);
    if (!ID_PATTERN.test(skillId)) invalid();
    const agentId = string(data["agentId"]);
    const knownAgent = DEPLOY_AGENTS.find((agent) => agent === agentId);
    if (knownAgent === undefined) invalid();
    const mode = data["mode"] === null ? null : deployMode(data["mode"]);
    const canFinish = boolean(data["canFinish"]);
    const canDismiss = boolean(data["canDismiss"]);
    const classification = recoveryClassification(data["classification"]);
    if (canFinish === (classification === "conflict")) invalid();
    if (
        canDismiss !==
        (classification === "completed" ||
            classification === "not_started" ||
            classification === "conflict")
    ) {
        invalid();
    }
    return {
        id,
        kind: operationKind(data["kind"]),
        state: operationState(data["state"]),
        classification,
        skillId,
        skillName: string(data["skillName"]),
        agentId: knownAgent,
        configRoot: string(data["configRoot"]),
        targetPath: string(data["targetPath"]),
        mode,
        canFinish,
        canDismiss,
    };
}

function recoveryResolved(value: unknown): SkillDeployRecoveryResolved {
    const data = object(value, ["id", "classification"]);
    const id = string(data["id"]);
    if (!ID_PATTERN.test(id)) invalid();
    return {
        id,
        classification: recoveryClassification(data["classification"]),
    };
}

/** List unfinished skill deploy journal rows, or an honest browser preview. */
export async function listSkillDeployRecoveries(): Promise<
    { kind: "preview" } | { kind: "desktop"; items: SkillDeployRecoveryItem[] }
> {
    if (!isTauri()) return { kind: "preview" };
    const data = await call("list_skill_deploy_recoveries", {});
    if (!Array.isArray(data)) invalid();
    return {
        kind: "desktop",
        items: data.map((value: unknown) => recoveryItem(value)),
    };
}

/** Finish one unfinished skill deploy journal row after re-classification. */
export async function finishSkillDeployRecovery(input: {
    id: string;
    expectedClassification: SkillDeployRecoveryClassification;
}): Promise<SkillDeployRecoveryResolved> {
    desktop();
    if (!ID_PATTERN.test(input.id)) {
        throw new SkillRequestError("invalid_request");
    }
    const resolved = recoveryResolved(
        await call("finish_skill_deploy_recovery", { request: input }),
    );
    if (
        resolved.id !== input.id ||
        resolved.classification !== input.expectedClassification
    ) {
        invalid();
    }
    return resolved;
}

/** Dismiss one unfinished skill deploy journal row without touching files. */
export async function dismissSkillDeployRecovery(input: {
    id: string;
    expectedClassification: SkillDeployRecoveryClassification;
}): Promise<SkillDeployRecoveryResolved> {
    desktop();
    if (!ID_PATTERN.test(input.id)) {
        throw new SkillRequestError("invalid_request");
    }
    const resolved = recoveryResolved(
        await call("dismiss_skill_deploy_recovery", { request: input }),
    );
    if (
        resolved.id !== input.id ||
        resolved.classification !== input.expectedClassification
    ) {
        invalid();
    }
    return resolved;
}

/** Classification of local vs upstream change for an update preview. */
export type SkillUpdateStatus =
    "current" | "local_modified" | "upstream_changed" | "diverged";
/** How apply may resolve an update preview. */
export type SkillUpdateStrategy = "keep" | "replace";
/** Per-deployment outcome after replacing library content. */
export type SkillUpdateDeployOutcome =
    "refreshed" | "symlink_updated" | "failed";
/** Path-level summary versus the live managed copy. */
export interface SkillUpdateDiff {
    addedFiles: string[];
    removedFiles: string[];
    changedFiles: string[];
    descriptionChanged: boolean;
    liveDescription: string;
    candidateDescription: string;
}
/** Preview of updating one library skill from its recorded source. */
export interface SkillUpdatePreview {
    skillId: string;
    skillName: string;
    skillRevision: number;
    fetchMethod: SkillFetchMethod;
    sourceUrl: string;
    command: string;
    recordedSourceRevision: string | null;
    candidateSourceRevision: string | null;
    recordedContentDigest: string;
    liveContentDigest: string;
    candidateContentDigest: string;
    candidateFileCount: number;
    candidateByteSize: number;
    status: SkillUpdateStatus;
    strategies: SkillUpdateStrategy[];
    diff: SkillUpdateDiff;
}
/** One Agent deployment touched while applying a skill update. */
export interface SkillUpdateDeployResult {
    agentId: SkillDeployAgent;
    configRoot: string;
    mode: SkillDeployMode;
    outcome: SkillUpdateDeployOutcome;
}
/** Result of applying a reviewed keep or replace strategy. */
export interface SkillUpdateApplied {
    skill: SkillRecord;
    strategy: SkillUpdateStrategy;
    status: SkillUpdateStatus;
    deployments: SkillUpdateDeployResult[];
}

const UPDATE_STATUSES = [
    "current",
    "local_modified",
    "upstream_changed",
    "diverged",
] as const;
const UPDATE_STRATEGIES = ["keep", "replace"] as const;

function updateStatus(value: unknown): SkillUpdateStatus {
    const text = string(value);
    const known = UPDATE_STATUSES.find((status) => status === text);
    if (known === undefined) invalid();
    return known;
}

function updateStrategy(value: unknown): SkillUpdateStrategy {
    const text = string(value);
    const known = UPDATE_STRATEGIES.find((strategy) => strategy === text);
    if (known === undefined) invalid();
    return known;
}

function pathList(value: unknown): string[] {
    if (!Array.isArray(value) || value.length > 64) invalid();
    return value.map((entry: unknown) => {
        const path = string(entry);
        if (
            path.trim() === "" ||
            hasControlCharacter(path) ||
            path.length > 512
        )
            invalid();
        return path;
    });
}

function updateDiff(value: unknown): SkillUpdateDiff {
    const data = object(value, [
        "addedFiles",
        "removedFiles",
        "changedFiles",
        "descriptionChanged",
        "liveDescription",
        "candidateDescription",
    ]);
    const liveDescription = string(data["liveDescription"]);
    const candidateDescription = string(data["candidateDescription"]);
    if (
        liveDescription.trim() === "" ||
        candidateDescription.trim() === "" ||
        hasControlCharacter(liveDescription) ||
        hasControlCharacter(candidateDescription)
    )
        invalid();
    return {
        addedFiles: pathList(data["addedFiles"]),
        removedFiles: pathList(data["removedFiles"]),
        changedFiles: pathList(data["changedFiles"]),
        descriptionChanged: boolean(data["descriptionChanged"]),
        liveDescription,
        candidateDescription,
    };
}

function updatePreview(value: unknown): SkillUpdatePreview {
    const data = object(value, [
        "skillId",
        "skillName",
        "skillRevision",
        "fetchMethod",
        "sourceUrl",
        "command",
        "recordedSourceRevision",
        "candidateSourceRevision",
        "recordedContentDigest",
        "liveContentDigest",
        "candidateContentDigest",
        "candidateFileCount",
        "candidateByteSize",
        "status",
        "strategies",
        "diff",
    ]);
    const skillId = string(data["skillId"]);
    const skillName = string(data["skillName"]);
    const sourceUrl = string(data["sourceUrl"]);
    const command = string(data["command"]);
    const recordedContentDigest = string(data["recordedContentDigest"]);
    const liveContentDigest = string(data["liveContentDigest"]);
    const candidateContentDigest = string(data["candidateContentDigest"]);
    const fetchMethod = string(data["fetchMethod"]);
    if (
        !ID_PATTERN.test(skillId) ||
        !NAME_PATTERN.test(skillName) ||
        (fetchMethod !== "git" && fetchMethod !== "npx") ||
        !DIGEST_PATTERN.test(recordedContentDigest) ||
        !DIGEST_PATTERN.test(liveContentDigest) ||
        !DIGEST_PATTERN.test(candidateContentDigest) ||
        hasControlCharacter(sourceUrl) ||
        hasControlCharacter(command) ||
        sourceUrl.trim() === "" ||
        command.trim() === ""
    )
        invalid();
    if (!Array.isArray(data["strategies"]) || data["strategies"].length > 8)
        invalid();
    const recordedSourceRevision =
        data["recordedSourceRevision"] === null
            ? null
            : string(data["recordedSourceRevision"]);
    const candidateSourceRevision =
        data["candidateSourceRevision"] === null
            ? null
            : string(data["candidateSourceRevision"]);
    if (
        recordedSourceRevision !== null &&
        (recordedSourceRevision.trim() === "" ||
            hasControlCharacter(recordedSourceRevision))
    )
        invalid();
    if (
        candidateSourceRevision !== null &&
        (candidateSourceRevision.trim() === "" ||
            hasControlCharacter(candidateSourceRevision))
    )
        invalid();
    return {
        skillId,
        skillName,
        skillRevision: integer(data["skillRevision"], 1),
        fetchMethod,
        sourceUrl,
        command,
        recordedSourceRevision,
        candidateSourceRevision,
        recordedContentDigest,
        liveContentDigest,
        candidateContentDigest,
        candidateFileCount: integer(data["candidateFileCount"], 0),
        candidateByteSize: integer(data["candidateByteSize"], 0),
        status: updateStatus(data["status"]),
        strategies: data["strategies"].map((entry: unknown) =>
            updateStrategy(entry),
        ),
        diff: updateDiff(data["diff"]),
    };
}

/**
 * Preview updating one library skill from its recorded Git or npx source.
 * Browser mode reports that the desktop app is required; it never invents a
 * successful comparison.
 */
export async function previewSkillUpdate(input: {
    id: string;
    expectedRevision: number;
}): Promise<
    { kind: "preview" } | { kind: "desktop"; preview: SkillUpdatePreview }
> {
    if (!isTauri()) return { kind: "preview" };
    const preview = updatePreview(
        await call("preview_skill_update", { request: input }),
    );
    if (
        preview.skillId !== input.id ||
        preview.skillRevision !== input.expectedRevision
    )
        invalid();
    return { kind: "desktop", preview };
}

const UPDATE_DEPLOY_OUTCOMES = [
    "refreshed",
    "symlink_updated",
    "failed",
] as const;

function updateDeployOutcome(value: unknown): SkillUpdateDeployOutcome {
    const text = string(value);
    const known = UPDATE_DEPLOY_OUTCOMES.find((outcome) => outcome === text);
    if (known === undefined) invalid();
    return known;
}

function updateDeployResult(value: unknown): SkillUpdateDeployResult {
    if (!isObject(value)) invalid();
    return {
        agentId: deployAgent(value["agentId"]),
        configRoot: string(value["configRoot"]),
        mode: deployMode(value["mode"]),
        outcome: updateDeployOutcome(value["outcome"]),
    };
}

function updateApplied(value: unknown): SkillUpdateApplied {
    if (!isObject(value)) invalid();
    const deployments = value["deployments"];
    if (!Array.isArray(deployments)) invalid();
    return {
        skill: record(value["skill"]),
        strategy: updateStrategy(value["strategy"]),
        status: updateStatus(value["status"]),
        deployments: deployments.map((row: unknown) => updateDeployResult(row)),
    };
}

/**
 * Apply a reviewed keep or replace strategy after an update preview. Browser
 * mode refuses; it never invents a successful apply.
 */
export async function applySkillUpdate(input: {
    id: string;
    expectedRevision: number;
    strategy: SkillUpdateStrategy;
    expectedStatus: SkillUpdateStatus;
    expectedRecordedDigest: string;
    expectedLiveDigest: string;
    expectedCandidateDigest: string;
    expectedCandidateSourceRevision: string | null;
}): Promise<SkillUpdateApplied> {
    desktop();
    if (!ID_PATTERN.test(input.id) || input.expectedRevision < 1) {
        throw new SkillRequestError("invalid_request");
    }
    const applied = updateApplied(
        await call("apply_skill_update", { request: input }),
    );
    if (
        applied.skill.id !== input.id ||
        applied.strategy !== input.strategy ||
        applied.status !== input.expectedStatus
    ) {
        invalid();
    }
    return applied;
}

/**
 * Resolve remote Git `HEAD` for one skill and store the comparison. Browser
 * mode refuses; it never invents an up-to-date result.
 */
export async function checkSkillRevision(id: string): Promise<SkillRecord> {
    desktop();
    if (!ID_PATTERN.test(id)) throw new SkillRequestError("invalid_request");
    const row = record(await call("check_skill_revision", { request: { id } }));
    if (row.id !== id || row.updateCheckStatus === null) invalid();
    return row;
}
