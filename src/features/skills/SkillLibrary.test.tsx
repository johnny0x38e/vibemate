import {
    act,
    cleanup,
    fireEvent,
    render,
    screen,
    waitFor,
    within,
} from "@testing-library/react";
import { StrictMode, createRef, type Ref } from "react";
import type { ModuleNavigationHandle } from "../../components/moduleNavigation";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createAppI18n } from "../../i18n";
import { NotificationProvider } from "../../components/Notifications";
import {
    cleanupDeletedSkillCopies,
    deleteSkill,
    getSkill,
    importSkillFolder,
    getMarketplaceSkillDetail,
    installMarketplaceSkill,
    installSkill,
    listMarketplaceSkills,
    listAgentSkillDeployments,
    listSkillDeployments,
    listSkillDeployRecoveries,
    listSkills,
    pickSkillFolder,
    previewMarketplaceInstall,
    previewSkillDeploy,
    previewSkillInstall,
    searchMarketplaceSkills,
    setSkillSource,
    SkillRequestError,
    type MarketplaceSkill,
    type SkillInstallPreview,
    type SkillInstallReport,
    type SkillRecord,
} from "../../lib/desktop/skills";
import { SkillLibrary } from "./SkillLibrary";
import {
    MARKETPLACE_DETAIL_PREFETCH_CONCURRENCY,
    MARKETPLACE_PAGE_SIZE,
} from "./SkillMarketplacePage";
import { setFieldSelectValue } from "../../test/fieldSelect";
vi.mock(import("../../lib/desktop/skills"), async (importOriginal) => ({
    ...(await importOriginal()),
    listSkills: vi.fn(),
    getSkill: vi.fn(),
    pickSkillFolder: vi.fn(),
    importSkillFolder: vi.fn(),
    previewSkillInstall: vi.fn(),
    installSkill: vi.fn(),
    listMarketplaceSkills: vi.fn(),
    searchMarketplaceSkills: vi.fn(),
    getMarketplaceSkillDetail: vi.fn(),
    previewMarketplaceInstall: vi.fn(),
    installMarketplaceSkill: vi.fn(),
    setSkillSource: vi.fn(),
    deleteSkill: vi.fn(),
    cleanupDeletedSkillCopies: vi.fn(),
    previewSkillDeploy: vi.fn(),
    listSkillDeployments: vi.fn(),
    listAgentSkillDeployments: vi.fn(),
    listSkillDeployRecoveries: vi.fn(),
}));
const list = vi.mocked(listSkills);
const get = vi.mocked(getSkill);
const pick = vi.mocked(pickSkillFolder);
const create = vi.mocked(importSkillFolder);
const preview = vi.mocked(previewSkillInstall);
const install = vi.mocked(installSkill);
const listMarketplace = vi.mocked(listMarketplaceSkills);
const searchMarketplace = vi.mocked(searchMarketplaceSkills);
const getMarketplaceDetail = vi.mocked(getMarketplaceSkillDetail);
const previewMarketplace = vi.mocked(previewMarketplaceInstall);
const installMarketplace = vi.mocked(installMarketplaceSkill);
const save = vi.mocked(setSkillSource);
const remove = vi.mocked(deleteSkill);
const removeCopies = vi.mocked(cleanupDeletedSkillCopies);
const previewDeploy = vi.mocked(previewSkillDeploy);
const listDeployments = vi.mocked(listSkillDeployments);
const listAgentDeployments = vi.mocked(listAgentSkillDeployments);
const listRecoveries = vi.mocked(listSkillDeployRecoveries);

const ID = "0123456789abcdef0123456789abcdef";
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
    contentDigest: "ab".repeat(32),
    fileCount: 2,
    byteSize: 2048,
    revision: 1,
    importedAtMs: 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_000,
};

function page(
    items: SkillRecord[],
    nextCursor: string | null = null,
    total = items.length,
) {
    return { kind: "desktop" as const, page: { items, nextCursor, total } };
}

const COMMIT = "a".repeat(40);
const GIT_COMMAND = `git fetch --depth 1 https://github.com/example/skills v1.2.0 && git checkout --detach FETCH_HEAD`;
const NPX_COMMAND =
    "npx --yes skills@1.7.4 add https://github.com/example/skills --skill '*' --copy -g -a claude-code -y";
const gitPreview: SkillInstallPreview = {
    method: "git",
    sourceUrl: "https://github.com/example/skills",
    revision: "v1.2.0",
    installer: "git",
    installerVersion: null,
    command: GIT_COMMAND,
};
const npxPreview: SkillInstallPreview = {
    method: "npx",
    sourceUrl: "https://github.com/example/skills",
    revision: null,
    installer: "skills",
    installerVersion: "1.7.4",
    command: NPX_COMMAND,
};
const installedRow: SkillRecord = {
    ...row,
    id: "f".repeat(32),
    name: "installed-skill",
    originPath: null,
    sourceUrl: "https://github.com/example/skills",
    sourceRevision: COMMIT,
    fetchMethod: "git",
    installerVersion: null,
    installerCommand: GIT_COMMAND,
    revision: 2,
};
const gitReport: SkillInstallReport = {
    method: "git",
    sourceUrl: "https://github.com/example/skills",
    revision: COMMIT,
    installer: "git",
    installerVersion: null,
    command: GIT_COMMAND,
    exitStatus: 0,
    imported: [installedRow],
    skipped: [],
};

async function setup(
    locale: "en" | "zh-CN" = "en",
    homeRef?: Ref<ModuleNavigationHandle>,
) {
    const i18n = await createAppI18n(locale);
    // The provider renders the same transient feedback a user sees.
    return render(
        <StrictMode>
            <I18nextProvider i18n={i18n}>
                <NotificationProvider>
                    <SkillLibrary
                        {...(homeRef === undefined ? {} : { homeRef })}
                    />
                </NotificationProvider>
            </I18nextProvider>
        </StrictMode>,
    );
}

