import {
    cleanup,
    fireEvent,
    render,
    screen,
    waitFor,
    within,
} from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createAppI18n } from "../../i18n";
import type { AgentId } from "../../lib/desktop/agents";
import {
    applySkillDeploy,
    disableSkillDeploy,
    listAgentSkillDeployments,
    listSkills,
    previewSkillDeploy,
    SkillRequestError,
    type SkillDeployAgent,
    type SkillDeployPreview,
    type SkillDeploymentRecord,
    type SkillRecord,
} from "../../lib/desktop/skills";
import {
    previewAgentSkillTargets,
    resyncSkillDeploy,
} from "../../lib/desktop/skillResync";
import { AgentSkillPanel } from "./AgentSkillPanel";

vi.mock(import("../../lib/desktop/skills"), async (importOriginal) => ({
    ...(await importOriginal()),
    listSkills: vi.fn(),
    listAgentSkillDeployments: vi.fn(),
    previewSkillDeploy: vi.fn(),
    applySkillDeploy: vi.fn(),
    disableSkillDeploy: vi.fn(),
}));
const library = vi.mocked(listSkills);
const deployments = vi.mocked(listAgentSkillDeployments);
const previewDeploy = vi.mocked(previewSkillDeploy);
const apply = vi.mocked(applySkillDeploy);
const disable = vi.mocked(disableSkillDeploy);
vi.mock(import("../../lib/desktop/skillResync"), async (importOriginal) => ({
    ...(await importOriginal()),
    resyncSkillDeploy: vi.fn(),
    previewAgentSkillTargets: vi.fn(),
}));
const resync = vi.mocked(resyncSkillDeploy);
const batchTargets = vi.mocked(previewAgentSkillTargets);

function skill(id: string, name: string): SkillRecord {
    return {
        id,
        name,
        description: `${name} description`,
        originPath: null,
        sourceUrl: null,
        sourceRevision: null,
        fetchMethod: null,
        installerVersion: null,
        installerCommand: null,
        remoteRevision: null,
        updateCheckStatus: null,
        lastCheckedAtMs: null,
        lastCheckError: null,
        contentDigest: "ab".repeat(32),
        fileCount: 1,
        byteSize: 10,
        revision: 3,
        importedAtMs: 1,
        updatedAtMs: 1,
    };
}
const alpha = skill("a".repeat(32), "alpha");
const beta = skill("b".repeat(32), "beta");
const gamma = skill("c".repeat(32), "gamma");

function preview(
    agentId: SkillDeployAgent,
    record: SkillRecord,
    targetState: SkillDeployPreview["targetState"],
): SkillDeployPreview {
    return {
        agentId,
        skillId: record.id,
        skillName: record.name,
        skillRevision: record.revision,
        contentDigest: record.contentDigest,
        configRoot: "/home/test/.claude",
        targetPath: `/home/test/.claude/skills/${record.name}`,
        targetName: record.name,
        targetState,
        symlinkAvailable: true,
        suggestedMode: "symlink",
        canApply: targetState !== "foreign",
    };
}

function placed(
    record: SkillRecord,
    overrides: Partial<SkillDeploymentRecord> = {},
): SkillDeploymentRecord {
    return {
        skillId: record.id,
        skillName: record.name,
        agentId: "claudeCode",
        configRoot: "/home/test/.claude",
        mode: "symlink",
        skillRevision: record.revision,
        contentDigest: record.contentDigest,
        inSync: true,
        deployedAtMs: 1,
        ...overrides,
    };
}

/** Live target state per skill id for the next previews. */
let targets: Record<string, SkillDeployPreview["targetState"]>;

async function mount(agentId: AgentId = "claudeCode", hidden = false) {
    const i18n = await createAppI18n("en");
    const onOpenSkills = vi.fn();
    const view = render(
        <I18nextProvider i18n={i18n}>
            <AgentSkillPanel
                agentId={agentId}
                hidden={hidden}
                onOpenSkills={onOpenSkills}
            />
        </I18nextProvider>,
    );
    return { ...view, onOpenSkills, i18n };
}

function skillRow(name: string): Promise<HTMLElement> {
    return screen.findByRole("listitem", { name: `Claude Code · ${name}` });
}

