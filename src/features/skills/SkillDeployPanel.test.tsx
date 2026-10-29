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
import {
    applySkillDeploy,
    disableSkillDeploy,
    listSkillDeployments,
    previewSkillDeploy,
    SkillRequestError,
    type SkillDeployAgent,
    type SkillDeployPreview,
    type SkillDeploymentRecord,
    type SkillRecord,
} from "../../lib/desktop/skills";
import { resyncSkillDeploy } from "../../lib/desktop/skillResync";
import { setFieldSelectValue } from "../../test/fieldSelect";
import {
    editedCopyPreview,
    resyncedCopy,
} from "../../test/skillResyncFixtures";
import { SkillDeployPanel } from "./SkillDeployPanel";

vi.mock(import("../../lib/desktop/skills"), async (importOriginal) => ({
    ...(await importOriginal()),
    previewSkillDeploy: vi.fn(),
    listSkillDeployments: vi.fn(),
    applySkillDeploy: vi.fn(),
    disableSkillDeploy: vi.fn(),
}));
const previewDeploy = vi.mocked(previewSkillDeploy);
const listDeployments = vi.mocked(listSkillDeployments);
const apply = vi.mocked(applySkillDeploy);
const disable = vi.mocked(disableSkillDeploy);
vi.mock(import("../../lib/desktop/skillResync"), async (importOriginal) => ({
    ...(await importOriginal()),
    resyncSkillDeploy: vi.fn(),
}));
const resync = vi.mocked(resyncSkillDeploy);

const ID = "0123456789abcdef0123456789abcdef";
const skill: SkillRecord = {
    id: ID,
    name: "demo-skill",
    description: "Explains the demo skill.",
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
    revision: 7,
    importedAtMs: 1,
    updatedAtMs: 1,
};

function preview(
    agentId: SkillDeployAgent,
    targetState: SkillDeployPreview["targetState"],
    symlinkAvailable = true,
): SkillDeployPreview {
    return {
        agentId,
        skillId: ID,
        skillName: skill.name,
        skillRevision: skill.revision,
        contentDigest: skill.contentDigest,
        configRoot: `/home/test/${agentId}`,
        targetPath: `/home/test/${agentId}/skills/demo-skill`,
        targetName: skill.name,
        targetState,
        symlinkAvailable,
        suggestedMode: symlinkAvailable ? "symlink" : "copy",
        canApply: targetState !== "foreign",
    };
}

function record(
    agentId: SkillDeployAgent,
    overrides: Partial<SkillDeploymentRecord> = {},
): SkillDeploymentRecord {
    return {
        skillId: ID,
        skillName: skill.name,
        agentId,
        configRoot: `/home/test/${agentId}`,
        mode: "symlink",
        skillRevision: skill.revision,
        contentDigest: skill.contentDigest,
        inSync: true,
        deployedAtMs: 1,
        ...overrides,
    };
}

/** Live target state per Agent for the next reads. */
let targets: Record<SkillDeployAgent, SkillDeployPreview["targetState"]>;

async function mount(locale: "en" | "zh-CN" = "en") {
    const i18n = await createAppI18n(locale);
    return render(
        <I18nextProvider i18n={i18n}>
            <SkillDeployPanel record={skill} />
        </I18nextProvider>,
    );
}

async function agentRow(agent: string): Promise<HTMLElement> {
    return screen.findByRole("listitem", { name: `${agent} · demo-skill` });
}

beforeEach(() => {
    targets = {
        claudeCode: "absent",
        pi: "absent",
        grokBuild: "absent",
        codex: "absent",
    };
    previewDeploy.mockReset().mockImplementation((agentId) =>
        Promise.resolve({
            kind: "desktop" as const,
            preview: preview(agentId, targets[agentId]),
        }),
    );
    listDeployments
        .mockReset()
        .mockResolvedValue({ kind: "desktop", items: [] });
    apply.mockReset().mockImplementation((input) =>
        Promise.resolve({
            agentId: input.agentId,
            skillId: input.skillId,
            skillName: skill.name,
            skillRevision: input.expectedSkillRevision,
            mode: input.mode,
            targetPath: `${input.configRoot}/skills/demo-skill`,
        }),
    );
    resync.mockReset().mockImplementation((input) =>
        Promise.resolve({
            agentId: input.agentId,
            skillId: input.skillId,
            skillName: skill.name,
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
            targetPath: `${input.configRoot}/skills/demo-skill`,
            targetRemoved: true,
            leftForeign: false,
        }),
    );
});
afterEach(() => {
    cleanup();
});