beforeEach(() => {
    list.mockReset().mockResolvedValue(page([row]));
    get.mockReset().mockResolvedValue(row);
    pick.mockReset().mockResolvedValue({
        kind: "selected",
        path: "/tmp/sources/demo",
    });
    create.mockReset().mockResolvedValue({ imported: [row], skipped: [] });
    preview.mockReset().mockResolvedValue({
        kind: "desktop",
        preview: gitPreview,
    });
    install.mockReset().mockResolvedValue(gitReport);
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
    listMarketplace.mockReset().mockResolvedValue({
        kind: "desktop",
        skills: [marketplaceRow],
    });
    searchMarketplace.mockReset().mockResolvedValue({
        kind: "desktop",
        skills: [marketplaceRow],
    });
    getMarketplaceDetail
        .mockReset()
        .mockImplementation((source: string, skillId: string) =>
            Promise.resolve({
                kind: "desktop" as const,
                skill: {
                    id: `${source}/${skillId}`,
                    source,
                    skillId,
                    name: skillId,
                    installs: 42,
                    description:
                        skillId === "tdd"
                            ? "Test-driven development skill from the catalog page."
                            : `Summary for ${skillId}.`,
                    summary: null,
                    catalogInstallCommand: null,
                },
            }),
        );
    previewMarketplace.mockReset().mockResolvedValue({
        kind: "desktop",
        preview: {
            method: "git",
            sourceUrl: "https://github.com/mattpocock/skills",
            revision: "HEAD",
            installer: "git",
            installerVersion: null,
            command:
                "git fetch --depth 1 https://github.com/mattpocock/skills HEAD && git checkout --detach FETCH_HEAD",
        },
    });
    installMarketplace.mockReset().mockResolvedValue({
        ...gitReport,
        sourceUrl: "https://github.com/mattpocock/skills",
        command:
            "git fetch --depth 1 https://github.com/mattpocock/skills HEAD && git checkout --detach FETCH_HEAD",
        imported: [
            {
                ...installedRow,
                sourceUrl: "https://github.com/mattpocock/skills",
                installerCommand:
                    "git fetch --depth 1 https://github.com/mattpocock/skills HEAD && git checkout --detach FETCH_HEAD",
            },
        ],
    });
    previewDeploy.mockReset().mockResolvedValue({ kind: "preview" });
    listDeployments
        .mockReset()
        .mockResolvedValue({ kind: "desktop", items: [] });
    // Browser preview by default: the list hides its injected-Agent chips.
    listAgentDeployments.mockReset().mockResolvedValue({ kind: "preview" });
    listRecoveries
        .mockReset()
        .mockResolvedValue({ kind: "desktop", items: [] });
    save.mockReset().mockResolvedValue({
        ...row,
        sourceUrl: "https://example.com/skills",
        sourceRevision: "v1.2.0",
        revision: 2,
    });
    remove.mockReset().mockResolvedValue({ id: ID, cleanupPending: false });
    removeCopies.mockReset().mockResolvedValue(true);
});
afterEach(() => {
    cleanup();
});

test("the list keeps one compact row per skill", async () => {
    await setup();
    expect(
        await screen.findByRole("heading", { name: "demo-skill" }),
    ).toBeDefined();
    expect(screen.getByText("Explains the demo skill.")).toBeDefined();
    // A folder import states that, instead of claiming an unknown source.
    expect(screen.getByText("Source: local import")).toBeDefined();
    // File count, size and dates belong to the detail page.
    expect(screen.queryByText("2 files")).toBeNull();
    expect(screen.queryByText("2 KB")).toBeNull();
    expect(screen.queryByText("Imported from /tmp/sources/demo")).toBeNull();
    expect(
        screen.getByRole("button", { name: "Open details for demo-skill" }),
    ).toBeDefined();
    expect(list).toHaveBeenCalledTimes(1);
});

test("the detail page shows the imported facts", async () => {
    await setup();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Open details for demo-skill",
        }),
    );
    expect(get).toHaveBeenCalledWith(ID);
    expect(
        await screen.findByRole("heading", { level: 1, name: "Skill details" }),
    ).toBeDefined();
    expect(screen.getByText("2 files")).toBeDefined();
    expect(screen.getByText("2 KB")).toBeDefined();
    expect(screen.getByText("/tmp/sources/demo")).toBeDefined();
    expect(screen.getByText("Local import")).toBeDefined();
    // The content digest stays out of the page; only facts a user can read are shown.
    expect(screen.queryByText("ab".repeat(32))).toBeNull();
    expect(screen.getByText("Imported from")).toBeDefined();
    expect(screen.getByText("Last change")).toBeDefined();
    expect(
        await screen.findByText("Injecting a skill needs the desktop app."),
    ).toBeDefined();
});

test("the detail page injects into each Agent from its own row", async () => {
    previewDeploy.mockImplementation((agentId) =>
        Promise.resolve({
            kind: "desktop" as const,
            preview: {
                agentId,
                skillId: ID,
                skillName: row.name,
                skillRevision: row.revision,
                contentDigest: row.contentDigest,
                configRoot: `/home/test/${agentId}`,
                targetPath: `/home/test/${agentId}/skills/demo-skill`,
                targetName: row.name,
                targetState: "absent" as const,
                symlinkAvailable: true,
                suggestedMode: "symlink" as const,
                canApply: true,
            },
        }),
    );
    listDeployments.mockResolvedValue({ kind: "desktop", items: [] });
    await setup();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Open details for demo-skill",
        }),
    );
    const rows = await screen.findByRole("list", { name: "Agents" });
    for (const agent of ["Claude Code", "Pi", "Grok Build"]) {
        const agentRow = within(rows).getByRole("listitem", {
            name: `${agent} · demo-skill`,
        });
        expect(await within(agentRow).findByText("Not injected")).toBeDefined();
        expect(
            within(agentRow).getByRole("button", {
                name: `Inject demo-skill into ${agent}`,
            }),
        ).toBeDefined();
    }
    // The old Agent dropdown is gone; every Agent is previewed directly.
    expect(screen.queryByRole("combobox", { name: "Agent" })).toBeNull();
    expect(previewDeploy).toHaveBeenCalledWith("claudeCode", ID);
    expect(previewDeploy).toHaveBeenCalledWith("pi", ID);
    expect(previewDeploy).toHaveBeenCalledWith("grokBuild", ID);
});

test("each list row states how many Agents the skill is injected into", async () => {
    const other: SkillRecord = { ...row, id: "e".repeat(32), name: "other" };
    list.mockResolvedValue(page([row, other]));
    const record = (agentId: "pi" | "claudeCode") => ({
        skillId: ID,
        skillName: row.name,
        agentId,
        configRoot: `/home/test/${agentId}`,
        mode: "symlink" as const,
        skillRevision: 1,
        contentDigest: row.contentDigest,
        inSync: agentId === "pi",
        deployedAtMs: 1,
    });
    listAgentDeployments.mockImplementation((agentId) =>
        Promise.resolve({
            kind: "desktop" as const,
            items:
                agentId === "grokBuild"
                    ? []
                    : [record(agentId === "pi" ? "pi" : "claudeCode")],
        }),
    );
    await setup();
    const rows = await screen.findByRole("list", { name: "Imported skills" });
    const demo = within(rows)
        .getByRole("heading", { name: "demo-skill" })
        .closest("li");
    const second = within(rows)
        .getByRole("heading", { name: "other" })
        .closest("li");
    if (!(demo instanceof HTMLElement) || !(second instanceof HTMLElement))
        throw new Error("missing rows");
    // An out-of-sync placement still counts as injected.
    expect(
        await within(demo).findByText("Injected into 2 agents"),
    ).toBeDefined();
    expect(within(second).getByText("Not injected")).toBeDefined();
    expect(listAgentDeployments).toHaveBeenCalledWith("claudeCode");
    expect(listAgentDeployments).toHaveBeenCalledWith("pi");
    expect(listAgentDeployments).toHaveBeenCalledWith("grokBuild");
});

test("a failed count read hides the chips instead of guessing", async () => {
    listAgentDeployments.mockRejectedValue(
        new SkillRequestError("storage_unavailable"),
    );
    await setup();
    expect(
        await screen.findByRole("heading", { name: "demo-skill" }),
    ).toBeDefined();
    await waitFor(() => {
        expect(listAgentDeployments).toHaveBeenCalled();
    });
    expect(screen.queryByText("Not injected")).toBeNull();
    expect(screen.queryByText(/Injected into/u)).toBeNull();
});

test("a large skill reports its size in megabytes", async () => {
    // 11 MiB, the size of a real published skill; kilobytes would be unreadable.
    const large = { ...row, byteSize: 11_534_336 };
    list.mockResolvedValue(page([large]));
    get.mockResolvedValue(large);
    await setup();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Open details for demo-skill",
        }),
    );
    expect(await screen.findByText("11 MB")).toBeDefined();
});