beforeEach(() => {
    targets = {};
    library.mockReset().mockResolvedValue({
        kind: "desktop",
        page: { items: [alpha, beta, gamma], nextCursor: null, total: 3 },
    });
    deployments.mockReset().mockResolvedValue({ kind: "desktop", items: [] });
    previewDeploy.mockReset().mockImplementation((agentId, skillId) => {
        const record = [alpha, beta, gamma].find((row) => row.id === skillId);
        if (record === undefined) throw new Error("unknown skill");
        return Promise.resolve({
            kind: "desktop" as const,
            preview: preview(agentId, record, targets[skillId] ?? "absent"),
        });
    });
    apply.mockReset().mockImplementation((input) =>
        Promise.resolve({
            agentId: input.agentId,
            skillId: input.skillId,
            skillName: "x",
            skillRevision: input.expectedSkillRevision,
            mode: input.mode,
            targetPath: "/home/test/.claude/skills/x",
        }),
    );
    // Browser answer by default: rows fall back to per-skill previews.
    batchTargets.mockReset().mockResolvedValue({ kind: "preview" });
    resync.mockReset().mockImplementation((input) =>
        Promise.resolve({
            agentId: input.agentId,
            skillId: input.skillId,
            skillName: "x",
            skillRevision: input.expectedSkillRevision,
            mode: input.mode,
            targetPath: input.reviewedTargetPath,
            overwroteUserChanges: false,
            backupPath: null,
        }),
    );
    disable.mockReset().mockImplementation((input) =>
        Promise.resolve({
            agentId: input.agentId,
            skillId: input.skillId,
            targetPath: "/home/test/.claude/skills/x",
            targetRemoved: true,
            leftForeign: false,
        }),
    );
});
afterEach(() => {
    cleanup();
});

