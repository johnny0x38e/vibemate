import type { SkillDeployPreview } from "../lib/desktop/skills";
import type {
    AgentSkillTarget,
    SkillResynced,
} from "../lib/desktop/skillResync";

/** Synthetic library skill identity used by resync tests. */
export const resyncSkillId = "0123456789abcdef0123456789abcdef";
const configRoot = "/test/home/.claude";
const targetPath = "/test/home/.claude/skills/demo-skill";

/** Preview of a recorded copy that was edited outside vibemate. */
export const editedCopyPreview: SkillDeployPreview = {
    agentId: "claudeCode",
    skillId: resyncSkillId,
    skillName: "demo-skill",
    skillRevision: 3,
    contentDigest: "d".repeat(64),
    configRoot,
    targetPath,
    targetName: "demo-skill",
    targetState: "foreign",
    symlinkAvailable: true,
    suggestedMode: "symlink",
    canApply: false,
    overwritesUserChanges: true,
    backupDir:
        "/test/app/skill-deploy-backups/claudeCode/0123456789abcdef0123456789abcdef",
};

/** Rust answer after the edited copy was rewritten. */
export const resyncedCopy: SkillResynced<"claudeCode"> = {
    agentId: "claudeCode",
    skillId: resyncSkillId,
    skillName: "demo-skill",
    skillRevision: 3,
    mode: "copy",
    targetPath,
    overwroteUserChanges: true,
    backupPath:
        "/test/app/skill-deploy-backups/claudeCode/0123456789abcdef0123456789abcdef/1700000000001-" +
        "a".repeat(32),
};

/**
 * Batch preview rows as Rust sends them (no `sharedDuplicatePath` key without
 * a Codex duplicate): owned, occupied by a user folder, and free.
 */
export const agentSkillTargets: Omit<
    AgentSkillTarget,
    "sharedDuplicatePath"
>[] = [
    {
        skillId: resyncSkillId,
        targetState: "owned_copy",
        targetPath,
        symlinkAvailable: true,
    },
    {
        skillId: "1".repeat(32),
        targetState: "foreign",
        targetPath: "/test/home/.claude/skills/occupied",
        symlinkAvailable: true,
    },
    {
        skillId: "2".repeat(32),
        targetState: "absent",
        targetPath: "/test/home/.claude/skills/free",
        symlinkAvailable: true,
    },
];