test("the library total is stated above the list and omitted when empty", async () => {
    // 7 skills, one page loaded: the count describes the library, not the page.
    list.mockResolvedValue(page([row], null, 7));
    await setup();
    expect(await screen.findByText("7 skills in total")).toBeDefined();

    // An empty library explains the next step instead of showing a zero count.
    cleanup();
    list.mockResolvedValue(page([], null, 0));
    await setup();
    expect(
        await screen.findByText("No skill folder has been imported yet."),
    ).toBeDefined();
    expect(screen.queryByText(/in total/)).toBeNull();
});

test("local import asks for a folder and imports everything in it", async () => {
    const second: SkillRecord = { ...row, id: "f".repeat(32), name: "zebra" };
    create.mockResolvedValue({ imported: [row, second], skipped: [] });
    // The re-read list is what the page shows, so it holds both skills.
    list.mockResolvedValueOnce(page([row]));
    list.mockResolvedValueOnce(page([row, second]));
    await setup();
    fireEvent.click(await screen.findByRole("button", { name: "Import" }));
    await waitFor(() => {
        expect(pick).toHaveBeenCalledWith("Choose a skill folder");
    });
    expect(create).toHaveBeenCalledWith("/tmp/sources/demo");
    // Both rows are announced and listed, and the first page is re-read.
    expect(await screen.findByText("Imported 2 skills.")).toBeDefined();
    await waitFor(() => {
        expect(list).toHaveBeenCalledTimes(2);
    });
    expect(screen.getByRole("heading", { name: "zebra" })).toBeDefined();
});

test("closing the picker changes nothing", async () => {
    pick.mockResolvedValue({ kind: "cancelled" });
    await setup();
    fireEvent.click(await screen.findByRole("button", { name: "Import" }));
    await waitFor(() => {
        expect(pick).toHaveBeenCalledTimes(1);
    });
    expect(create).not.toHaveBeenCalled();
    expect(list).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Nothing was imported.")).toBeNull();
});

test("refused folders are named with their reason", async () => {
    create.mockResolvedValue({
        imported: [row],
        skipped: [
            { folder: "broken", code: "manifest_invalid" },
            { folder: "duplicate", code: "name_taken" },
        ],
    });
    await setup();
    fireEvent.click(await screen.findByRole("button", { name: "Import" }));
    expect(await screen.findByText("Imported 1 skill.")).toBeDefined();
    expect(screen.getByText("2 folders were skipped:")).toBeDefined();
    expect(
        screen.getByText(
            "broken: SKILL.md needs YAML frontmatter with a name and a one-line description.",
        ),
    ).toBeDefined();
    expect(
        screen.getByText(
            "duplicate: A skill with this name is already in the library.",
        ),
    ).toBeDefined();
});

test("a folder with no skills is explained and reported", async () => {
    create.mockResolvedValue({
        imported: [],
        skipped: [{ folder: "empty", code: "manifest_missing" }],
    });
    await setup();
    fireEvent.click(await screen.findByRole("button", { name: "Import" }));
    expect(await screen.findByText("Nothing was imported.")).toBeDefined();
    expect(
        screen.getByText("empty: The folder has no SKILL.md file."),
    ).toBeDefined();
    // No success claim is shown for an import that added nothing.
    expect(screen.queryByText("Imported 0 skills.")).toBeNull();
});

test("import failures are translated and unknown outcomes require a reload", async () => {
    await setup();
    pick.mockRejectedValue(new SkillRequestError("no_skills_found"));
    fireEvent.click(await screen.findByRole("button", { name: "Import" }));
    expect(
        await screen.findByText(
            "That folder holds no skill: no SKILL.md was found in it or in its subfolders.",
        ),
    ).toBeDefined();
    expect(screen.queryByRole("button", { name: "Reload list" })).toBeNull();

    // The second attempt reaches the import step again.
    pick.mockResolvedValue({ kind: "selected", path: "/tmp/sources/demo" });
    create.mockRejectedValue(new SkillRequestError("outcome_unknown"));
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    expect(
        await screen.findByText(
            "The result is unknown. Reload the list before another change.",
        ),
    ).toBeDefined();
    const reload = screen.getByRole("button", { name: "Reload list" });
    list.mockResolvedValue(page([row]));
    fireEvent.click(reload);
    await waitFor(() => {
        expect(list).toHaveBeenCalledTimes(2);
    });
});

test("loading more pages keeps the name order and skips repeats", async () => {
    const second: SkillRecord = { ...row, id: "f".repeat(32), name: "zebra" };
    const cursor = `${row.name}\n${row.id}`;
    list.mockResolvedValueOnce(page([row], cursor));
    list.mockResolvedValueOnce(page([row, second], null));
    await setup();
    fireEvent.click(await screen.findByRole("button", { name: "Load more" }));
    expect(await screen.findByRole("heading", { name: "zebra" })).toBeDefined();
    expect(list).toHaveBeenLastCalledWith(cursor);
    expect(screen.getAllByRole("heading", { name: "demo-skill" })).toHaveLength(
        1,
    );
    // The last page no longer offers more rows.
    expect(screen.queryByRole("button", { name: "Load more" })).toBeNull();
});