test("every library skill gets a row with its status for this Agent", async () => {
    targets[beta.id] = "absent";
    deployments.mockResolvedValue({
        kind: "desktop",
        items: [placed(alpha), placed(beta, { inSync: false })],
    });
    await mount();
    const list = await screen.findByRole("list", { name: "Library skills" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(3);

    const a = await skillRow("alpha");
    expect(within(a).getByText("Injected")).toBeDefined();
    expect(
        within(a).getByRole("button", {
            name: "Remove alpha from Claude Code",
        }),
    ).toBeDefined();

    const b = await skillRow("beta");
    expect(await within(b).findByText("Needs sync")).toBeDefined();
    expect(
        within(b).getByRole("button", {
            name: "Resync beta in Claude Code",
        }),
    ).toBeDefined();

    const c = await skillRow("gamma");
    expect(within(c).getByText("Not injected")).toBeDefined();
    // Only the out-of-sync row needed a live preview.
    expect(previewDeploy).toHaveBeenCalledTimes(1);
    expect(previewDeploy).toHaveBeenCalledWith("claudeCode", beta.id);
});

test("inject previews the target, then applies it with an automatic link", async () => {
    await mount();
    const c = await skillRow("gamma");
    deployments.mockResolvedValue({ kind: "desktop", items: [placed(gamma)] });
    fireEvent.click(
        within(c).getByRole("button", {
            name: "Inject gamma into Claude Code",
        }),
    );
    await waitFor(() => {
        expect(apply).toHaveBeenCalledWith({
            agentId: "claudeCode",
            skillId: gamma.id,
            expectedSkillRevision: 3,
            configRoot: "/home/test/.claude",
            mode: "symlink",
            targetState: "absent",
        });
    });
    expect(await within(c).findByText("Injected")).toBeDefined();
});

test("an occupied name is reported without writing anything", async () => {
    targets[gamma.id] = "foreign";
    await mount();
    const c = await skillRow("gamma");
    fireEvent.click(
        within(c).getByRole("button", {
            name: "Inject gamma into Claude Code",
        }),
    );
    expect(await within(c).findByText("Occupied")).toBeDefined();
    expect(
        within(c).getByText(
            "A folder vibemate does not manage already uses this name. It is left untouched.",
        ),
    ).toBeDefined();
    expect(apply).not.toHaveBeenCalled();
    expect(
        within(c).queryByRole("button", { name: /Inject gamma/u }),
    ).toBeNull();
});

test("remove confirms first, then stops the placement", async () => {
    deployments.mockResolvedValue({ kind: "desktop", items: [placed(alpha)] });
    await mount();
    const a = await skillRow("alpha");
    fireEvent.click(
        within(a).getByRole("button", {
            name: "Remove alpha from Claude Code",
        }),
    );
    const dialog = await screen.findByRole("dialog", {
        name: "Remove from Claude Code?",
    });
    expect(disable).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
    await waitFor(() => {
        expect(disable).toHaveBeenCalledWith({
            agentId: "claudeCode",
            skillId: alpha.id,
            configRoot: "/home/test/.claude",
        });
    });
});

test("resync uses the recorded mode for a deleted placement", async () => {
    deployments.mockResolvedValue({
        kind: "desktop",
        items: [placed(beta, { inSync: false, mode: "copy" })],
    });
    await mount();
    const b = await skillRow("beta");
    fireEvent.click(
        await within(b).findByRole("button", {
            name: "Resync beta in Claude Code",
        }),
    );
    // Resync always asks first; nothing the user edited is overwritten here.
    const confirm = await screen.findByRole("dialog", {
        name: "Resync in Claude Code?",
    });
    expect(within(confirm).queryByRole("note")).toBeNull();
    expect(resync).not.toHaveBeenCalled();
    fireEvent.click(within(confirm).getByRole("button", { name: "Resync" }));
    await waitFor(() => {
        expect(resync).toHaveBeenCalledWith(
            expect.objectContaining({
                skillId: beta.id,
                mode: "copy",
                reviewedTargetState: "absent",
                reviewedTargetPath: "/home/test/.claude/skills/beta",
            }),
        );
    });
    expect(apply).not.toHaveBeenCalled();
});

test("an edited placement resyncs through the resync command", async () => {
    targets[beta.id] = "foreign";
    deployments.mockResolvedValue({
        kind: "desktop",
        items: [placed(beta, { inSync: false })],
    });
    await mount();
    const b = await skillRow("beta");
    expect(await within(b).findByText("Needs sync")).toBeDefined();
    fireEvent.click(
        await within(b).findByRole("button", {
            name: "Resync beta in Claude Code",
        }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Resync" }));
    await waitFor(() => {
        expect(resync).toHaveBeenCalledWith(
            expect.objectContaining({
                skillId: beta.id,
                mode: "symlink",
                reviewedTargetState: "foreign",
            }),
        );
    });
    expect(
        within(b).getByRole("button", { name: "Remove beta from Claude Code" }),
    ).toBeDefined();
});

test("the batch target read shows an occupied name before Inject", async () => {
    batchTargets.mockResolvedValue({
        kind: "desktop",
        items: [
            {
                skillId: gamma.id,
                targetState: "foreign",
                targetPath: "/home/test/.claude/skills/gamma",
                symlinkAvailable: true,
            },
            {
                skillId: alpha.id,
                targetState: "absent",
                targetPath: "/home/test/.claude/skills/alpha",
                symlinkAvailable: true,
            },
        ],
    });
    await mount();
    const c = await skillRow("gamma");
    expect(await within(c).findByText("Occupied")).toBeDefined();
    expect(batchTargets).toHaveBeenCalledWith("claudeCode");
    expect(
        within(c).queryByRole("button", { name: /Inject gamma/u }),
    ).toBeNull();
    const a = await skillRow("alpha");
    expect(within(a).getByText("Not injected")).toBeDefined();
    expect(apply).not.toHaveBeenCalled();
});

test("the Codex tab shows the shared-folder duplicate before Inject", async () => {
    previewDeploy.mockImplementation((agentId, skillId) => {
        const record = [alpha, beta, gamma].find((row) => row.id === skillId);
        if (record === undefined) throw new Error("unknown skill");
        return Promise.resolve({
            kind: "desktop" as const,
            preview: {
                ...preview(agentId, record, "absent"),
                sharedDuplicatePath:
                    skillId === alpha.id
                        ? "/home/test/.agents/skills/alpha"
                        : null,
            },
        });
    });
    await mount("codex");
    const a = await screen.findByRole("listitem", { name: "Codex · alpha" });
    expect(
        await within(a).findByText(
            "A skill with the same name already exists in ~/.agents/skills (/home/test/.agents/skills/alpha). Codex loads both folders and lists same-named skills separately; they are not merged.",
        ),
    ).toBeDefined();
    const b = await screen.findByRole("listitem", { name: "Codex · beta" });
    expect(within(b).queryByText(/~\/\.agents/u)).toBeNull();
    expect(batchTargets).toHaveBeenCalledWith("codex");
});

test("an unsupported Agent keeps its notice and reads nothing", async () => {
    await mount("hermes");
    expect(
        screen.getByText(
            "Skill deployment for this agent is not implemented yet.",
        ),
    ).toBeDefined();
    expect(library).not.toHaveBeenCalled();
    expect(deployments).not.toHaveBeenCalled();
});

test("a hidden tab reads nothing until it is shown", async () => {
    const { rerender, i18n, onOpenSkills } = await mount("claudeCode", true);
    expect(library).not.toHaveBeenCalled();
    rerender(
        <I18nextProvider i18n={i18n}>
            <AgentSkillPanel
                agentId="claudeCode"
                hidden={false}
                onOpenSkills={onOpenSkills}
            />
        </I18nextProvider>,
    );
    expect(await skillRow("alpha")).toBeDefined();
    expect(deployments).toHaveBeenCalledWith("claudeCode");
});

test("an empty library offers the Skills module", async () => {
    library.mockResolvedValue({
        kind: "desktop",
        page: { items: [], nextCursor: null, total: 0 },
    });
    const { onOpenSkills } = await mount();
    fireEvent.click(await screen.findByRole("button", { name: "Open Skills" }));
    expect(onOpenSkills).toHaveBeenCalledTimes(1);
});

test("a target the batch read could not inspect is unreadable, not occupied", async () => {
    batchTargets.mockResolvedValue({
        kind: "desktop",
        items: [
            {
                skillId: gamma.id,
                targetState: "unreadable",
                targetPath: "/home/test/.claude/skills/gamma",
                symlinkAvailable: true,
            },
        ],
    });
    await mount();
    const c = await skillRow("gamma");
    expect(await within(c).findByText("Can't read target")).toBeDefined();
    expect(within(c).queryByText("Occupied")).toBeNull();
    expect(
        within(c).getByText(
            "vibemate couldn't read what is at this skill's target, so it won't inject here. Check the folder's permissions, then reload.",
        ),
    ).toBeDefined();
    expect(
        within(c).queryByRole("button", { name: /Inject gamma/u }),
    ).toBeNull();
    expect(
        within(c).getByText("Can't read target").getAttribute("data-state"),
    ).toBe("unreadable");
});

test("the unreadable state has its own zh-CN copy", async () => {
    batchTargets.mockResolvedValue({
        kind: "desktop",
        items: [
            {
                skillId: gamma.id,
                targetState: "unreadable",
                targetPath: "/home/test/.claude/skills/gamma",
                symlinkAvailable: true,
            },
        ],
    });
    const { i18n } = await mount();
    await i18n.changeLanguage("zh-CN");
    const c = await screen.findByRole("listitem", {
        name: "Claude Code · gamma",
    });
    expect(await within(c).findByText("无法读取目标")).toBeDefined();
    expect(within(c).queryByText("已被占用")).toBeNull();
});

test("resyncing edited files confirms the overwrite and reports the backup", async () => {
    targets[beta.id] = "foreign";
    deployments.mockResolvedValue({
        kind: "desktop",
        items: [placed(beta, { inSync: false })],
    });
    const backupDir = "/test/app/skill-deploy-backups/claudeCode/" + beta.id;
    previewDeploy.mockImplementation((agentId, skillId) => {
        const record = [alpha, beta, gamma].find((row) => row.id === skillId);
        if (record === undefined) throw new Error("unknown skill");
        const state = targets[skillId] ?? "absent";
        return Promise.resolve({
            kind: "desktop" as const,
            preview: {
                ...preview(agentId, record, state),
                ...(state === "foreign"
                    ? { overwritesUserChanges: true, backupDir }
                    : {}),
            },
        });
    });
    resync.mockImplementation((input) =>
        Promise.resolve({
            agentId: input.agentId,
            skillId: input.skillId,
            skillName: "beta",
            skillRevision: input.expectedSkillRevision,
            mode: input.mode,
            targetPath: input.reviewedTargetPath,
            overwroteUserChanges: true,
            backupPath: `${backupDir}/1700000000001-${"a".repeat(32)}`,
        }),
    );
    await mount();
    const b = await skillRow("beta");
    fireEvent.click(
        await within(b).findByRole("button", {
            name: "Resync beta in Claude Code",
        }),
    );
    const confirm = await screen.findByRole("dialog", {
        name: "Resync in Claude Code?",
    });
    expect(within(confirm).getByRole("note").textContent).toBe(
        `This overwrites your changes in /home/test/.claude/skills/beta. They are backed up to ${backupDir} first.`,
    );
    fireEvent.click(
        within(confirm).getByRole("button", { name: "Back up and overwrite" }),
    );
    expect(
        await screen.findByText(
            `Resynced beta. Your changes were backed up to ${backupDir}/1700000000001-${"a".repeat(32)}.`,
        ),
    ).toBeDefined();
    expect(resync).toHaveBeenCalledTimes(1);
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
    });
});

test("cancelling the resync confirmation writes nothing", async () => {
    deployments.mockResolvedValue({
        kind: "desktop",
        items: [placed(beta, { inSync: false })],
    });
    await mount();
    const b = await skillRow("beta");
    fireEvent.click(
        await within(b).findByRole("button", {
            name: "Resync beta in Claude Code",
        }),
    );
    const confirm = await screen.findByRole("dialog", {
        name: "Resync in Claude Code?",
    });
    fireEvent.click(within(confirm).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(resync).not.toHaveBeenCalled();
});

const TARGET_READ_FAILED =
    "Can't read the injected copy in the Agent folder. Check the folder's permissions, then try again.";
const LIBRARY_READ_FAILED = "The skill library could not be read.";

test("inject whose target preview cannot be read names the injected copy", async () => {
    await mount();
    const c = await skillRow("gamma");
    previewDeploy.mockRejectedValue(new SkillRequestError("read_failed"));
    fireEvent.click(
        within(c).getByRole("button", {
            name: "Inject gamma into Claude Code",
        }),
    );
    expect((await within(c).findByRole("alert")).textContent).toBe(
        TARGET_READ_FAILED,
    );
    expect(apply).not.toHaveBeenCalled();
});

test("inject whose apply cannot read the library keeps the library copy", async () => {
    apply.mockRejectedValue(new SkillRequestError("read_failed"));
    await mount();
    const c = await skillRow("gamma");
    fireEvent.click(
        within(c).getByRole("button", {
            name: "Inject gamma into Claude Code",
        }),
    );
    expect((await within(c).findByRole("alert")).textContent).toBe(
        LIBRARY_READ_FAILED,
    );
});

test("resync and remove that cannot read the injected copy name the copy", async () => {
    resync.mockRejectedValue(new SkillRequestError("read_failed"));
    disable.mockRejectedValue(new SkillRequestError("read_failed"));
    deployments.mockResolvedValue({
        kind: "desktop",
        items: [placed(alpha), placed(beta, { inSync: false, mode: "copy" })],
    });
    await mount();
    const b = await skillRow("beta");
    fireEvent.click(
        await within(b).findByRole("button", {
            name: "Resync beta in Claude Code",
        }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Resync" }));
    expect((await within(b).findByRole("alert")).textContent).toBe(
        TARGET_READ_FAILED,
    );

    const a = await skillRow("alpha");
    fireEvent.click(
        within(a).getByRole("button", {
            name: "Remove alpha from Claude Code",
        }),
    );
    const dialog = await screen.findByRole("dialog", {
        name: "Remove from Claude Code?",
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
    expect((await within(a).findByRole("alert")).textContent).toBe(
        TARGET_READ_FAILED,
    );
});

test("a library list that cannot be read keeps the library copy", async () => {
    library.mockRejectedValue(new SkillRequestError("read_failed"));
    await mount();
    expect(await screen.findByText(LIBRARY_READ_FAILED)).toBeDefined();
    expect(screen.queryByText(TARGET_READ_FAILED)).toBeNull();
});

test("the injected-copy read failure has zh-CN copy", async () => {
    resync.mockRejectedValue(new SkillRequestError("read_failed"));
    deployments.mockResolvedValue({
        kind: "desktop",
        items: [placed(beta, { inSync: false, mode: "copy" })],
    });
    const { i18n } = await mount();
    await i18n.changeLanguage("zh-CN");
    const b = await screen.findByRole("listitem", {
        name: "Claude Code · beta",
    });
    fireEvent.click(
        await within(b).findByRole("button", {
            name: "在 Claude Code 中重新同步 beta",
        }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "重新同步" }));
    expect((await within(b).findByRole("alert")).textContent).toBe(
        "无法读取已注入的副本。请检查 Agent 文件夹的权限后重试。",
    );
});
