import { invoke, isTauri } from "@tauri-apps/api/core";
import { beforeEach, expect, test, vi } from "vitest";
import {
    cleanupDeletedSkillCopies,
    deleteSkill,
    getMarketplaceSkillDetail,
    importSkillFolder,
    installMarketplaceSkill,
    installSkill,
    listMarketplaceSkills,
    listSkills,
    pickSkillFolder,
    previewMarketplaceInstall,
    previewSkillInstall,
    searchMarketplaceSkills,
    setSkillSource,
    SkillRequestError,
    type MarketplaceSkill,
    type SkillRecord,
} from "./skills";
vi.mock(import("@tauri-apps/api/core"));

const ID = "0123456789abcdef0123456789abcdef";
const OTHER_ID = "fedcba9876543210fedcba9876543210";
const DIGEST = "ab".repeat(32);
const row: SkillRecord = {
    id: ID,
    name: "demo-skill",
    description: "Explains the demo skill.",
    originPath: "/tmp/sources/demo",
    sourceUrl: null,
    sourceRevision: null,
    fetchMethod: null,
    installerVersion: null,
    installerCommand: null,
    remoteRevision: null,
    updateCheckStatus: null,
    lastCheckedAtMs: null,
    lastCheckError: null,
    contentDigest: DIGEST,
    fileCount: 2,
    byteSize: 128,
    revision: 1,
    importedAtMs: 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_000,
};

beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(isTauri).mockReturnValue(true);
});

test("preview reads nothing and rejects every write", async () => {
    vi.mocked(isTauri).mockReturnValue(false);
    expect(await listSkills(null)).toEqual({ kind: "preview" });
    await expect(pickSkillFolder("Choose")).rejects.toMatchObject({
        code: "desktop_required",
    });
    await expect(importSkillFolder("/tmp/sources/demo")).rejects.toMatchObject({
        code: "desktop_required",
    });
    await expect(
        setSkillSource({
            id: ID,
            expectedRevision: 1,
            url: "https://example.com/skills",
            revision: null,
            fetchMethod: null,
            installerVersion: null,
        }),
    ).rejects.toMatchObject({ code: "desktop_required" });
    await expect(
        deleteSkill({ id: ID, expectedRevision: 1 }),
    ).rejects.toMatchObject({ code: "desktop_required" });
    await expect(cleanupDeletedSkillCopies()).rejects.toMatchObject({
        code: "desktop_required",
    });
    expect(vi.mocked(invoke)).not.toHaveBeenCalled();
});

