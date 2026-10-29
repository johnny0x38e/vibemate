import { expect, test } from "vitest";
import {
    SkillRequestError,
    type SkillDeployPreview,
    type SkillDeploymentRecord,
} from "../../lib/desktop/skills";
import {
    editedCopyPreview,
    resyncedCopy,
} from "../../test/skillResyncFixtures";
import {
    actionErrorScope,
    batchStatus,
    canResync,
    chooseDeployMode,
    countInjectedAgents,
    deploymentFor,
    editedOutside,
    injectStatus,
    primaryAction,
    resyncBackupPath,
    resyncMode,
    resyncOverwrite,
    skillErrorCode,
    skillErrorKey,
    uncertainOutcome,
} from "./skillInject";

const ID = "0123456789abcdef0123456789abcdef";

function preview(
    targetState: SkillDeployPreview["targetState"],
): SkillDeployPreview {
    return {
        agentId: "claudeCode",
        skillId: ID,
        skillName: "demo-skill",
        skillRevision: 1,
        contentDigest: "ab".repeat(32),
        configRoot: "/home/test/.claude",
        targetPath: "/home/test/.claude/skills/demo-skill",
        targetName: "demo-skill",
        targetState,
        symlinkAvailable: true,
        suggestedMode: "symlink",
        canApply: targetState !== "foreign",
    };
}

function record(
    overrides: Partial<SkillDeploymentRecord> = {},
): SkillDeploymentRecord {
    return {
        skillId: ID,
        skillName: "demo-skill",
        agentId: "claudeCode",
        configRoot: "/home/test/.claude",
        mode: "symlink",
        skillRevision: 1,
        contentDigest: "ab".repeat(32),
        inSync: true,
        deployedAtMs: 1,
        ...overrides,
    };
}

test("the ownership record decides injected and needs sync", () => {
    expect(injectStatus(preview("owned_symlink"), record())).toBe("injected");
    expect(injectStatus(null, record())).toBe("injected");
    expect(injectStatus(preview("absent"), record({ inSync: false }))).toBe(
        "needs_sync",
    );
    expect(injectStatus(preview("foreign"), record({ inSync: false }))).toBe(
        "needs_sync",
    );
});

test("without a record a foreign folder is occupied", () => {
    expect(injectStatus(preview("foreign"), null)).toBe("occupied");
    expect(injectStatus(preview("absent"), null)).toBe("not_injected");
    expect(injectStatus(null, null)).toBe("not_injected");
});

test("each status has exactly one action and occupied has none", () => {
    expect(primaryAction("not_injected")).toBe("inject");
    expect(primaryAction("injected")).toBe("remove");
    expect(primaryAction("needs_sync")).toBe("resync");
    expect(primaryAction("occupied")).toBe("none");
});

test("link mode is automatic unless Advanced asks for a copy", () => {
    expect(chooseDeployMode(true, "auto")).toBe("symlink");
    expect(chooseDeployMode(false, "auto")).toBe("copy");
    expect(chooseDeployMode(true, "copy")).toBe("copy");
    expect(resyncMode("copy", true, "auto")).toBe("copy");
    expect(resyncMode("symlink", true, "auto")).toBe("symlink");
    expect(resyncMode("symlink", false, "auto")).toBe("copy");
});

test("any recorded placement can be resynced, including edited files", () => {
    const owned = record();
    expect(canResync(preview("absent"), owned)).toBe(true);
    expect(canResync(preview("foreign"), owned)).toBe(true);
    expect(canResync(preview("foreign"), null)).toBe(false);
    expect(canResync(null, owned)).toBe(false);
    expect(editedOutside(preview("foreign"), owned)).toBe(true);
    expect(editedOutside(preview("absent"), owned)).toBe(false);
    expect(editedOutside(preview("foreign"), null)).toBe(false);
});

test("the record for the current configuration root is chosen", () => {
    const old = record({ configRoot: "/old" });
    const current = record();
    expect(
        deploymentFor([old, current], "claudeCode", "/home/test/.claude"),
    ).toBe(current);
    expect(deploymentFor([old], "pi", null)).toBeNull();
});

test("counts are distinct Agents per skill across the bulk lists", () => {
    const counts = countInjectedAgents([
        [record()],
        [record({ agentId: "pi", inSync: false })],
        [record({ agentId: "grokBuild", skillId: "other" })],
    ]);
    expect(counts.get(ID)).toBe(2);
    expect(counts.get("other")).toBe(1);
    expect(counts.get("missing")).toBeUndefined();
});

test("errors keep their safe code and uncertain outcomes ask for a reload", () => {
    expect(skillErrorCode(new SkillRequestError("target_conflict"))).toBe(
        "target_conflict",
    );
    expect(skillErrorCode(new Error("boom"))).toBe("operation_failed");
    expect(uncertainOutcome("outcome_unknown")).toBe(true);
    expect(uncertainOutcome("target_conflict")).toBe(false);
});

test("the batch read marks occupied and unreadable targets apart", () => {
    expect(batchStatus("foreign")).toBe("occupied");
    expect(batchStatus("unreadable")).toBe("unreadable");
    expect(batchStatus("absent")).toBeNull();
    expect(batchStatus("owned_copy")).toBeNull();
    expect(batchStatus(undefined)).toBeNull();
    expect(primaryAction("unreadable")).toBe("none");
});

test("a resync that replaces edits carries its backup folder and result path", () => {
    expect(resyncOverwrite(editedCopyPreview)).toEqual({
        backupDir: editedCopyPreview.backupDir,
    });
    expect(resyncOverwrite(preview("absent"))).toBeNull();
    expect(resyncBackupPath(resyncedCopy)).toBe(resyncedCopy.backupPath);
    expect(
        resyncBackupPath({
            ...resyncedCopy,
            overwroteUserChanges: false,
            backupPath: null,
        }),
    ).toBeNull();
});

test("read_failed names the injected copy only for Agent-target reads", () => {
    expect(actionErrorScope("inject")).toBe("library");
    expect(actionErrorScope("resync")).toBe("target");
    expect(actionErrorScope("remove")).toBe("target");
    expect(skillErrorKey("read_failed", "target")).toBe(
        "skills.inject.targetReadFailed",
    );
    expect(skillErrorKey("read_failed", "library")).toBe(
        "skills.errors.read_failed",
    );
    // Other codes keep their shared copy in either scope.
    expect(skillErrorKey("target_conflict", "target")).toBe(
        "skills.errors.target_conflict",
    );
});