test("every Agent row shows its own status and one matching action", async () => {
    targets = {
        claudeCode: "absent",
        pi: "owned_symlink",
        grokBuild: "foreign",
        codex: "absent",
    };
    listDeployments.mockResolvedValue({
        kind: "desktop",
        items: [record("pi")],
    });
    await mount();

    const claude = await agentRow("Claude Code");
    expect(await within(claude).findByText("Not injected")).toBeDefined();
    expect(
        within(claude).getByRole("button", {
            name: "Inject demo-skill into Claude Code",
        }),
    ).toBeDefined();

    const pi = await agentRow("Pi");
    expect(within(pi).getByText("Injected")).toBeDefined();
    expect(
        within(pi).getByRole("button", { name: "Remove demo-skill from Pi" }),
    ).toBeDefined();
    expect(within(pi).queryByRole("button", { name: /Inject/u })).toBeNull();

    const grok = await agentRow("Grok Build");
    expect(within(grok).getByText("Occupied")).toBeDefined();
    expect(
        within(grok).getByText(
            "A folder vibemate does not manage already uses this name. It is left untouched.",
        ),
    ).toBeDefined();
    // Only the details toggle remains; nothing can overwrite the folder.
    expect(
        within(grok)
            .getAllByRole("button")
            .map((button) => button.getAttribute("aria-label")),
    ).toEqual(["Show details for Grok Build"]);

    // Revision numbers never reach the page.
    expect(screen.queryByText(/revision|\b7\b/iu)).toBeNull();
});

test("the target path is shown on demand, not as a row line", async () => {
    await mount();
    const claude = await agentRow("Claude Code");
    await within(claude).findByText("Not injected");
    expect(
        within(claude).queryByText("/home/test/claudeCode/skills/demo-skill"),
    ).toBeNull();
    fireEvent.click(
        within(claude).getByRole("button", {
            name: "Show details for Claude Code",
        }),
    );
    expect(within(claude).getByText("Location")).toBeDefined();
    expect(
        within(claude).getByText("/home/test/claudeCode/skills/demo-skill"),
    ).toBeDefined();
});

test("inject links automatically and re-reads the rows", async () => {
    await mount();
    const pi = await agentRow("Pi");
    await within(pi).findByText("Not injected");
    targets.pi = "owned_symlink";
    listDeployments.mockResolvedValue({
        kind: "desktop",
        items: [record("pi")],
    });
    fireEvent.click(
        within(pi).getByRole("button", { name: "Inject demo-skill into Pi" }),
    );
    await waitFor(() => {
        expect(apply).toHaveBeenCalledWith({
            agentId: "pi",
            skillId: ID,
            expectedSkillRevision: 7,
            configRoot: "/home/test/pi",
            mode: "symlink",
            targetState: "absent",
        });
    });
    expect(await within(pi).findByText("Injected")).toBeDefined();
});

test("inject copies when links are unavailable or Advanced asks for it", async () => {
    previewDeploy.mockImplementation((agentId) =>
        Promise.resolve({
            kind: "desktop" as const,
            preview: preview(agentId, "absent", agentId !== "claudeCode"),
        }),
    );
    await mount();
    const claude = await agentRow("Claude Code");
    await within(claude).findByText("Not injected");
    fireEvent.click(
        within(claude).getByRole("button", {
            name: "Inject demo-skill into Claude Code",
        }),
    );
    await waitFor(() => {
        expect(apply).toHaveBeenLastCalledWith(
            expect.objectContaining({ agentId: "claudeCode", mode: "copy" }),
        );
    });

    // The override lives behind a collapsed Advanced disclosure.
    fireEvent.click(screen.getByText("Advanced"));
    await setFieldSelectValue(
        screen.getByRole("combobox", { name: "Link method" }),
        "copy",
    );
    const pi = await agentRow("Pi");
    await waitFor(() => {
        expect(
            within(pi)
                .getByRole("button", { name: "Inject demo-skill into Pi" })
                .getAttribute("aria-disabled"),
        ).toBe("false");
    });
    fireEvent.click(
        within(pi).getByRole("button", { name: "Inject demo-skill into Pi" }),
    );
    await waitFor(() => {
        expect(apply).toHaveBeenLastCalledWith(
            expect.objectContaining({ agentId: "pi", mode: "copy" }),
        );
    });
});