test("a valid page is returned with the last-row cursor", async () => {
    const second: SkillRecord = {
        ...row,
        id: "fedcba9876543210fedcba9876543210",
        name: "other-skill",
    };
    vi.mocked(invoke).mockResolvedValue({
        items: [row, second],
        nextCursor: `${second.name}\n${second.id}`,
        total: 5,
    });
    const result = await listSkills(null, 2);
    expect(result).toEqual({
        kind: "desktop",
        page: {
            items: [row, second],
            nextCursor: `${second.name}\n${second.id}`,
            total: 5,
        },
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("list_skills", {
        after: null,
        limit: 2,
    });
});

test("malformed pages are rejected instead of shown", async () => {
    const cases: unknown[] = [
        // Unknown extra field.
        { items: [{ ...row, extra: true }], nextCursor: null, total: 1 },
        // Digest that is not a SHA-256 hex string.
        {
            items: [{ ...row, contentDigest: "short" }],
            nextCursor: null,
            total: 1,
        },
        // Skill name with a space.
        { items: [{ ...row, name: "demo skill" }], nextCursor: null, total: 1 },
        // Revision without a source address.
        {
            items: [{ ...row, source_revision: "v1", sourceRevision: "v1" }],
            nextCursor: null,
            total: 1,
        },
        // No origin path and no source address.
        { items: [{ ...row, originPath: null }], nextCursor: null, total: 1 },
        // Negative timestamps and out-of-order updates.
        { items: [{ ...row, updatedAtMs: 0 }], nextCursor: null, total: 1 },
        // Cursor that does not repeat the last row.
        {
            items: [row],
            nextCursor: "other-skill\n0123456789abcdef0123456789abcdef",
            total: 1,
        },
        // Cursor without the name separator.
        { items: [row], nextCursor: ID, total: 1 },
        // More rows than the requested limit.
        { items: [row, row], nextCursor: null, total: 2 },
        // A missing, negative or impossible library total.
        { items: [row], nextCursor: null },
        { items: [row], nextCursor: null, total: -1 },
        { items: [row], nextCursor: null, total: 0 },
        { items: [row], nextCursor: null, total: 100_001 },
        { items: [row], nextCursor: null, total: 1.5 },
    ];
    for (const value of cases) {
        vi.mocked(invoke).mockResolvedValue(value);
        await expect(listSkills(null, 1)).rejects.toMatchObject({
            code: "invalid_response",
        });
    }
});

test("a page after the first must continue in name order", async () => {
    const cursor = `${row.name}\n${row.id}`;
    // The same row again would repeat a visible skill.
    vi.mocked(invoke).mockResolvedValue({
        items: [row],
        nextCursor: null,
        total: 1,
    });
    await expect(listSkills(cursor, 20)).rejects.toMatchObject({
        code: "invalid_response",
    });
    const later: SkillRecord = { ...row, name: "zebra" };
    vi.mocked(invoke).mockResolvedValue({
        items: [later],
        nextCursor: null,
        total: 2,
    });
    await expect(listSkills(cursor, 20)).resolves.toMatchObject({
        kind: "desktop",
    });
});

test("the folder picker reports a choice or a cancel", async () => {
    vi.mocked(invoke).mockResolvedValue({ kind: "cancelled" });
    await expect(pickSkillFolder("Choose a skill folder")).resolves.toEqual({
        kind: "cancelled",
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("pick_skill_folder", {
        title: "Choose a skill folder",
    });

    vi.mocked(invoke).mockResolvedValue({
        kind: "selected",
        path: "/tmp/sources/demo",
    });
    await expect(pickSkillFolder("Choose a skill folder")).resolves.toEqual({
        kind: "selected",
        path: "/tmp/sources/demo",
    });

    const cases: unknown[] = [
        // A selected folder without a path.
        { kind: "selected" },
        // Cancellation must not carry extra fields.
        { kind: "cancelled", path: "/tmp/x" },
        // An unknown outcome.
        { kind: "failed" },
        // A path the interface cannot display.
        { kind: "selected", path: "/tmp/\u0000bad" },
        { kind: "selected", path: "   " },
    ];
    for (const value of cases) {
        vi.mocked(invoke).mockResolvedValue(value);
        await expect(pickSkillFolder("Choose")).rejects.toMatchObject({
            code: "invalid_response",
        });
    }
});

test("a folder import reports added skills and refused folders", async () => {
    vi.mocked(invoke).mockResolvedValue({
        imported: [row],
        skipped: [{ folder: "broken", code: "manifest_invalid" }],
    });
    await expect(importSkillFolder("/tmp/sources/demo")).resolves.toEqual({
        imported: [row],
        skipped: [{ folder: "broken", code: "manifest_invalid" }],
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("import_skill_folder", {
        request: { path: "/tmp/sources/demo" },
    });
});

test("impossible import reports are rejected", async () => {
    const cases: unknown[] = [
        // Nothing imported and nothing refused cannot describe a chosen folder.
        { imported: [], skipped: [] },
        // Duplicate rows for one skill.
        { imported: [row, row], skipped: [] },
        // A reason code the interface does not translate.
        {
            imported: [],
            skipped: [{ folder: "broken", code: "unknown_reason" }],
        },
        // A transport code is not a folder reason.
        {
            imported: [],
            skipped: [{ folder: "broken", code: "desktop_required" }],
        },
        // A folder label that is empty, too long or contains control characters.
        { imported: [], skipped: [{ folder: "  ", code: "name_taken" }] },
        {
            imported: [],
            skipped: [{ folder: "x".repeat(129), code: "name_taken" }],
        },
        {
            imported: [],
            skipped: [{ folder: "bad\u0007", code: "name_taken" }],
        },
        // An import never records an upstream source for the user.
        {
            imported: [{ ...row, sourceUrl: "https://example.com/skills" }],
            skipped: [],
        },
    ];
    for (const value of cases) {
        vi.mocked(invoke).mockResolvedValue(value);
        await expect(
            importSkillFolder("/tmp/sources/demo"),
        ).rejects.toMatchObject({
            code: "invalid_response",
        });
    }

    // A second imported skill keeps the report valid.
    const second: SkillRecord = { ...row, id: OTHER_ID, name: "other-skill" };
    vi.mocked(invoke).mockResolvedValue({
        imported: [row, second],
        skipped: [],
    });
    await expect(importSkillFolder("/tmp/sources/demo")).resolves.toMatchObject(
        {
            imported: [row, second],
        },
    );
});

test("source records must advance the revision they were edited from", async () => {
    const updated: SkillRecord = {
        ...row,
        sourceUrl: "https://example.com/skills",
        sourceRevision: "v1.2.0",
        revision: 2,
        updatedAtMs: row.updatedAtMs + 5,
    };
    vi.mocked(invoke).mockResolvedValue(updated);
    await expect(
        setSkillSource({
            id: ID,
            expectedRevision: 1,
            url: "https://example.com/skills",
            revision: "v1.2.0",
            fetchMethod: null,
            installerVersion: null,
        }),
    ).resolves.toEqual(updated);

    vi.mocked(invoke).mockResolvedValue({ ...updated, id: "f".repeat(32) });
    await expect(
        setSkillSource({
            id: ID,
            expectedRevision: 1,
            url: "https://example.com/skills",
            revision: null,
            fetchMethod: null,
            installerVersion: null,
        }),
    ).rejects.toMatchObject({ code: "invalid_response" });

    vi.mocked(invoke).mockResolvedValue({ ...updated, revision: 5 });
    await expect(
        setSkillSource({
            id: ID,
            expectedRevision: 1,
            url: "https://example.com/skills",
            revision: null,
            fetchMethod: null,
            installerVersion: null,
        }),
    ).rejects.toMatchObject({ code: "invalid_response" });
});

test("a delete acknowledgment must match the request", async () => {
    vi.mocked(invoke).mockResolvedValue({ id: ID, cleanupPending: false });
    expect(await deleteSkill({ id: ID, expectedRevision: 1 })).toEqual({
        id: ID,
        cleanupPending: false,
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("delete_skill", {
        request: { id: ID, expectedRevision: 1 },
    });

    // An acknowledgment for another skill is not accepted as this deletion.
    vi.mocked(invoke).mockResolvedValue({
        id: OTHER_ID,
        cleanupPending: false,
    });
    await expect(
        deleteSkill({ id: ID, expectedRevision: 1 }),
    ).rejects.toMatchObject({ code: "invalid_response" });
    // Neither is a malformed one; the UI never guesses a cleanup state.
    vi.mocked(invoke).mockResolvedValue({ id: ID, cleanupPending: "yes" });
    await expect(
        deleteSkill({ id: ID, expectedRevision: 1 }),
    ).rejects.toMatchObject({ code: "invalid_response" });
    vi.mocked(invoke).mockResolvedValue({
        id: ID,
        cleanupPending: false,
        extra: true,
    });
    await expect(
        deleteSkill({ id: ID, expectedRevision: 1 }),
    ).rejects.toMatchObject({ code: "invalid_response" });
});

test("parked-file cleanup reports honest booleans", async () => {
    vi.mocked(invoke).mockResolvedValue(true);
    expect(await cleanupDeletedSkillCopies()).toBe(true);
    expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "cleanup_deleted_skill_copies",
        {},
    );
    vi.mocked(invoke).mockResolvedValue(false);
    expect(await cleanupDeletedSkillCopies()).toBe(false);
    vi.mocked(invoke).mockResolvedValue("true");
    await expect(cleanupDeletedSkillCopies()).rejects.toMatchObject({
        code: "invalid_response",
    });
});

test("domain codes are kept and unknown failures are generic", async () => {
    vi.mocked(invoke).mockRejectedValue("no_skills_found");
    await expect(importSkillFolder("/tmp/sources/demo")).rejects.toMatchObject({
        code: "no_skills_found",
    });
    vi.mocked(invoke).mockRejectedValue(new Error("transport"));
    await expect(importSkillFolder("/tmp/sources/demo")).rejects.toMatchObject({
        code: "operation_failed",
    });
    vi.mocked(invoke).mockRejectedValue("not_a_documented_code");
    await expect(importSkillFolder("/tmp/sources/demo")).rejects.toMatchObject({
        code: "operation_failed",
    });
    await expect(importSkillFolder("/tmp/sources/demo")).rejects.toBeInstanceOf(
        SkillRequestError,
    );
});

const COMMIT = "a".repeat(40);
const GIT_COMMAND = `git fetch --depth 1 https://github.com/example/skills v1.2.0 && git checkout --detach FETCH_HEAD`;
const NPX_COMMAND =
    "npx --yes skills@1.7.4 add https://github.com/example/skills --skill '*' --copy -g -a claude-code -y";

test("an install preview is validated before it can be shown", async () => {
    const preview = {
        method: "git",
        sourceUrl: "https://github.com/example/skills",
        revision: "v1.2.0",
        installer: "git",
        installerVersion: null,
        command: GIT_COMMAND,
    };
    vi.mocked(invoke).mockResolvedValue(preview);
    const source = {
        method: "git" as const,
        source: {
            repository: "example/skills",
            revision: "v1.2.0",
            skill: null,
        },
    };
    await expect(previewSkillInstall(source)).resolves.toEqual({
        kind: "desktop",
        preview,
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("preview_skill_install", {
        source,
    });

    const malformed: unknown[] = [
        { ...preview, installer: "npx" },
        { ...preview, revision: null },
        { ...preview, command: "git clone something" },
        { ...preview, command: "bad\u0000command" },
        {
            ...preview,
            method: "npx",
            installer: "skills",
            installerVersion: null,
            revision: null,
            command: NPX_COMMAND,
        },
        {
            ...preview,
            method: "npx",
            installer: "skills",
            installerVersion: "1.7.4",
            revision: null,
            command: NPX_COMMAND,
            extra: true,
        },
    ];
    for (const value of malformed) {
        vi.mocked(invoke).mockResolvedValue(value);
        await expect(previewSkillInstall(source)).rejects.toMatchObject({
            code: "invalid_response",
        });
    }

    vi.mocked(invoke).mockResolvedValue({
        method: "npx",
        sourceUrl: "https://github.com/example/skills",
        revision: null,
        installer: "skills",
        installerVersion: "1.7.4",
        command: NPX_COMMAND,
    });
    await expect(
        previewSkillInstall({
            method: "npx",
            source: {
                source: "example/skills",
                installerVersion: null,
                skill: null,
            },
        }),
    ).resolves.toMatchObject({
        kind: "desktop",
        preview: { method: "npx", command: NPX_COMMAND },
    });
});

test("browser preview never invents an install command", async () => {
    vi.mocked(isTauri).mockReturnValue(false);
    await expect(
        previewSkillInstall({
            method: "git",
            source: {
                repository: "example/skills",
                revision: "v1",
                skill: null,
            },
        }),
    ).resolves.toEqual({ kind: "preview" });
    await expect(
        installSkill({
            method: "git",
            source: {
                repository: "example/skills",
                revision: "v1",
                skill: null,
            },
        }),
    ).rejects.toMatchObject({ code: "desktop_required" });
    expect(vi.mocked(invoke)).not.toHaveBeenCalled();
});

test("an install report must describe the reviewed command and rows", async () => {
    const imported: SkillRecord = {
        ...row,
        originPath: null,
        sourceUrl: "https://github.com/example/skills",
        sourceRevision: COMMIT,
        fetchMethod: "git",
        installerVersion: null,
        installerCommand: GIT_COMMAND,
        revision: 2,
    };
    const report = {
        method: "git",
        sourceUrl: "https://github.com/example/skills",
        revision: COMMIT,
        installer: "git",
        installerVersion: null,
        command: GIT_COMMAND,
        exitStatus: 0,
        imported: [imported],
        skipped: [],
    };
    const source = {
        method: "git" as const,
        source: {
            repository: "example/skills",
            revision: "v1.2.0",
            skill: null,
        },
    };
    vi.mocked(invoke).mockResolvedValue(report);
    await expect(installSkill(source)).resolves.toEqual(report);
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("install_skill", { source });

    const npxImported: SkillRecord = {
        ...imported,
        sourceRevision: null,
        fetchMethod: "npx",
        installerVersion: "1.7.4",
        installerCommand: NPX_COMMAND,
    };
    const npxReport = {
        ...report,
        method: "npx",
        revision: null,
        installer: "skills",
        installerVersion: "1.7.4",
        command: NPX_COMMAND,
        imported: [npxImported],
    };
    vi.mocked(invoke).mockResolvedValue(npxReport);
    await expect(
        installSkill({
            method: "npx",
            source: {
                source: "example/skills",
                installerVersion: null,
                skill: null,
            },
        }),
    ).resolves.toEqual(npxReport);

    const malformed: unknown[] = [
        { ...report, imported: [{ ...imported, originPath: "/tmp/source" }] },
        {
            ...report,
            imported: [{ ...imported, sourceUrl: "https://example.com" }],
        },
        { ...report, imported: [{ ...imported, fetchMethod: "npx" }] },
        {
            ...report,
            imported: [{ ...imported, installerCommand: NPX_COMMAND }],
        },
        { ...report, exitStatus: 1 },
        { ...report, installer: "npx" },
        { ...report, installerVersion: "9.9.9" },
        { ...npxReport, installerVersion: "9.9.9" },
        { ...report, imported: [], skipped: [] },
        { ...report, imported: [imported, imported] },
        { ...report, revision: "b".repeat(40) },
        { ...report, extra: true },
    ];
    for (const value of malformed) {
        vi.mocked(invoke).mockResolvedValue(value);
        await expect(installSkill(source)).rejects.toMatchObject({
            code: "invalid_response",
        });
    }

    vi.mocked(invoke).mockRejectedValue("installer_failed");
    await expect(installSkill(source)).rejects.toMatchObject({
        code: "installer_failed",
    });
});

const marketplaceRow: MarketplaceSkill = {
    id: "mattpocock/skills/tdd",
    source: "mattpocock/skills",
    skillId: "tdd",
    name: "tdd",
    installs: 42,
    description: null,
    summary: null,
    catalogInstallCommand: null,
};
const MARKETPLACE_COMMAND =
    "git fetch --depth 1 https://github.com/mattpocock/skills HEAD && git checkout --detach FETCH_HEAD";

test("marketplace browse and search validate catalog rows", async () => {
    vi.mocked(invoke).mockResolvedValue([marketplaceRow]);
    await expect(listMarketplaceSkills("trending")).resolves.toEqual({
        kind: "desktop",
        skills: [marketplaceRow],
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("list_marketplace_skills", {
        board: "trending",
    });

    vi.mocked(invoke).mockResolvedValue([marketplaceRow]);
    await expect(searchMarketplaceSkills("td")).resolves.toEqual({
        kind: "desktop",
        skills: [marketplaceRow],
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "search_marketplace_skills",
        { query: "td", limit: 40 },
    );

    await expect(searchMarketplaceSkills("a")).rejects.toMatchObject({
        code: "invalid_request",
    });

    vi.mocked(invoke).mockResolvedValue([
        { ...marketplaceRow, id: "wrong/id" },
    ]);
    await expect(listMarketplaceSkills("all_time")).rejects.toMatchObject({
        code: "invalid_response",
    });

    vi.mocked(isTauri).mockReturnValue(false);
    vi.mocked(invoke).mockClear();
    await expect(listMarketplaceSkills("hot")).resolves.toEqual({
        kind: "preview",
    });
    await expect(searchMarketplaceSkills("td")).resolves.toEqual({
        kind: "preview",
    });
    expect(vi.mocked(invoke)).not.toHaveBeenCalled();
});

test("marketplace catalog IPC that never settles times out", async () => {
    vi.useFakeTimers();
    try {
        vi.mocked(invoke).mockImplementation(
            () =>
                new Promise(() => {
                    /* never settles */
                }),
        );
        const pending = listMarketplaceSkills("all_time");
        const assertion = expect(pending).rejects.toMatchObject({
            code: "marketplace_timed_out",
        });
        await vi.advanceTimersByTimeAsync(25_000);
        await assertion;
    } finally {
        vi.useRealTimers();
    }
});

test("marketplace skill detail requires a real description", async () => {
    const detailed: MarketplaceSkill = {
        ...marketplaceRow,
        description: "Test-driven development skill from the catalog page.",
        summary:
            "Test-driven development with vertical slices and behavior-focused tests.",
        catalogInstallCommand:
            "npx skills add https://github.com/mattpocock/skills --skill tdd",
        installs: 1_161_230,
    };
    vi.mocked(invoke).mockResolvedValue(detailed);
    await expect(
        getMarketplaceSkillDetail("mattpocock/skills", "tdd"),
    ).resolves.toEqual({ kind: "desktop", skill: detailed });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "get_marketplace_skill_detail",
        { source: "mattpocock/skills", skillId: "tdd" },
    );

    vi.mocked(invoke).mockResolvedValue({
        ...marketplaceRow,
        description: null,
    });
    await expect(
        getMarketplaceSkillDetail("mattpocock/skills", "tdd"),
    ).rejects.toMatchObject({ code: "invalid_response" });

    vi.mocked(invoke).mockResolvedValue({
        ...marketplaceRow,
        description: "ok",
        catalogInstallCommand: "curl evil.example",
    });
    await expect(
        getMarketplaceSkillDetail("mattpocock/skills", "tdd"),
    ).rejects.toMatchObject({ code: "invalid_response" });

    await expect(getMarketplaceSkillDetail("a", "tdd")).rejects.toMatchObject({
        code: "invalid_request",
    });

    vi.mocked(isTauri).mockReturnValue(false);
    vi.mocked(invoke).mockClear();
    await expect(
        getMarketplaceSkillDetail("mattpocock/skills", "tdd"),
    ).resolves.toEqual({ kind: "preview" });
    expect(vi.mocked(invoke)).not.toHaveBeenCalled();
});

test("marketplace skill detail accepts null installs and optional enrichments", async () => {
    const detailed: MarketplaceSkill = {
        ...marketplaceRow,
        installs: null,
        description: "Meta description only.",
        summary: null,
        catalogInstallCommand: null,
    };
    vi.mocked(invoke).mockResolvedValue(detailed);
    await expect(
        getMarketplaceSkillDetail("mattpocock/skills", "tdd"),
    ).resolves.toEqual({ kind: "desktop", skill: detailed });
});

test("marketplace install reviews HEAD and rejects a non-git preview", async () => {
    vi.mocked(invoke).mockResolvedValue({
        method: "git",
        sourceUrl: "https://github.com/mattpocock/skills",
        revision: "HEAD",
        installer: "git",
        installerVersion: null,
        command: MARKETPLACE_COMMAND,
    });
    await expect(
        previewMarketplaceInstall({
            source: "mattpocock/skills",
            skillId: "tdd",
        }),
    ).resolves.toMatchObject({
        kind: "desktop",
        preview: { revision: "HEAD", command: MARKETPLACE_COMMAND },
    });

    vi.mocked(invoke).mockResolvedValue({
        method: "git",
        sourceUrl: "https://github.com/mattpocock/skills",
        revision: "main",
        installer: "git",
        installerVersion: null,
        command: MARKETPLACE_COMMAND.replace("HEAD", "main"),
    });
    await expect(
        previewMarketplaceInstall({
            source: "mattpocock/skills",
            skillId: "tdd",
        }),
    ).rejects.toMatchObject({ code: "invalid_response" });

    const imported: SkillRecord = {
        ...row,
        originPath: null,
        sourceUrl: "https://github.com/mattpocock/skills",
        sourceRevision: COMMIT,
        fetchMethod: "git",
        installerVersion: null,
        installerCommand: MARKETPLACE_COMMAND,
    };
    vi.mocked(invoke).mockResolvedValue({
        method: "git",
        sourceUrl: "https://github.com/mattpocock/skills",
        revision: COMMIT,
        installer: "git",
        installerVersion: null,
        command: MARKETPLACE_COMMAND,
        exitStatus: 0,
        imported: [imported],
        skipped: [],
    });
    await expect(
        installMarketplaceSkill({
            source: "mattpocock/skills",
            skillId: "tdd",
        }),
    ).resolves.toMatchObject({
        revision: COMMIT,
        imported: [imported],
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "install_marketplace_skill",
        { request: { source: "mattpocock/skills", skillId: "tdd" } },
    );

    vi.mocked(invoke).mockRejectedValue("marketplace_source_unsupported");
    await expect(
        installMarketplaceSkill({
            source: "mattpocock/skills",
            skillId: "tdd",
        }),
    ).rejects.toMatchObject({ code: "marketplace_source_unsupported" });
});
