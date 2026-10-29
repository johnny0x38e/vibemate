import { invoke, isTauri } from "@tauri-apps/api/core";
import { beforeEach, expect, test, vi } from "vitest";
import {
    previewAgentSkillTargets,
    resyncRequestFromPreview,
    resyncSkillDeploy,
} from "./skillResync";
import { SkillRequestError } from "./skills";
import {
    agentSkillTargets,
    editedCopyPreview,
    resyncedCopy,
} from "../../test/skillResyncFixtures";
vi.mock(import("@tauri-apps/api/core"));

beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(isTauri).mockReturnValue(true);
});

async function failure(operation: () => Promise<unknown>): Promise<string> {
    try {
        await operation();
    } catch (error: unknown) {
        if (error instanceof SkillRequestError) return error.code;
        throw error;
    }
    throw new Error("expected a failure");
}

function withoutBackupPath(): Record<string, unknown> {
    const copy: Record<string, unknown> = { ...resyncedCopy };
    delete copy["backupPath"];
    return copy;
}

test("resync sends the reviewed preview values and returns the backup facts", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(resyncedCopy);
    const request = resyncRequestFromPreview(editedCopyPreview, "copy");
    const result = await resyncSkillDeploy(request);
    expect(invoke).toHaveBeenCalledWith("resync_skill_deploy", {
        request: {
            agentId: "claudeCode",
            skillId: editedCopyPreview.skillId,
            configRoot: "/test/home/.claude",
            expectedSkillRevision: 3,
            reviewedTargetPath: "/test/home/.claude/skills/demo-skill",
            reviewedTargetState: "foreign",
            mode: "copy",
        },
    });
    expect(result).toEqual(resyncedCopy);
});

test("resync maps backend codes and rejects mismatched or extra fields", async () => {
    const request = resyncRequestFromPreview(editedCopyPreview, "copy");
    for (const code of [
        "not_found",
        "target_changed",
        "target_conflict",
        "revision_conflict",
        "symlink_unavailable",
        "write_failed",
        "outcome_unknown",
        "agent_location_unavailable",
    ]) {
        vi.mocked(invoke).mockRejectedValueOnce(code);
        expect(await failure(() => resyncSkillDeploy(request))).toBe(code);
    }
    vi.mocked(invoke).mockRejectedValueOnce("/secret/path leaked");
    expect(await failure(() => resyncSkillDeploy(request))).toBe(
        "operation_failed",
    );
    vi.mocked(invoke).mockResolvedValueOnce({
        ...resyncedCopy,
        mode: "symlink",
    });
    expect(await failure(() => resyncSkillDeploy(request))).toBe(
        "invalid_response",
    );
    vi.mocked(invoke).mockResolvedValueOnce({ ...resyncedCopy, extra: true });
    expect(await failure(() => resyncSkillDeploy(request))).toBe(
        "invalid_response",
    );
    vi.mocked(invoke).mockResolvedValueOnce({
        ...resyncedCopy,
        targetPath: "/test/other",
    });
    expect(await failure(() => resyncSkillDeploy(request))).toBe(
        "invalid_response",
    );
    // The flag and the backup path travel together, and both are required.
    for (const broken of [
        { ...resyncedCopy, backupPath: null },
        { ...resyncedCopy, overwroteUserChanges: false },
        { ...resyncedCopy, backupPath: "relative/backup" },
        withoutBackupPath(),
    ]) {
        vi.mocked(invoke).mockResolvedValueOnce(broken);
        expect(await failure(() => resyncSkillDeploy(request))).toBe(
            "invalid_response",
        );
    }
    vi.mocked(invoke).mockResolvedValueOnce({
        ...resyncedCopy,
        overwroteUserChanges: false,
        backupPath: null,
    });
    expect(await resyncSkillDeploy(request)).toMatchObject({
        overwroteUserChanges: false,
        backupPath: null,
    });
});

test("resync validates the request before calling Rust and needs the desktop", async () => {
    const request = resyncRequestFromPreview(editedCopyPreview, "copy");
    expect(
        await failure(() =>
            resyncSkillDeploy({ ...request, reviewedTargetPath: "relative/x" }),
        ),
    ).toBe("invalid_request");
    expect(
        await failure(() =>
            resyncSkillDeploy({ ...request, expectedSkillRevision: 0 }),
        ),
    ).toBe("invalid_request");
    expect(invoke).not.toHaveBeenCalled();
    vi.mocked(isTauri).mockReturnValue(false);
    expect(await failure(() => resyncSkillDeploy(request))).toBe(
        "desktop_required",
    );
});

test("resync accepts Codex as an agent", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
        ...resyncedCopy,
        agentId: "codex",
        targetPath: "/test/home/.codex/skills/demo-skill",
    });
    const result = await resyncSkillDeploy({
        ...resyncRequestFromPreview(editedCopyPreview, "copy"),
        agentId: "codex",
        configRoot: "/test/home/.codex",
        reviewedTargetPath: "/test/home/.codex/skills/demo-skill",
    });
    expect(result.agentId).toBe("codex");
});

test("batch target preview is read-only IPC with strict rows", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(agentSkillTargets);
    expect(await previewAgentSkillTargets("claudeCode")).toEqual({
        kind: "desktop",
        items: agentSkillTargets.map((row) => ({
            ...row,
            sharedDuplicatePath: null,
        })),
    });
    expect(invoke).toHaveBeenCalledWith("preview_agent_skill_targets", {
        agentId: "claudeCode",
    });
    vi.mocked(invoke).mockResolvedValueOnce([
        agentSkillTargets[0],
        agentSkillTargets[0],
    ]);
    expect(await failure(() => previewAgentSkillTargets("codex"))).toBe(
        "invalid_response",
    );
    vi.mocked(invoke).mockResolvedValueOnce([
        { ...agentSkillTargets[0], targetState: "occupied" },
    ]);
    expect(await failure(() => previewAgentSkillTargets("pi"))).toBe(
        "invalid_response",
    );
    vi.mocked(isTauri).mockReturnValue(false);
    expect(await previewAgentSkillTargets("pi")).toEqual({ kind: "preview" });
});

test("batch rows carry unreadable targets and Codex shared duplicates only", async () => {
    const [row] = agentSkillTargets;
    const shared = "/test/home/.agents/skills/demo-skill";
    vi.mocked(invoke).mockResolvedValueOnce([
        { ...row, targetState: "unreadable", sharedDuplicatePath: shared },
    ]);
    expect(await previewAgentSkillTargets("codex")).toEqual({
        kind: "desktop",
        items: [
            { ...row, targetState: "unreadable", sharedDuplicatePath: shared },
        ],
    });
    for (const [agent, path] of [
        ["pi", shared],
        ["codex", null],
        ["codex", ""],
        ["codex", "/a\nb"],
    ] as const) {
        vi.mocked(invoke).mockResolvedValueOnce([
            { ...row, sharedDuplicatePath: path },
        ]);
        expect(await failure(() => previewAgentSkillTargets(agent))).toBe(
            "invalid_response",
        );
    }
});