test("the detail page records an address without leaving the skill", async () => {
    await setup();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Open details for demo-skill",
        }),
    );
    expect(get).toHaveBeenCalledWith(ID);
    const url = await screen.findByLabelText("Source address");
    const revision = screen.getByLabelText("Revision");
    // A revision without an address is refused before any request.
    fireEvent.change(revision, { target: { value: "v9" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(
        screen.getByText(
            "A revision can only be saved together with a source address.",
        ),
    ).toBeDefined();
    expect(save).not.toHaveBeenCalled();

    fireEvent.change(url, {
        target: { value: "https://example.com/skills" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
        expect(save).toHaveBeenCalledWith({
            id: ID,
            expectedRevision: 1,
            url: "https://example.com/skills",
            revision: "v9",
            fetchMethod: null,
            installerVersion: null,
        });
    });
    // The page keeps showing the skill, now with the stored source (the mock
    // returns v1.2.0) instead of the unknown-source line.
    expect(
        await screen.findByText(
            "https://example.com/skills · revision v1.2.0 · Manual source",
        ),
    ).toBeDefined();
    expect(screen.getByText("Source saved.")).toBeDefined();
    expect(screen.getByLabelText("Source address")).toBeDefined();
    expect(screen.queryByText("Source: local import")).toBeNull();
    // The list row was updated in place, so going back needs no second read.
    fireEvent.click(screen.getByRole("button", { name: "Back to skills" }));
    expect(
        await screen.findByText(
            "Source: https://example.com/skills · revision v1.2.0 · Manual source",
        ),
    ).toBeDefined();
    expect(list).toHaveBeenCalledTimes(1);
});

test("a stale source revision asks for a reload instead of overwriting", async () => {
    await setup();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Open details for demo-skill",
        }),
    );
    const url = await screen.findByLabelText("Source address");
    save.mockRejectedValue(new SkillRequestError("revision_conflict"));
    fireEvent.change(url, {
        target: { value: "https://example.com/skills" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(
        await screen.findByText(
            "Another change was saved first. Reload and try again.",
        ),
    ).toBeDefined();
    const reload = screen.getByRole("button", { name: "Reload list" });
    // The re-read supplies the newer revision, so a retry cannot overwrite it.
    get.mockResolvedValue({ ...row, revision: 3 });
    fireEvent.click(reload);
    await waitFor(() => {
        expect(get).toHaveBeenCalledTimes(2);
    });
    // The reopened form starts from the stored values again.
    fireEvent.change(await screen.findByLabelText("Source address"), {
        target: { value: "https://example.com/skills" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
        expect(save).toHaveBeenLastCalledWith({
            id: ID,
            expectedRevision: 3,
            url: "https://example.com/skills",
            revision: null,
            fetchMethod: null,
            installerVersion: null,
        });
    });
});

test("a skill is deleted only after the confirmation names it", async () => {
    // The re-read list after the deletion no longer holds the skill.
    list.mockResolvedValueOnce(page([row])).mockResolvedValue(
        page([], null, 0),
    );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Delete “demo-skill”" }),
    );
    // The dialog names the skill and states what stays unchanged.
    expect(
        await screen.findByText(
            "Delete “demo-skill” from the library and remove vibemate's copy of it? The folder it was imported from stays unchanged.",
        ),
    ).toBeDefined();
    // Cancelling changes nothing.
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(remove).not.toHaveBeenCalled();

    fireEvent.click(
        screen.getByRole("button", { name: "Delete “demo-skill”" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    await waitFor(() => {
        expect(remove).toHaveBeenCalledExactlyOnceWith({
            id: ID,
            expectedRevision: 1,
        });
    });
    expect(await screen.findByText("Deleted “demo-skill”.")).toBeDefined();
    // The list is re-read instead of silently dropping the row.
    await waitFor(() => {
        expect(list).toHaveBeenCalledTimes(2);
    });
    expect(screen.queryByRole("heading", { name: "demo-skill" })).toBeNull();
});

test("a deployed skill lists its references and opens the detail page", async () => {
    listDeployments.mockResolvedValue({
        kind: "desktop",
        items: [
            {
                skillId: ID,
                skillName: row.name,
                agentId: "claudeCode",
                configRoot: "/home/test/.claude",
                mode: "symlink",
                skillRevision: 1,
                contentDigest: row.contentDigest,
                inSync: true,
                deployedAtMs: 1,
            },
        ],
    });
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Delete “demo-skill”" }),
    );
    expect(await screen.findByText("Claude Code · Injected")).toBeDefined();
    expect(
        screen
            .getByRole("button", { name: "Delete" })
            .getAttribute("aria-disabled"),
    ).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "Review deployments" }));
    expect(
        await screen.findByRole("heading", { level: 1, name: "Skill details" }),
    ).toBeDefined();
    expect(remove).not.toHaveBeenCalled();
});

test("a stale delete confirmation is refused and the list can be reloaded", async () => {
    remove.mockRejectedValue(new SkillRequestError("revision_conflict"));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Delete “demo-skill”" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    expect(
        await screen.findByText(
            "Another change was saved first. Reload and try again.",
        ),
    ).toBeDefined();
    // The row stays until the user re-reads the list.
    expect(screen.getByRole("heading", { name: "demo-skill" })).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "Reload list" }));
    await waitFor(() => {
        expect(list).toHaveBeenCalledTimes(2);
    });
});

test("leftover files after a delete can be cleaned up", async () => {
    remove.mockResolvedValue({ id: ID, cleanupPending: true });
    list.mockResolvedValueOnce(page([row])).mockResolvedValue(
        page([], null, 0),
    );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Delete “demo-skill”" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    // The leftover is stated and keeps a retry instead of being hidden.
    expect(
        await screen.findByRole("button", { name: "Retry file cleanup" }),
    ).toBeDefined();
    expect(
        screen.getAllByText(
            "Skill deleted; copied files remain. Retry cleanup.",
        ).length,
    ).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("button", { name: "Retry file cleanup" }));
    await waitFor(() => {
        expect(removeCopies).toHaveBeenCalledTimes(1);
    });
    expect(await screen.findByText("Copied files removed.")).toBeDefined();
    expect(
        screen.queryByRole("button", { name: "Retry file cleanup" }),
    ).toBeNull();
});

test("marketplace secondary page reviews HEAD before installing", async () => {
    list.mockResolvedValueOnce(page([row])).mockResolvedValue(
        page([row, installedRow], null, 2),
    );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    // Browse opens a secondary page, not a dialog.
    expect(
        await screen.findByRole("heading", { name: "Install from skills.sh" }),
    ).toBeDefined();
    expect(
        screen.getByRole("button", { name: "Back to skills" }),
    ).toBeDefined();
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => {
        expect(listMarketplace).toHaveBeenCalledWith("all_time");
    });
    fireEvent.click(
        await screen.findByRole("button", {
            name: /tdd/,
        }),
    );
    // Card click opens the detail dialog; Review there starts the same install path.
    const detail = await screen.findByRole("dialog");
    expect(detail).toBeDefined();
    expect(within(detail).getByText("mattpocock/skills")).toBeDefined();
    fireEvent.click(
        within(detail).getByRole("button", { name: "Review command" }),
    );
    expect(
        await screen.findByText(
            "git fetch --depth 1 https://github.com/mattpocock/skills HEAD && git checkout --detach FETCH_HEAD",
        ),
    ).toBeDefined();
    expect(previewMarketplace).toHaveBeenCalledWith({
        source: "mattpocock/skills",
        skillId: "tdd",
    });
    fireEvent.click(within(detail).getByRole("button", { name: "Install" }));
    await waitFor(() => {
        expect(installMarketplace).toHaveBeenCalledWith({
            source: "mattpocock/skills",
            skillId: "tdd",
        });
    });
    expect(await screen.findByRole("button", { name: "Done" })).toBeDefined();
    expect(
        screen.getByText("https://github.com/mattpocock/skills"),
    ).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    await waitFor(() => {
        expect(
            screen.queryByRole("heading", { name: "Install from skills.sh" }),
        ).toBeNull();
    });
    expect(
        await screen.findByRole("heading", { name: "Skills" }),
    ).toBeDefined();
});

test("marketplace catalog load failure clears loading and shows an error", async () => {
    listMarketplace
        .mockReset()
        .mockRejectedValue(new SkillRequestError("marketplace_unavailable"));
    list.mockResolvedValue(page([row]));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    expect(
        await screen.findByRole("heading", { name: "Install from skills.sh" }),
    ).toBeDefined();
    expect(
        await screen.findByText("The skills.sh catalog could not be reached."),
    ).toBeDefined();
    expect(screen.queryByText("Loading the catalog…")).toBeNull();
});

test("marketplace empty catalog clears loading and shows empty", async () => {
    listMarketplace.mockReset().mockResolvedValue({
        kind: "desktop",
        skills: [],
    });
    list.mockResolvedValue(page([row]));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    expect(
        await screen.findByText("No GitHub-installable skills matched."),
    ).toBeDefined();
    expect(screen.queryByText("Loading the catalog…")).toBeNull();
});

