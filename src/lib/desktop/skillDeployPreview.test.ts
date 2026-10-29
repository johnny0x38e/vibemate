import { invoke, isTauri } from "@tauri-apps/api/core";
import { beforeEach, expect, test, vi } from "vitest";
import { previewSkillDeploy, SkillRequestError } from "./skills";
vi.mock(import("@tauri-apps/api/core"));

const ID = "0123456789abcdef0123456789abcdef";
const base = {
    agentId: "codex",
    skillId: ID,
    skillName: "demo-skill",
    skillRevision: 1,
    contentDigest: "ab".repeat(32),
    configRoot: "/test/codex",
    targetPath: "/test/codex/skills/demo-skill",
    targetName: "demo-skill",
    targetState: "absent",
    symlinkAvailable: true,
    suggestedMode: "symlink",
    canApply: true,
};
beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(isTauri).mockReturnValue(true);
});

async function code(
    response: unknown,
    agentId: "codex" | "pi" = "codex",
): Promise<string> {
    vi.mocked(invoke).mockResolvedValueOnce(response);
    try {
        await previewSkillDeploy(agentId, ID);
    } catch (error: unknown) {
        if (error instanceof SkillRequestError) return error.code;
    }
    return "no error";
}

test("an omitted shared duplicate is normalized to null", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(base);
    const result = await previewSkillDeploy("codex", ID);
    expect(
        result.kind === "desktop" && result.preview.sharedDuplicatePath,
    ).toBe(null);
});

test("a Codex shared duplicate path is passed through", async () => {
    const path = "/test/home/.agents/skills/demo-skill";
    vi.mocked(invoke).mockResolvedValueOnce({
        ...base,
        sharedDuplicatePath: path,
    });
    const result = await previewSkillDeploy("codex", ID);
    expect(
        result.kind === "desktop" && result.preview.sharedDuplicatePath,
    ).toBe(path);
});

test("contract violations around the shared duplicate are refused", async () => {
    // Rust omits the key instead of sending null.
    expect(await code({ ...base, sharedDuplicatePath: null })).toBe(
        "invalid_response",
    );
    expect(await code({ ...base, sharedDuplicatePath: "" })).toBe(
        "invalid_response",
    );
    expect(await code({ ...base, sharedDuplicatePath: "/a\nb" })).toBe(
        "invalid_response",
    );
    // Only Codex loads `~/.agents/skills`.
    expect(
        await code(
            { ...base, agentId: "pi", sharedDuplicatePath: "/test/x" },
            "pi",
        ),
    ).toBe("invalid_response");
    expect(await code({ ...base, shared_duplicate_path: "/test/x" })).toBe(
        "invalid_response",
    );
    expect(await code({ ...base, agentId: "pi" }, "pi")).toBe("no error");
});

test("the resync backup facts are normalized and travel together", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(base);
    const plain = await previewSkillDeploy("codex", ID);
    expect(plain.kind === "desktop" && plain.preview).toMatchObject({
        overwritesUserChanges: false,
        backupDir: null,
    });
    const backupDir = "/test/app/skill-deploy-backups/codex/" + ID;
    const edited = {
        ...base,
        targetState: "foreign",
        canApply: false,
        overwritesUserChanges: true,
        backupDir,
    };
    vi.mocked(invoke).mockResolvedValueOnce(edited);
    const result = await previewSkillDeploy("codex", ID);
    expect(result.kind === "desktop" && result.preview).toMatchObject({
        overwritesUserChanges: true,
        backupDir,
    });
    // Rust omits both keys when false and never sends one without the other.
    expect(await code({ ...edited, overwritesUserChanges: false })).toBe(
        "invalid_response",
    );
    expect(await code({ ...base, backupDir })).toBe("invalid_response");
    expect(await code({ ...base, overwritesUserChanges: true })).toBe(
        "invalid_response",
    );
    expect(await code({ ...edited, backupDir: null })).toBe("invalid_response");
    expect(await code({ ...edited, backupDir: "/a\nb" })).toBe(
        "invalid_response",
    );
});