test("remove asks for confirmation before it runs", async () => {
    targets.claudeCode = "owned_symlink";
    listDeployments.mockResolvedValue({
        kind: "desktop",
        items: [record("claudeCode")],
    });
    await mount();
    const claude = await agentRow("Claude Code");
    fireEvent.click(
        await within(claude).findByRole("button", {
            name: "Remove demo-skill from Claude Code",
        }),
    );
    const dialog = await screen.findByRole("dialog", {
        name: "Remove from Claude Code?",
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(disable).not.toHaveBeenCalled();

    fireEvent.click(
        within(claude).getByRole("button", {
            name: "Remove demo-skill from Claude Code",
        }),
    );
    const again = await screen.findByRole("dialog", {
        name: "Remove from Claude Code?",
    });
    fireEvent.click(within(again).getByRole("button", { name: "Remove" }));
    await waitFor(() => {
        expect(disable).toHaveBeenCalledWith({
            agentId: "claudeCode",
            skillId: ID,
            configRoot: "/home/test/claudeCode",
        });
    });
});

test("a deleted placement needs sync and resync restores its mode", async () => {
    listDeployments.mockResolvedValue({
        kind: "desktop",
        items: [record("grokBuild", { inSync: false, mode: "copy" })],
    });
    await mount();
    const grok = await agentRow("Grok Build");
    expect(await within(grok).findByText("Needs sync")).toBeDefined();
    fireEvent.click(
        within(grok).getByRole("button", {
            name: "Resync demo-skill in Grok Build",
        }),
    );
    const confirm = await screen.findByRole("dialog", {
        name: "Resync in Grok Build?",
    });
    expect(resync).not.toHaveBeenCalled();
    fireEvent.click(within(confirm).getByRole("button", { name: "Resync" }));
    await waitFor(() => {
        expect(resync).toHaveBeenCalledWith({
            agentId: "grokBuild",
            skillId: ID,
            expectedSkillRevision: 7,
            configRoot: "/home/test/grokBuild",
            reviewedTargetPath: "/home/test/grokBuild/skills/demo-skill",
            reviewedTargetState: "absent",
            mode: "copy",
        });
    });
    expect(apply).not.toHaveBeenCalled();
});

test("an edited placement can resync now and can still be removed", async () => {
    targets.pi = "foreign";
    listDeployments.mockResolvedValue({
        kind: "desktop",
        items: [record("pi", { inSync: false })],
    });
    await mount();
    const pi = await agentRow("Pi");
    expect(await within(pi).findByText("Needs sync")).toBeDefined();
    expect(
        within(pi).getByText(
            "The placed files were edited outside vibemate. Resync replaces them with the library version (the edited files are backed up first); Remove keeps them and stops managing them.",
        ),
    ).toBeDefined();
    const button = within(pi).getByRole("button", {
        name: "Resync demo-skill in Pi",
    });
    expect(button.getAttribute("aria-disabled")).not.toBe("true");
    fireEvent.click(button);
    fireEvent.click(await screen.findByRole("button", { name: "Resync" }));
    await waitFor(() => {
        expect(resync).toHaveBeenCalledWith({
            agentId: "pi",
            skillId: ID,
            expectedSkillRevision: 7,
            configRoot: "/home/test/pi",
            reviewedTargetPath: "/home/test/pi/skills/demo-skill",
            reviewedTargetState: "foreign",
            mode: "symlink",
        });
    });
    expect(apply).not.toHaveBeenCalled();
    fireEvent.click(
        within(pi).getByRole("button", { name: "Remove demo-skill from Pi" }),
    );
    expect(
        await screen.findByText(
            "The files were edited outside vibemate, so they are left in place; vibemate only stops managing them.",
        ),
    ).toBeDefined();
});

test("the Codex row injects and shows a shared-folder duplicate hint", async () => {
    previewDeploy.mockImplementation((agentId) =>
        Promise.resolve({
            kind: "desktop" as const,
            preview: {
                ...preview(agentId, targets[agentId]),
                sharedDuplicatePath:
                    agentId === "codex"
                        ? "/home/test/.agents/skills/demo-skill"
                        : null,
            },
        }),
    );
    await mount();
    const codex = await agentRow("Codex");
    expect(await within(codex).findByText("Not injected")).toBeDefined();
    expect(
        within(codex).getByText(
            "A skill with the same name already exists in ~/.agents/skills (/home/test/.agents/skills/demo-skill). Codex loads both folders and lists same-named skills separately; they are not merged.",
        ),
    ).toBeDefined();
    expect(within(await agentRow("Pi")).queryByText(/~\/\.agents/u)).toBeNull();
    fireEvent.click(
        within(codex).getByRole("button", {
            name: "Inject demo-skill into Codex",
        }),
    );
    await waitFor(() => {
        expect(apply).toHaveBeenCalledWith({
            agentId: "codex",
            skillId: ID,
            expectedSkillRevision: 7,
            configRoot: "/home/test/codex",
            mode: "symlink",
            targetState: "absent",
        });
    });
});

test("a failed action stays on its row with a translated reason", async () => {
    apply.mockRejectedValue(new SkillRequestError("target_conflict"));
    await mount();
    const claude = await agentRow("Claude Code");
    await within(claude).findByText("Not injected");
    fireEvent.click(
        within(claude).getByRole("button", {
            name: "Inject demo-skill into Claude Code",
        }),
    );
    expect(await within(claude).findByRole("alert")).toBeDefined();
    const pi = await agentRow("Pi");
    expect(within(pi).queryByRole("alert")).toBeNull();
});

test("the browser preview explains that injecting needs the desktop app", async () => {
    previewDeploy.mockResolvedValue({ kind: "preview" });
    await mount();
    expect(
        await screen.findByText("Injecting a skill needs the desktop app."),
    ).toBeDefined();
});

test("the Chinese rows use the same statuses and actions", async () => {
    targets.pi = "owned_symlink";
    listDeployments.mockResolvedValue({
        kind: "desktop",
        items: [record("pi")],
    });
    await mount("zh-CN");
    const pi = await screen.findByRole("listitem", { name: "Pi · demo-skill" });
    expect(await within(pi).findByText("已注入")).toBeDefined();
    expect(
        within(pi).getByRole("button", { name: "从 Pi 移除 demo-skill" }),
    ).toBeDefined();
    const claude = await screen.findByRole("listitem", {
        name: "Claude Code · demo-skill",
    });
    expect(within(claude).getByText("未注入")).toBeDefined();
});

test("resync of edited files names the backup folder first and the backup after", async () => {
    targets.claudeCode = "foreign";
    listDeployments.mockResolvedValue({
        kind: "desktop",
        items: [record("claudeCode", { inSync: false })],
    });
    previewDeploy.mockImplementation((agentId) =>
        Promise.resolve({
            kind: "desktop" as const,
            preview:
                agentId === "claudeCode"
                    ? {
                          ...editedCopyPreview,
                          skillId: ID,
                          skillRevision: skill.revision,
                          configRoot: "/home/test/claudeCode",
                          targetPath: "/home/test/claudeCode/skills/demo-skill",
                      }
                    : preview(agentId, targets[agentId]),
        }),
    );
    resync.mockResolvedValue({ ...resyncedCopy, skillId: ID });
    await mount();
    const claude = await agentRow("Claude Code");
    fireEvent.click(
        await within(claude).findByRole("button", {
            name: "Resync demo-skill in Claude Code",
        }),
    );
    const confirm = await screen.findByRole("dialog", {
        name: "Resync in Claude Code?",
    });
    expect(
        within(confirm).getByText(
            `This overwrites your changes in /home/test/claudeCode/skills/demo-skill. They are backed up to ${editedCopyPreview.backupDir ?? ""} first.`,
        ),
    ).toBeDefined();
    expect(resync).not.toHaveBeenCalled();
    fireEvent.click(
        within(confirm).getByRole("button", { name: "Back up and overwrite" }),
    );
    expect(
        await screen.findByText(
            `Resynced in Claude Code. Your changes were backed up to ${resyncedCopy.backupPath ?? ""}.`,
        ),
    ).toBeDefined();
    expect(resync).toHaveBeenCalledWith(
        expect.objectContaining({
            agentId: "claudeCode",
            reviewedTargetState: "foreign",
        }),
    );
});

test("the resync confirmation and backup notice are translated", async () => {
    targets.claudeCode = "foreign";
    listDeployments.mockResolvedValue({
        kind: "desktop",
        items: [record("claudeCode", { inSync: false })],
    });
    previewDeploy.mockImplementation((agentId) =>
        Promise.resolve({
            kind: "desktop" as const,
            preview:
                agentId === "claudeCode"
                    ? {
                          ...preview(agentId, "foreign"),
                          overwritesUserChanges: true,
                          backupDir: "/test/app/backups",
                      }
                    : preview(agentId, targets[agentId]),
        }),
    );
    await mount("zh-CN");
    const claude = await agentRow("Claude Code");
    fireEvent.click(
        await within(claude).findByRole("button", {
            name: "在 Claude Code 中重新同步 demo-skill",
        }),
    );
    const confirm = await screen.findByRole("dialog", {
        name: "在 Claude Code 中重新同步？",
    });
    expect(
        within(confirm).getByText(
            "这会覆盖你在 /home/test/claudeCode/skills/demo-skill 中的修改。覆盖前会先备份到 /test/app/backups。",
        ),
    ).toBeDefined();
    fireEvent.click(within(confirm).getByRole("button", { name: "取消" }));
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(resync).not.toHaveBeenCalled();
});

const TARGET_READ_FAILED =
    "Can't read the injected copy in the Agent folder. Check the folder's permissions, then try again.";
const LIBRARY_READ_FAILED = "The skill library could not be read.";

test("an unreadable injected copy in the single preview names the copy, not the library", async () => {
    previewDeploy.mockImplementation((agentId) =>
        agentId === "pi"
            ? Promise.reject(new SkillRequestError("read_failed"))
            : Promise.resolve({
                  kind: "desktop" as const,
                  preview: preview(agentId, targets[agentId]),
              }),
    );
    await mount();
    const pi = await agentRow("Pi");
    expect((await within(pi).findByRole("alert")).textContent).toContain(
        TARGET_READ_FAILED,
    );
    expect(within(pi).queryByText(LIBRARY_READ_FAILED)).toBeNull();
});

test("a resync that cannot read the injected copy names the copy", async () => {
    resync.mockRejectedValue(new SkillRequestError("read_failed"));
    listDeployments.mockResolvedValue({
        kind: "desktop",
        items: [record("grokBuild", { inSync: false, mode: "copy" })],
    });
    await mount();
    const grok = await agentRow("Grok Build");
    fireEvent.click(
        await within(grok).findByRole("button", {
            name: "Resync demo-skill in Grok Build",
        }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Resync" }));
    expect((await within(grok).findByRole("alert")).textContent).toBe(
        TARGET_READ_FAILED,
    );
});

test("a remove that cannot read the injected copy names the copy", async () => {
    disable.mockRejectedValue(new SkillRequestError("read_failed"));
    targets.claudeCode = "owned_symlink";
    listDeployments.mockResolvedValue({
        kind: "desktop",
        items: [record("claudeCode")],
    });
    await mount();
    const claude = await agentRow("Claude Code");
    fireEvent.click(
        await within(claude).findByRole("button", {
            name: "Remove demo-skill from Claude Code",
        }),
    );
    const dialog = await screen.findByRole("dialog", {
        name: "Remove from Claude Code?",
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
    expect((await within(claude).findByRole("alert")).textContent).toBe(
        TARGET_READ_FAILED,
    );
});

test("the injected-copy read failure is translated", async () => {
    previewDeploy.mockRejectedValue(new SkillRequestError("read_failed"));
    await mount("zh-CN");
    const pi = await screen.findByRole("listitem", { name: "Pi · demo-skill" });
    expect((await within(pi).findByRole("alert")).textContent).toContain(
        "无法读取已注入的副本。请检查 Agent 文件夹的权限后重试。",
    );
    expect(within(pi).queryByText("无法读取技能库。")).toBeNull();
});

test("library reads keep the library copy", async () => {
    apply.mockRejectedValue(new SkillRequestError("read_failed"));
    await mount();
    const claude = await agentRow("Claude Code");
    await within(claude).findByText("Not injected");
    fireEvent.click(
        within(claude).getByRole("button", {
            name: "Inject demo-skill into Claude Code",
        }),
    );
    expect((await within(claude).findByRole("alert")).textContent).toBe(
        LIBRARY_READ_FAILED,
    );
    cleanup();

    listDeployments.mockRejectedValue(new SkillRequestError("read_failed"));
    await mount();
    expect(await screen.findByText(LIBRARY_READ_FAILED)).toBeDefined();
    expect(screen.queryByText(TARGET_READ_FAILED)).toBeNull();
});