test("marketplace deferred catalog settle clears loading after resolve", async () => {
    let finish:
        | ((value: { kind: "desktop"; skills: MarketplaceSkill[] }) => void)
        | undefined;
    listMarketplace.mockReset().mockImplementation(
        () =>
            new Promise((resolve) => {
                finish = resolve;
            }),
    );
    list.mockResolvedValue(page([row]));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    expect(await screen.findByText("Loading the catalog…")).toBeDefined();
    // Catalog loading must not trap the user on the secondary page.
    const back = screen.getByRole("button", { name: "Back to skills" });
    expect(back.getAttribute("aria-disabled")).not.toBe("true");
    act(() => {
        finish?.({
            kind: "desktop",
            skills: [
                {
                    id: "mattpocock/skills/tdd",
                    source: "mattpocock/skills",
                    skillId: "tdd",
                    name: "tdd",
                    installs: 42,
                    description: null,
                    summary: null,
                    catalogInstallCommand: null,
                },
            ],
        });
    });
    expect(
        await screen.findByRole("button", {
            name: /tdd/,
        }),
    ).toBeDefined();
    expect(screen.queryByText("Loading the catalog…")).toBeNull();
});

test("marketplace Back stays put while install is pending or uncertain", async () => {
    let finishInstall: ((report: typeof gitReport) => void) | undefined;
    installMarketplace.mockImplementation(
        () =>
            new Promise((resolve) => {
                finishInstall = resolve;
            }),
    );
    list.mockResolvedValueOnce(page([row])).mockResolvedValue(
        page([row, installedRow], null, 2),
    );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    await waitFor(() => {
        expect(listMarketplace).toHaveBeenCalled();
    });
    fireEvent.click(
        await screen.findByRole("button", {
            name: /tdd/,
        }),
    );
    const pendingDetail = await screen.findByRole("dialog");
    fireEvent.click(
        within(pendingDetail).getByRole("button", { name: "Review command" }),
    );
    fireEvent.click(
        await within(pendingDetail).findByRole("button", { name: "Install" }),
    );
    expect(
        await screen.findByText("Installing from the marketplace…"),
    ).toBeDefined();
    const back = screen.getByRole("button", { name: "Back to skills" });
    expect(back.getAttribute("aria-disabled")).toBe("true");
    fireEvent.click(back);
    // Mid-install Back must not return to the list.
    expect(
        screen.getByRole("heading", { name: "Install from skills.sh" }),
    ).toBeDefined();
    finishInstall?.({
        ...gitReport,
        sourceUrl: "https://github.com/mattpocock/skills",
        command:
            "git fetch --depth 1 https://github.com/mattpocock/skills HEAD && git checkout --detach FETCH_HEAD",
        imported: [
            {
                ...installedRow,
                sourceUrl: "https://github.com/mattpocock/skills",
                installerCommand:
                    "git fetch --depth 1 https://github.com/mattpocock/skills HEAD && git checkout --detach FETCH_HEAD",
            },
        ],
    });
    expect(await screen.findByRole("button", { name: "Done" })).toBeDefined();
});

test("marketplace catalog uses a card grid with client-side paging", async () => {
    const many: MarketplaceSkill[] = Array.from({ length: 13 }, (_, index) => {
        const n = index + 1;
        const label = `skill-${String(n).padStart(2, "0")}`;
        return {
            id: `owner/repo/${label}`,
            source: "owner/repo",
            skillId: label,
            name: label,
            installs: n,
            description: null,
            summary: null,
            catalogInstallCommand: null,
        };
    });
    listMarketplace.mockReset().mockResolvedValue({
        kind: "desktop",
        skills: many,
    });
    list.mockResolvedValue(page([row]));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    expect(
        await screen.findByRole("button", {
            name: /skill-01/,
        }),
    ).toBeDefined();
    // Page size 12: the 13th card waits on page 2.
    expect(screen.queryByRole("button", { name: /skill-13/ })).toBeNull();
    expect(
        screen.getByRole("navigation", { name: "Marketplace pages" }),
    ).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(
        await screen.findByRole("button", {
            name: /skill-13/,
        }),
    ).toBeDefined();
    expect(screen.queryByRole("button", { name: /skill-01/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Page 1 of 2" }));
    expect(
        await screen.findByRole("button", {
            name: /skill-01/,
        }),
    ).toBeDefined();
    expect(screen.queryByRole("button", { name: /skill-13/ })).toBeNull();
});

test("marketplace board change resets paging to page 1", async () => {
    const boardRows = (prefix: string): MarketplaceSkill[] =>
        Array.from({ length: 13 }, (_, index) => {
            const n = index + 1;
            const label = `${prefix}-skill-${String(n).padStart(2, "0")}`;
            return {
                id: `${prefix}/repo/${label}`,
                source: `${prefix}/repo`,
                skillId: label,
                name: label,
                installs: n,
                description: null,
                summary: null,
                catalogInstallCommand: null,
            };
        });
    listMarketplace
        .mockReset()
        .mockResolvedValueOnce({
            kind: "desktop",
            skills: boardRows("all"),
        })
        .mockResolvedValueOnce({
            kind: "desktop",
            skills: boardRows("hot"),
        });
    list.mockResolvedValue(page([row]));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    await screen.findByRole("button", { name: /all-skill-01/ });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("button", { name: /all-skill-13/ });
    await setFieldSelectValue(
        screen.getByRole("combobox", { name: "Leaderboard" }),
        "hot",
    );
    await waitFor(() => {
        expect(listMarketplace).toHaveBeenLastCalledWith("hot");
    });
    expect(
        await screen.findByRole("button", { name: /hot-skill-01/ }),
    ).toBeDefined();
    expect(screen.queryByRole("button", { name: /hot-skill-13/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /all-skill-13/ })).toBeNull();
});

test("marketplace search resets paging to page 1", async () => {
    const many: MarketplaceSkill[] = Array.from({ length: 13 }, (_, index) => {
        const n = index + 1;
        const label = `skill-${String(n).padStart(2, "0")}`;
        return {
            id: `owner/repo/${label}`,
            source: "owner/repo",
            skillId: label,
            name: label,
            installs: n,
            description: null,
            summary: null,
            catalogInstallCommand: null,
        };
    });
    listMarketplace.mockReset().mockResolvedValue({
        kind: "desktop",
        skills: many,
    });
    searchMarketplace.mockReset().mockResolvedValue({
        kind: "desktop",
        skills: [
            {
                id: "owner/repo/found",
                source: "owner/repo",
                skillId: "found",
                name: "found-skill",
                installs: 1,
                description: null,
                summary: null,
                catalogInstallCommand: null,
            },
        ],
    });
    list.mockResolvedValue(page([row]));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    await screen.findByRole("button", { name: /skill-01/ });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("button", { name: /skill-13/ });
    fireEvent.change(screen.getByLabelText("Search"), {
        target: { value: "found" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(
        await screen.findByRole("button", {
            name: /found-skill/,
        }),
    ).toBeDefined();
    expect(searchMarketplace).toHaveBeenCalledWith("found");
    // A single-page search result must not keep the previous page chrome.
    expect(
        screen.queryByRole("navigation", { name: "Marketplace pages" }),
    ).toBeNull();
});

test("marketplace card opens detail dialog and close returns to catalog", async () => {
    list.mockResolvedValue(page([row]));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    fireEvent.click(
        await screen.findByRole("button", {
            name: /tdd/,
        }),
    );
    const detail = await screen.findByRole("dialog");
    expect(
        within(detail).getByRole("heading", { name: "Catalog skill" }),
    ).toBeDefined();
    await waitFor(() => {
        expect(getMarketplaceDetail).toHaveBeenCalledWith(
            "mattpocock/skills",
            "tdd",
        );
    });
    // Lazy description fills the dialog body; facts still show name/source/installs.
    expect(
        await within(detail).findByText(
            "Test-driven development skill from the catalog page.",
        ),
    ).toBeDefined();
    expect(within(detail).getAllByText("tdd").length).toBeGreaterThan(0);
    expect(within(detail).getByText("mattpocock/skills")).toBeDefined();
    expect(within(detail).getByText("42 installs")).toBeDefined();
    fireEvent.click(within(detail).getByRole("button", { name: "Close" }));
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
    });
    // Marketplace page remains; selection stays so footer Review still works.
    expect(
        screen.getByRole("heading", { name: "Install from skills.sh" }),
    ).toBeDefined();
    // After a successful detail fetch the card summary shows the truncated description.
    expect(
        screen.getByRole("button", {
            name: /tdd.*Test-driven development skill from the catalog page/,
        }),
    ).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "Review command" }));
    expect(
        await screen.findByText(
            "git fetch --depth 1 https://github.com/mattpocock/skills HEAD && git checkout --detach FETCH_HEAD",
        ),
    ).toBeDefined();
});

test("marketplace detail dialog can review and install", async () => {
    list.mockResolvedValueOnce(page([row])).mockResolvedValue(
        page([row, installedRow], null, 2),
    );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    fireEvent.click(
        await screen.findByRole("button", {
            name: /tdd/,
        }),
    );
    const detail = await screen.findByRole("dialog");
    fireEvent.click(
        within(detail).getByRole("button", { name: "Review command" }),
    );
    expect(
        await within(detail).findByText(
            "git fetch --depth 1 https://github.com/mattpocock/skills HEAD && git checkout --detach FETCH_HEAD",
        ),
    ).toBeDefined();
    fireEvent.click(within(detail).getByRole("button", { name: "Install" }));
    // Install dismisses the modal so the page footer owns progress / Done.
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
    });
    await waitFor(() => {
        expect(installMarketplace).toHaveBeenCalledWith({
            source: "mattpocock/skills",
            skillId: "tdd",
        });
    });
    expect(await screen.findByRole("button", { name: "Done" })).toBeDefined();
});

test("marketplace detail dialog shows a loading state then the real description", async () => {
    list.mockResolvedValue(page([row]));
    let finish:
        | ((value: { kind: "desktop"; skill: MarketplaceSkill }) => void)
        | undefined;
    getMarketplaceDetail.mockReset().mockImplementation(
        () =>
            new Promise((resolve) => {
                finish = resolve;
            }),
    );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    // List rows omit a fake source · skillId summary when description is null.
    const card = await screen.findByRole("button", { name: /tdd/ });
    expect(card.textContent).not.toContain("mattpocock/skills · tdd");
    expect(within(card).getByText("42 installs")).toBeDefined();

    fireEvent.click(card);
    const detail = await screen.findByRole("dialog");
    expect(within(detail).getByText("Loading description…")).toBeDefined();
    act(() => {
        finish?.({
            kind: "desktop",
            skill: {
                id: "mattpocock/skills/tdd",
                source: "mattpocock/skills",
                skillId: "tdd",
                name: "tdd",
                installs: 42,
                description:
                    "Test-driven development skill from the catalog page.",
                summary: null,
                catalogInstallCommand: null,
            },
        });
    });
    expect(
        await within(detail).findByText(
            "Test-driven development skill from the catalog page.",
        ),
    ).toBeDefined();
    expect(within(detail).queryByText("Loading description…")).toBeNull();
});

test("marketplace detail dialog reports a failed description fetch", async () => {
    list.mockResolvedValue(page([row]));
    getMarketplaceDetail
        .mockReset()
        .mockRejectedValue(
            new SkillRequestError("marketplace_response_invalid"),
        );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: /tdd/ }));
    const detail = await screen.findByRole("dialog");
    expect(
        await within(detail).findByText(
            "The skills.sh catalog returned a response vibemate cannot use.",
        ),
    ).toBeDefined();
    // No invented description appears in the dialog body.
    expect(
        within(detail).queryByText(
            "Test-driven development skill from the catalog page.",
        ),
    ).toBeNull();
    expect(getMarketplaceDetail).toHaveBeenCalledWith(
        "mattpocock/skills",
        "tdd",
    );
    // Facts remain so the user can still review/install.
    expect(within(detail).getByText("mattpocock/skills")).toBeDefined();
    expect(
        within(detail).getByRole("button", { name: "Review command" }),
    ).toBeDefined();
});

test("marketplace card summary truncates a long description", async () => {
    const long = "A".repeat(130) + " end-marker-should-not-appear-on-the-card";
    list.mockResolvedValue(page([row]));
    getMarketplaceDetail.mockReset().mockResolvedValue({
        kind: "desktop",
        skill: {
            id: "mattpocock/skills/tdd",
            source: "mattpocock/skills",
            skillId: "tdd",
            name: "tdd",
            installs: 42,
            description: long,
            summary: null,
            catalogInstallCommand: null,
        },
    });
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: /tdd/ }));
    const detail = await screen.findByRole("dialog");
    // Dialog shows the full description; the card clamps for the grid.
    expect(await within(detail).findByText(long)).toBeDefined();
    fireEvent.click(within(detail).getByRole("button", { name: "Close" }));
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
    });
    const card = screen.getByRole("button", { name: /tdd/ });
    expect(card.textContent).toContain("…");
    expect(card.textContent).not.toContain(
        "end-marker-should-not-appear-on-the-card",
    );
});

test("marketplace prefills card summaries for the current page without opening detail", async () => {
    list.mockResolvedValue(page([row]));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    // Background prefetch fills the visible card; no Dialog open required.
    expect(
        await screen.findByRole("button", {
            name: /tdd.*Test-driven development skill from the catalog page/,
        }),
    ).toBeDefined();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(getMarketplaceDetail).toHaveBeenCalledWith(
        "mattpocock/skills",
        "tdd",
    );
});

test("marketplace description prefetch covers only the current page", async () => {
    const many: MarketplaceSkill[] = Array.from({ length: 13 }, (_, index) => {
        const n = index + 1;
        const label = `skill-${String(n).padStart(2, "0")}`;
        return {
            id: `owner/repo/${label}`,
            source: "owner/repo",
            skillId: label,
            name: label,
            installs: n,
            description: null,
            summary: null,
            catalogInstallCommand: null,
        };
    });
    listMarketplace.mockReset().mockResolvedValue({
        kind: "desktop",
        skills: many,
    });
    list.mockResolvedValue(page([row]));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    await screen.findByRole("button", { name: /skill-01/ });
    await waitFor(() => {
        expect(getMarketplaceDetail).toHaveBeenCalled();
    });
    await waitFor(() => {
        expect(getMarketplaceDetail.mock.calls.length).toBe(
            MARKETPLACE_PAGE_SIZE,
        );
    });
    // Page-1 summaries appear; the 13th skill is not fetched yet.
    expect(
        await screen.findByRole("button", {
            name: /skill-01.*Summary for skill-01/,
        }),
    ).toBeDefined();
    expect(
        getMarketplaceDetail.mock.calls.some((call) => call[1] === "skill-13"),
    ).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("button", { name: /skill-13/ });
    await waitFor(() => {
        expect(
            getMarketplaceDetail.mock.calls.some(
                (call) => call[1] === "skill-13",
            ),
        ).toBe(true);
    });
    expect(
        await screen.findByRole("button", {
            name: /skill-13.*Summary for skill-13/,
        }),
    ).toBeDefined();
});

test("marketplace description prefetch ignores stale results after paging", async () => {
    const many: MarketplaceSkill[] = Array.from({ length: 13 }, (_, index) => {
        const n = index + 1;
        const label = `skill-${String(n).padStart(2, "0")}`;
        return {
            id: `owner/repo/${label}`,
            source: "owner/repo",
            skillId: label,
            name: label,
            installs: n,
            description: null,
            summary: null,
            catalogInstallCommand: null,
        };
    });
    const pending = new Map<
        string,
        (value: { kind: "desktop"; skill: MarketplaceSkill }) => void
    >();
    getMarketplaceDetail.mockReset().mockImplementation(
        (_source: string, skillId: string) =>
            new Promise((resolve) => {
                pending.set(skillId, resolve);
            }),
    );
    listMarketplace.mockReset().mockResolvedValue({
        kind: "desktop",
        skills: many,
    });
    list.mockResolvedValue(page([row]));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    await screen.findByRole("button", { name: /skill-01/ });
    await waitFor(() => {
        expect(pending.size).toBeGreaterThan(0);
    });
    // Leave page 1 while its detail fetches are still in flight.
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("button", { name: /skill-13/ });
    const page1Resolvers = [...pending.entries()].filter(
        ([skillId]) => skillId !== "skill-13",
    );
    act(() => {
        for (const [skillId, resolve] of page1Resolvers) {
            resolve({
                kind: "desktop",
                skill: {
                    id: `owner/repo/${skillId}`,
                    source: "owner/repo",
                    skillId,
                    name: skillId,
                    installs: 1,
                    description: `STALE ${skillId}`,
                    summary: null,
                    catalogInstallCommand: null,
                },
            });
        }
    });
    // Stale page-1 results must not paint on the catalog after paging away.
    fireEvent.click(screen.getByRole("button", { name: "Previous" }));
    const card = await screen.findByRole("button", { name: /skill-01/ });
    expect(card.textContent).not.toContain("STALE skill-01");
});

test("marketplace description prefetch keeps concurrency limited", async () => {
    const many: MarketplaceSkill[] = Array.from({ length: 6 }, (_, index) => {
        const n = index + 1;
        const label = `skill-${String(n).padStart(2, "0")}`;
        return {
            id: `owner/repo/${label}`,
            source: "owner/repo",
            skillId: label,
            name: label,
            installs: n,
            description: null,
            summary: null,
            catalogInstallCommand: null,
        };
    });
    let inFlight = 0;
    let maxInFlight = 0;
    const finishers: Array<() => void> = [];
    getMarketplaceDetail.mockReset().mockImplementation(
        (source: string, skillId: string) =>
            new Promise((resolve) => {
                inFlight += 1;
                maxInFlight = Math.max(maxInFlight, inFlight);
                finishers.push(() => {
                    inFlight -= 1;
                    resolve({
                        kind: "desktop",
                        skill: {
                            id: `${source}/${skillId}`,
                            source,
                            skillId,
                            name: skillId,
                            installs: 1,
                            description: `Summary for ${skillId}.`,
                            summary: null,
                            catalogInstallCommand: null,
                        },
                    });
                });
            }),
    );
    listMarketplace.mockReset().mockResolvedValue({
        kind: "desktop",
        skills: many,
    });
    list.mockResolvedValue(page([row]));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    await screen.findByRole("button", { name: /skill-01/ });
    await waitFor(() => {
        expect(finishers.length).toBe(MARKETPLACE_DETAIL_PREFETCH_CONCURRENCY);
    });
    expect(maxInFlight).toBe(MARKETPLACE_DETAIL_PREFETCH_CONCURRENCY);
    act(() => {
        // Release the first wave so the remaining skills can start.
        const firstWave = finishers.splice(0, finishers.length);
        for (const finish of firstWave) finish();
    });
    await waitFor(() => {
        expect(getMarketplaceDetail.mock.calls.length).toBe(6);
    });
    act(() => {
        const rest = finishers.splice(0, finishers.length);
        for (const finish of rest) finish();
    });
    expect(
        await screen.findByRole("button", {
            name: /skill-06.*Summary for skill-06/,
        }),
    ).toBeDefined();
    expect(maxInFlight).toBe(MARKETPLACE_DETAIL_PREFETCH_CONCURRENCY);
});

test("marketplace description prefetch failure leaves the card without invented text", async () => {
    list.mockResolvedValue(page([row]));
    getMarketplaceDetail
        .mockReset()
        .mockRejectedValue(
            new SkillRequestError("marketplace_response_invalid"),
        );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Browse marketplace" }),
    );
    const card = await screen.findByRole("button", { name: /tdd/ });
    await waitFor(() => {
        expect(getMarketplaceDetail).toHaveBeenCalled();
    });
    // Give the rejected prefetch a turn to clear pending state.
    await waitFor(() => {
        expect(card.textContent).not.toContain(
            "Test-driven development skill from the catalog page.",
        );
    });
    expect(card.textContent).not.toContain("mattpocock/skills · tdd");
    expect(within(card).getByText("42 installs")).toBeDefined();
    expect(screen.queryByRole("dialog")).toBeNull();
});

test("install shows the exact command before it runs", async () => {
    list.mockResolvedValueOnce(page([row])).mockResolvedValue(
        page([row, installedRow], null, 2),
    );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Install from source" }),
    );
    expect(
        await screen.findByRole("heading", {
            name: "Install a skill from source",
        }),
    ).toBeDefined();
    await waitFor(() => {
        expect(document.activeElement).toBe(
            screen.getByLabelText("Repository"),
        );
    });
    fireEvent.change(screen.getByLabelText("Repository"), {
        target: { value: "example/skills" },
    });
    fireEvent.change(screen.getByLabelText("Revision"), {
        target: { value: "v1.2.0" },
    });
    // Reviewing is the first step; no installer may run before confirmation.
    fireEvent.click(screen.getByRole("button", { name: "Review command" }));
    expect(
        await screen.findByText(
            "git fetch --depth 1 https://github.com/example/skills v1.2.0 && git checkout --detach FETCH_HEAD",
        ),
    ).toBeDefined();
    expect(preview).toHaveBeenCalledWith({
        method: "git",
        source: {
            repository: "example/skills",
            revision: "v1.2.0",
            skill: null,
        },
    });
    expect(install).not.toHaveBeenCalled();
    // The reviewed command replaces the review action with the confirmation.
    expect(document.activeElement).toBe(
        screen.getByRole("button", { name: "Install" }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Install" }));
    await waitFor(() => {
        expect(install).toHaveBeenCalledWith({
            method: "git",
            source: {
                repository: "example/skills",
                revision: "v1.2.0",
                skill: null,
            },
        });
    });
    // The dialog result and the global notification both state the count.
    expect(
        (await screen.findAllByText("Imported 1 skill.")).length,
    ).toBeGreaterThan(0);
    // The result replaces the confirmation; Done receives focus.
    expect(document.activeElement).toBe(
        screen.getByRole("button", { name: "Done" }),
    );
    // The row is re-read after the report, and the dialog stays for review.
    await waitFor(() => {
        expect(list).toHaveBeenCalledTimes(2);
    });
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    await waitFor(() => {
        expect(
            screen.queryByRole("heading", {
                name: "Install a skill from source",
            }),
        ).toBeNull();
    });
    expect(
        await screen.findByRole("heading", { name: "installed-skill" }),
    ).toBeDefined();
});

test("the npx install path reviews the reviewed installer version", async () => {
    preview.mockResolvedValue({ kind: "desktop", preview: npxPreview });
    install.mockResolvedValue({
        ...gitReport,
        method: "npx",
        revision: null,
        installer: "skills",
        installerVersion: "1.7.4",
        command: NPX_COMMAND,
        imported: [
            {
                ...installedRow,
                sourceRevision: null,
                fetchMethod: "npx",
                installerVersion: "1.7.4",
                installerCommand: NPX_COMMAND,
            },
        ],
    });
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Install from source" }),
    );
    await setFieldSelectValue(
        screen.getByRole("combobox", { name: "Source type" }),
        "npx",
    );
    fireEvent.change(screen.getByLabelText("Source"), {
        target: { value: "example/skills" },
    });
    // The Git revision field belongs to the other method and is gone.
    expect(screen.queryByLabelText("Revision")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Review command" }));
    expect(
        await screen.findByText(
            "npx --yes skills@1.7.4 add https://github.com/example/skills --skill '*' --copy -g -a claude-code -y",
        ),
    ).toBeDefined();
    expect(preview).toHaveBeenCalledWith({
        method: "npx",
        source: {
            source: "example/skills",
            installerVersion: null,
            skill: null,
        },
    });
    fireEvent.click(screen.getByRole("button", { name: "Install" }));
    await waitFor(() => {
        expect(install).toHaveBeenCalledTimes(1);
    });
    expect(await screen.findByText("skills@1.7.4")).toBeDefined();
});

test("a failed install stays in the dialog with a translated reason", async () => {
    install.mockRejectedValue(new SkillRequestError("installer_failed"));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Install from source" }),
    );
    fireEvent.change(screen.getByLabelText("Repository"), {
        target: { value: "example/skills" },
    });
    fireEvent.change(screen.getByLabelText("Revision"), {
        target: { value: "v1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Review command" }));
    fireEvent.click(await screen.findByRole("button", { name: "Install" }));
    expect(
        await screen.findByText(
            "The reviewed installer did not finish successfully. Nothing was imported.",
        ),
    ).toBeDefined();
    expect(
        screen.getByRole("heading", { name: "Install a skill from source" }),
    ).toBeDefined();
});

test("the install dialog reports a browser preview instead of running", async () => {
    preview.mockResolvedValue({ kind: "preview" });
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Install from source" }),
    );
    fireEvent.change(screen.getByLabelText("Repository"), {
        target: { value: "example/skills" },
    });
    fireEvent.change(screen.getByLabelText("Revision"), {
        target: { value: "v1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Review command" }));
    expect(
        await screen.findByText("This action needs the desktop app."),
    ).toBeDefined();
    expect(install).not.toHaveBeenCalled();
});

test("the source page can record a Git update method", async () => {
    const gitRecord: SkillRecord = {
        ...row,
        sourceUrl: "https://github.com/example/skills",
        sourceRevision: "v2",
        fetchMethod: "git",
        installerVersion: null,
        installerCommand:
            "git fetch --depth 1 https://github.com/example/skills v2 && git checkout --detach FETCH_HEAD",
        revision: 2,
    };
    save.mockResolvedValue(gitRecord);
    await setup();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Open details for demo-skill",
        }),
    );
    await setFieldSelectValue(
        await screen.findByRole("combobox", { name: "Update method" }),
        "git",
    );
    fireEvent.change(screen.getByLabelText("Source address"), {
        target: { value: "https://github.com/example/skills" },
    });
    fireEvent.change(screen.getByLabelText("Revision"), {
        target: { value: "v2" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
        expect(save).toHaveBeenCalledWith({
            id: ID,
            expectedRevision: 1,
            url: "https://github.com/example/skills",
            revision: "v2",
            fetchMethod: "git",
            installerVersion: null,
        });
    });
    expect(
        await screen.findByText(
            "https://github.com/example/skills · revision v2 · Git",
        ),
    ).toBeDefined();
});

test("the source page records an npx method and installer version", async () => {
    const npxRecord: SkillRecord = {
        ...row,
        sourceUrl: "https://github.com/example/skills",
        sourceRevision: null,
        fetchMethod: "npx",
        installerVersion: "1.7.4",
        installerCommand:
            "npx --yes skills@1.7.4 add https://github.com/example/skills --skill demo-skill --copy -g -a claude-code -y",
        revision: 2,
    };
    save.mockResolvedValue(npxRecord);
    await setup();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Open details for demo-skill",
        }),
    );
    await setFieldSelectValue(
        await screen.findByRole("combobox", { name: "Update method" }),
        "npx",
    );
    fireEvent.change(screen.getByLabelText("Source address"), {
        target: { value: "https://github.com/example/skills" },
    });
    // The Git revision no longer applies; the installer version takes its place.
    expect(screen.queryByLabelText("Revision")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
        expect(save).toHaveBeenCalledWith({
            id: ID,
            expectedRevision: 1,
            url: "https://github.com/example/skills",
            revision: null,
            fetchMethod: "npx",
            installerVersion: null,
        });
    });
    expect(
        await screen.findByText(
            "https://github.com/example/skills · skills@1.7.4",
        ),
    ).toBeDefined();
});

test("the browser preview reads nothing", async () => {
    list.mockResolvedValue({ kind: "preview" });
    await setup();
    expect(
        await screen.findByText(
            "Importing and listing skills needs the desktop app. This browser preview reads nothing.",
        ),
    ).toBeDefined();
    expect(screen.queryByRole("heading", { name: "demo-skill" })).toBeNull();
});

test("the Chinese page shows the same states and controls", async () => {
    await setup("zh-CN");
    expect(
        await screen.findByRole("heading", { level: 1, name: "技能" }),
    ).toBeDefined();
    expect(screen.getByRole("button", { name: "导入" })).toBeDefined();
    // The list arrives asynchronously; wait for the first row before reading it.
    expect(
        await screen.findByRole("heading", { name: "demo-skill" }),
    ).toBeDefined();
    expect(screen.getByText("来源：本地导入")).toBeDefined();
    // The delete action and its confirmation are translated as well.
    fireEvent.click(screen.getByRole("button", { name: "删除“demo-skill”" }));
    expect(await screen.findByText("删除技能？")).toBeDefined();
    expect(
        screen.getByText(
            "从技能库删除“demo-skill”并移除 vibemate 的副本？导入来源文件夹不会被修改。",
        ),
    ).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    fireEvent.click(
        screen.getByRole("button", { name: "查看 demo-skill 的详情" }),
    );
    expect(
        await screen.findByRole("heading", { level: 1, name: "技能详情" }),
    ).toBeDefined();
    expect(screen.getByText("2 个文件")).toBeDefined();
    expect(screen.getByText("导入自")).toBeDefined();
});

test("the sidebar home returns from the detail page", async () => {
    const homeRef = createRef<ModuleNavigationHandle>();
    await setup("en", homeRef);
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Open details for demo-skill",
        }),
    );
    await screen.findByLabelText("Source address");
    act(() => {
        expect(homeRef.current?.openHome()).toBe(true);
    });
    expect(screen.queryByLabelText("Source address")).toBeNull();
    expect(screen.getByRole("heading", { name: "Skills" })).toBeDefined();
});
