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
import {
    listMcpDefinitions,
    getMcpDefinition,
    saveMcpDefinition,
    cleanupMcpCredentials,
    deleteMcpDefinition,
    cleanupDeletedMcpCredentials,
    McpRequestError,
    type McpRecord,
} from "../../lib/desktop/mcp";
import { McpView } from "./McpView";
import {
    GrokMcpError,
    applyGrokMcpChange,
    readGrokMcpState,
    readGrokMcpHistory,
    previewGrokMcpChange,
} from "../../lib/desktop/grokMcp";
import { emptyGrokMcpState, grokMcpPlan } from "../../test/grokMcpFixtures";
vi.mock(import("../../lib/desktop/grokMcp"), async (original) => ({
    ...(await original()),
    readGrokMcpState: vi.fn(),
    readGrokMcpHistory: vi.fn(),
    previewGrokMcpChange: vi.fn(),
    applyGrokMcpChange: vi.fn(),
}));
import {
    PiMcpError,
    applyPiMcpChange,
    readPiMcpState,
    readPiMcpHistory,
    previewPiMcpChange,
} from "../../lib/desktop/piMcp";
import {
    emptyPiMcpState,
    piMcpDeployment,
    piMcpPlan,
} from "../../test/piMcpFixtures";
vi.mock(import("../../lib/desktop/piMcp"), async (original) => ({
    ...(await original()),
    readPiMcpState: vi.fn(),
    readPiMcpHistory: vi.fn(),
    previewPiMcpChange: vi.fn(),
    applyPiMcpChange: vi.fn(),
}));
vi.mock(import("../../lib/desktop/mcp"), async (importOriginal) => ({
    ...(await importOriginal()),
    listMcpDefinitions: vi.fn(),
    getMcpDefinition: vi.fn(),
    saveMcpDefinition: vi.fn(),
    cleanupMcpCredentials: vi.fn(),
    deleteMcpDefinition: vi.fn(),
    cleanupDeletedMcpCredentials: vi.fn(),
}));
const list = vi.mocked(listMcpDefinitions);
const get = vi.mocked(getMcpDefinition);
const save = vi.mocked(saveMcpDefinition);
const clean = vi.mocked(cleanupMcpCredentials);
const row: McpRecord = {
    id: "0123456789abcdef0123456789abcdef",
    displayName: "Tools",
    serverName: "tools",
    enabled: true,
    revision: 1,
    connection: {
        type: "stdio",
        command: "not-a-real-server",
        args: [],
        cwd: null,
        env: ["TOKEN"],
    },
    createdAtMs: 1,
    updatedAtMs: 1,
    cleanupPending: false,
};
function page(
    items: McpRecord[],
    nextCursor: string | null = null,
    total = items.length,
) {
    return {
        kind: "desktop" as const,
        page: { items, nextCursor, total, deletedCleanupPending: false },
    };
}
async function setup(
    locale: "en" | "zh-CN" = "en",
    homeRef?: Ref<ModuleNavigationHandle>,
) {
    const i18n = await createAppI18n(locale);
    return render(
        <StrictMode>
            <I18nextProvider i18n={i18n}>
                <McpView {...(homeRef === undefined ? {} : { homeRef })} />
            </I18nextProvider>
        </StrictMode>,
    );
}
/** Open McpDetail via the list Detail IconButton (not the whole row). */
async function openDetail(name = "Tools"): Promise<void> {
    fireEvent.click(
        await screen.findByRole("button", {
            name: `Open details for ${name}`,
        }),
    );
    await screen.findByRole("heading", { name: name });
}
beforeEach(() => {
    vi.mocked(readGrokMcpState)
        .mockReset()
        .mockResolvedValue(emptyGrokMcpState);
    vi.mocked(readGrokMcpHistory)
        .mockReset()
        .mockResolvedValue({ items: [], nextCursor: null });
    vi.mocked(previewGrokMcpChange)
        .mockReset()
        .mockResolvedValue({ ...grokMcpPlan, definitionId: row.id });
    vi.mocked(applyGrokMcpChange).mockReset();
    vi.mocked(readPiMcpState).mockReset().mockResolvedValue(emptyPiMcpState);
    vi.mocked(readPiMcpHistory)
        .mockReset()
        .mockResolvedValue({ items: [], nextCursor: null });
    vi.mocked(previewPiMcpChange)
        .mockReset()
        .mockResolvedValue({ ...piMcpPlan, definitionId: row.id });
    list.mockReset().mockResolvedValue(page([row]));
    get.mockReset().mockResolvedValue(row);
    save.mockReset().mockResolvedValue({ ...row, revision: 2, enabled: false });
    clean.mockReset().mockResolvedValue(row);
    vi.mocked(deleteMcpDefinition)
        .mockReset()
        .mockResolvedValue({ id: row.id, cleanupPending: false });
    vi.mocked(cleanupDeletedMcpCredentials).mockReset().mockResolvedValue(true);
});
afterEach(() => {
    cleanup();
});

test("Grok inject row shows path and Register IconButton; no dialog until clicked", async () => {
    const homeRef = createRef<ModuleNavigationHandle>();
    await setup("en", homeRef);
    await openDetail();
    expect(await screen.findByText(emptyGrokMcpState.targetPath)).toBeDefined();
    expect(readGrokMcpState).toHaveBeenCalledWith(row.id);
    expect(
        screen.queryByRole("button", { name: "Manage Grok Build · Tools" }),
    ).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(
        await screen.findByRole("button", {
            name: "Refresh registration status · Grok Build · Tools",
        }),
    ).toBeDefined();
    expect(
        await screen.findByRole("button", {
            name: "Register with Grok Build · Tools",
        }),
    ).toBeDefined();
    expect(
        screen.getByRole("button", {
            name: "Grok Build registration history · Tools",
        }),
    ).toBeDefined();
    // Clear rows do not open a manager dialog, so home can leave.
    expect(homeRef.current?.openHome()).toBe(true);
    expect(save).not.toHaveBeenCalled();
    expect(deleteMcpDefinition).not.toHaveBeenCalled();
    expect(applyPiMcpChange).not.toHaveBeenCalled();
    expect(applyGrokMcpChange).not.toHaveBeenCalled();
});

test("Grok stale revision reloads the list instead of editing or deploying", async () => {
    vi.mocked(previewGrokMcpChange).mockRejectedValue(
        new GrokMcpError("definition_changed"),
    );
    await setup();
    await openDetail();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Register with Grok Build · Tools",
        }),
    );
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByRole("alert");
    const before = list.mock.calls.length;
    fireEvent.click(
        within(dialog).getByRole("button", { name: "Reload definitions" }),
    );
    await waitFor(() => {
        expect(list.mock.calls.length).toBeGreaterThan(before);
    });
    expect(save).not.toHaveBeenCalled();
    expect(applyGrokMcpChange).not.toHaveBeenCalled();
});

test("hiding MCP unmounts inject rows before a late Grok read can paint", async () => {
    let finish: (value: typeof emptyGrokMcpState) => void = () => {};
    vi.mocked(readGrokMcpState).mockReturnValue(
        new Promise((resolve) => {
            finish = resolve;
        }),
    );
    const i18n = await createAppI18n("en");
    const view = (hidden: boolean) => (
        <StrictMode>
            <I18nextProvider i18n={i18n}>
                <McpView hidden={hidden} />
            </I18nextProvider>
        </StrictMode>
    );
    const rendered = render(view(false));
    await openDetail();
    expect(
        screen.getByRole("region", { name: "Grok Build registration · Tools" }),
    ).toBeDefined();
    expect(screen.queryByRole("dialog")).toBeNull();
    rendered.rerender(view(true));
    await act(async () => {
        finish(emptyGrokMcpState);
        await Promise.resolve();
    });
    // Still hidden: late resolve must not paint path into another module.
    expect(screen.queryByText(emptyGrokMcpState.targetPath)).toBeNull();
    expect(save).not.toHaveBeenCalled();
});

test("hiding MCP ignores a late Pi history failure from an unmounted history dialog", async () => {
    let fail: (error: Error) => void = () => {};
    vi.mocked(readPiMcpHistory).mockReturnValue(
        new Promise((_, reject) => {
            fail = reject;
        }),
    );
    const i18n = await createAppI18n("en");
    const view = (hidden: boolean) => (
        <StrictMode>
            <I18nextProvider i18n={i18n}>
                <McpView hidden={hidden} />
            </I18nextProvider>
        </StrictMode>
    );
    const rendered = render(view(false));
    await openDetail();
    // Open history so the Manager mounts and starts a history read.
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Pi registration history · Tools",
        }),
    );
    await screen.findByRole("dialog");
    rendered.rerender(view(true));
    await act(async () => {
        fail(new PiMcpError("outcome_unknown"));
        await Promise.resolve();
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(save).not.toHaveBeenCalled();
});

test("Pi preview reads a list revision and reloads without mutating the definition", async () => {
    vi.mocked(previewPiMcpChange).mockRejectedValue(
        new PiMcpError("definition_changed"),
    );
    await setup();
    await openDetail();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Register with Pi · Tools",
        }),
    );
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByRole("alert");
    expect(previewPiMcpChange).toHaveBeenCalledWith({
        definitionId: row.id,
        expectedRevision: row.revision,
        action: "deploy",
        historyId: null,
    });
    fireEvent.click(
        within(dialog).getByRole("button", { name: "Reload definitions" }),
    );
    await waitFor(() => {
        expect(list).toHaveBeenCalledTimes(2);
    });
    expect(save).not.toHaveBeenCalled();
    expect(clean).not.toHaveBeenCalled();
    expect(deleteMcpDefinition).not.toHaveBeenCalled();
    expect(
        (await screen.findAllByText(/^Not registered$/)).length,
    ).toBeGreaterThan(0);
});

test("deletion lists Pi references from other roots and cannot orphan them", async () => {
    vi.mocked(readPiMcpState).mockImplementation((id) =>
        Promise.resolve(
            id == null
                ? emptyPiMcpState
                : {
                      ...emptyPiMcpState,
                      deployments: [
                          {
                              ...piMcpDeployment,
                              definitionId: row.id,
                              configRoot: "/older/pi-directory",
                          },
                      ],
                  },
        ),
    );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Delete “Tools”" }),
    );
    await screen.findByText(/Pi reference: \/older\/pi-directory/);
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(deleteMcpDefinition).not.toHaveBeenCalled();
    fireEvent.click(
        screen.getByRole("button", { name: "Manage Pi references" }),
    );
    await screen.findByRole("dialog", { name: "Pi registration · Tools" });
    expect(deleteMcpDefinition).not.toHaveBeenCalled();
});

test("unknown reference reads block deletion and provide a safe retry", async () => {
    vi.mocked(readPiMcpState).mockImplementation((id) =>
        id == null
            ? Promise.resolve(emptyPiMcpState)
            : Promise.reject(new PiMcpError("storage_unavailable")),
    );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Delete “Tools”" }),
    );
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(deleteMcpDefinition).not.toHaveBeenCalled();
    vi.mocked(readPiMcpState).mockResolvedValue(emptyPiMcpState);
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => {
        expect(
            screen
                .getByRole("button", { name: "Delete" })
                .getAttribute("aria-disabled"),
        ).toBe("false");
    });
});

test("hiding a confirmed Pi write retains the home guard until its result settles", async () => {
    const homeRef = createRef<ModuleNavigationHandle>();
    let resolve: (value: {
        changeId: string;
        registered: boolean;
        reloadRequired: boolean;
    }) => void = () => {};
    vi.mocked(applyPiMcpChange).mockReturnValue(
        new Promise((done) => {
            resolve = done;
        }),
    );
    const i18n = await createAppI18n("en");
    const view = (hidden: boolean) => (
        <I18nextProvider i18n={i18n}>
            <McpView hidden={hidden} homeRef={homeRef} />
        </I18nextProvider>
    );
    const rendered = render(view(false));
    await openDetail();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Register with Pi · Tools",
        }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
        await within(dialog).findByRole("button", {
            name: "Confirm file change",
        }),
    );
    rendered.rerender(view(true));
    expect(homeRef.current?.openHome()).toBe(false);
    await act(async () => {
        resolve({
            changeId: "b".repeat(32),
            registered: true,
            reloadRequired: true,
        });
        await Promise.resolve();
    });
    expect(homeRef.current?.openHome()).toBe(true);
});

test("central enablement does not imply Agent registration", async () => {
    await setup("en");
    await screen.findByText("Tools");
    // List no longer shows per-Agent registration; that lives on detail.
    expect(
        screen.queryByRole("button", { name: "Manage Pi · Tools" }),
    ).toBeNull();
    expect(
        screen.queryByRole("button", { name: "Manage Grok Build · Tools" }),
    ).toBeNull();
    expect(save).not.toHaveBeenCalled();
    fireEvent.click(
        screen.getByRole("button", { name: "Disable “Tools” definition" }),
    );
    await screen.findByRole("button", {
        name: "Enable “Tools” definition",
    });
    expect(clean).not.toHaveBeenCalled();
    expect(get).not.toHaveBeenCalled();
    await openDetail();
    expect(
        (await screen.findAllByText(/^Not registered$/)).length,
    ).toBeGreaterThan(0);
});

test("detail IconButton opens detail; row body and edit/enable/delete do not", async () => {
    await setup();
    await screen.findByText("Tools");
    expect(
        screen.queryByRole("button", { name: "Manage Pi · Tools" }),
    ).toBeNull();
    // Whole-row click is gone: the name text is not a button.
    expect(
        screen.queryByRole("button", { name: "Open details for Tools" }),
    ).not.toBeNull();
    const beforeRow = get.mock.calls.length;
    fireEvent.click(screen.getByText("Tools"));
    expect(get.mock.calls.length).toBe(beforeRow);
    expect(screen.queryByRole("heading", { name: "Tools" })).toBeNull();

    fireEvent.click(
        screen.getByRole("button", { name: "Open details for Tools" }),
    );
    expect(await screen.findByRole("heading", { name: "Tools" })).toBeDefined();
    expect(get).toHaveBeenCalledWith(row.id);
    fireEvent.click(
        screen.getByRole("button", { name: "Back to MCP servers" }),
    );
    await screen.findByRole("button", { name: "Edit “Tools”" });
    const before = get.mock.calls.length;
    fireEvent.click(screen.getByRole("button", { name: "Edit “Tools”" }));
    await screen.findByLabelText(/^Name$/);
    expect(get.mock.calls.length).toBeGreaterThan(before);
    expect(screen.queryByText("Agent registration")).toBeNull();
});

test("detail shows definition + clear inject rows; header Edit primary; no delete", async () => {
    await setup();
    await openDetail();
    expect(screen.getByRole("heading", { name: "Tools" })).toBeDefined();
    expect(screen.getByText(/Definition enabled · stdio/)).toBeDefined();
    expect(screen.getByRole("region", { name: "Definition" })).toBeDefined();
    expect(
        screen.queryByRole("region", { name: "Enable and edit" }),
    ).toBeNull();
    expect(screen.queryByText(/Toggle enablement from the list/)).toBeNull();
    expect(
        screen.getByRole("region", { name: "Agent registration" }),
    ).toBeDefined();
    expect(screen.queryByRole("tab", { name: "Pi" })).toBeNull();
    expect(screen.queryByRole("tab", { name: "Grok Build" })).toBeNull();
    expect(
        screen.queryByRole("button", { name: "Manage Pi · Tools" }),
    ).toBeNull();
    expect(
        screen.queryByRole("button", { name: "Manage Grok Build · Tools" }),
    ).toBeNull();
    const pi = await screen.findByRole("region", {
        name: "Pi registration · Tools",
    });
    const grok = screen.getByRole("region", {
        name: "Grok Build registration · Tools",
    });
    expect(
        await within(pi).findByText(emptyPiMcpState.targetPath),
    ).toBeDefined();
    expect(
        await within(grok).findByText(emptyGrokMcpState.targetPath),
    ).toBeDefined();
    expect((await screen.findAllByText(/^Not registered$/)).length).toBe(2);
    // Unregistered rows expose IconButtons; no text primary deploy on the row.
    expect(
        within(pi).getByRole("button", {
            name: "Refresh registration status · Pi · Tools",
        }),
    ).toBeDefined();
    expect(
        within(pi).getByRole("button", { name: "Register with Pi · Tools" }),
    ).toBeDefined();
    expect(
        within(pi).getByRole("button", {
            name: "Pi registration history · Tools",
        }),
    ).toBeDefined();
    expect(
        within(grok).getByRole("button", {
            name: "Refresh registration status · Grok Build · Tools",
        }),
    ).toBeDefined();
    expect(
        within(grok).getByRole("button", {
            name: "Register with Grok Build · Tools",
        }),
    ).toBeDefined();
    expect(
        within(grok).getByRole("button", {
            name: "Grok Build registration history · Tools",
        }),
    ).toBeDefined();
    expect(
        within(pi).queryByRole("button", { name: "Review deployment" }),
    ).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
    const edit = screen.getByRole("button", { name: "Edit" });
    expect(edit.className).toMatch(/primary/);
    const definition = screen.getByRole("region", { name: "Definition" });
    expect(definition.textContent).not.toMatch(/Definition enabled/);
    expect(screen.queryByRole("button", { name: /Delete “Tools”/ })).toBeNull();
    // Agent marks from the shared Agent list assets.
    expect(pi.querySelector('[data-agent="pi"]')).not.toBeNull();
    expect(grok.querySelector('[data-agent="grokBuild"]')).not.toBeNull();
    fireEvent.click(edit);
    await screen.findByLabelText(/^Name$/);
});

test("row Refresh IconButton re-reads Agent registration status", async () => {
    await setup();
    await openDetail();
    const pi = await screen.findByRole("region", {
        name: "Pi registration · Tools",
    });
    await within(pi).findByText(emptyPiMcpState.targetPath);
    const before = vi.mocked(readPiMcpState).mock.calls.length;
    fireEvent.click(
        within(pi).getByRole("button", {
            name: "Refresh registration status · Pi · Tools",
        }),
    );
    await waitFor(() => {
        expect(vi.mocked(readPiMcpState).mock.calls.length).toBeGreaterThan(
            before,
        );
    });
    expect(await within(pi).findByText(/^Not registered$/)).toBeDefined();
});

test("Register name conflict shows warning dialog only; no empty registration form", async () => {
    vi.mocked(previewPiMcpChange).mockRejectedValue(
        new PiMcpError("name_conflict"),
    );
    await setup();
    await openDetail();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Register with Pi · Tools",
        }),
    );
    const dialog = await screen.findByRole("dialog", {
        name: "Pi registration · Tools",
    });
    const alert = await within(dialog).findByRole("alert");
    expect(alert.textContent).toMatch(/owns this name|hyphen\/underscore/i);
    // Conflict stays a warning popup: no review confirm, no registration facts.
    expect(
        within(dialog).queryByRole("button", { name: "Confirm file change" }),
    ).toBeNull();
    expect(
        within(dialog).queryByRole("region", { name: /registration/i }),
    ).toBeNull();
    expect(
        within(dialog).queryByRole("button", {
            name: "Refresh registration",
        }),
    ).toBeNull();
    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(applyPiMcpChange).not.toHaveBeenCalled();
});

test("Register IconButton opens review dialog; History opens history dialog; cancel closes", async () => {
    await setup();
    await openDetail();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Register with Pi · Tools",
        }),
    );
    const registerDialog = await screen.findByRole("dialog", {
        name: "Pi registration · Tools",
    });
    expect(
        await within(registerDialog).findByRole("button", {
            name: "Confirm file change",
        }),
    ).toBeDefined();
    fireEvent.click(
        within(registerDialog).getByRole("button", { name: "Close" }),
    );
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
    });
    fireEvent.click(
        screen.getByRole("button", {
            name: "Pi registration history · Tools",
        }),
    );
    const historyDialog = await screen.findByRole("dialog", {
        name: "Pi registration history · Tools",
    });
    expect(
        within(historyDialog).queryByRole("button", {
            name: "Review deployment",
        }),
    ).toBeNull();
    expect(within(historyDialog).getByText("Entry history")).toBeDefined();
    fireEvent.click(
        within(historyDialog).getByRole("button", { name: "Close" }),
    );
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(applyPiMcpChange).not.toHaveBeenCalled();
});

test("the stored definition total is stated above the list and omitted when empty", async () => {
    // 9 definitions, one page loaded: the count describes the library.
    list.mockResolvedValue(page([row], null, 9));
    await setup();
    expect(await screen.findByText("9 MCP servers in total")).toBeDefined();

    // An empty list keeps its empty-state sentence instead of a zero count.
    cleanup();
    list.mockResolvedValue(page([], null, 0));
    await setup();
    expect(
        await screen.findByText("No MCP definitions saved yet."),
    ).toBeDefined();
    expect(screen.queryByText(/in total/)).toBeNull();
});

test("sidebar home cannot abandon pending or uncertain MCP writes", async () => {
    const homeRef = createRef<ModuleNavigationHandle>();
    let reject: (reason: Error) => void = () => {};
    save.mockReturnValue(
        new Promise((_, fail) => {
            reject = fail;
        }),
    );
    await setup("en", homeRef);
    fireEvent.click(
        await screen.findByRole("button", { name: "Edit “Tools”" }),
    );
    await screen.findByLabelText(/^Name$/);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    act(() => {
        expect(homeRef.current?.openHome()).toBe(false);
    });
    expect(screen.getByLabelText(/^Name$/)).toBeDefined();
    await act(async () => {
        reject(new McpRequestError("outcome_unknown"));
        await Promise.resolve();
    });
    await screen.findByRole("alert");
    act(() => {
        expect(homeRef.current?.openHome()).toBe(false);
    });
    expect(screen.getByLabelText(/^Name$/)).toBeDefined();
});

test("a late MCP edit read cannot reopen an editor after sidebar home", async () => {
    const homeRef = createRef<ModuleNavigationHandle>();
    let resolve: (record: McpRecord) => void = () => {};
    get.mockReturnValue(
        new Promise((done) => {
            resolve = done;
        }),
    );
    await setup("en", homeRef);
    fireEvent.click(
        await screen.findByRole("button", { name: "Edit “Tools”" }),
    );
    act(() => {
        expect(homeRef.current?.openHome()).toBe(true);
    });
    await act(async () => {
        resolve(row);
        await Promise.resolve();
    });
    expect(screen.queryByLabelText(/^Name$/)).toBeNull();
    expect(
        await screen.findByRole("button", { name: "Edit “Tools”" }),
    ).toBeDefined();
});

test("preview blocks creation without displaying fake definitions", async () => {
    list.mockResolvedValue({ kind: "preview" });
    await setup();
    await screen.findByText(/Browser preview cannot read or save/);
    fireEvent.click(screen.getByRole("button", { name: "New" }));
    expect(screen.queryByRole("form")).toBeNull();
    expect(save).not.toHaveBeenCalled();
});

test("list failures offer a read retry", async () => {
    list.mockRejectedValueOnce(new McpRequestError("read_failed"));
    await setup();
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await screen.findByText("Tools");
});

test("new definition saves and returns to a re-read list", async () => {
    list.mockResolvedValue(page([]));
    save.mockResolvedValue(row);
    await setup();
    await screen.findByText("No MCP definitions saved yet.");
    fireEvent.click(screen.getByRole("button", { name: "New" }));
    fireEvent.change(screen.getByLabelText(/^Name$/), {
        target: { value: "Tools" },
    });
    fireEvent.change(screen.getByLabelText(/^Executable/), {
        target: { value: "not-a-real-server" },
    });
    list.mockResolvedValue(page([row]));
    fireEvent.click(screen.getByRole("button", { name: /^Save$/ }));
    await screen.findByText("Tools");
    expect(save).toHaveBeenCalledOnce();
    expect(screen.queryByRole("form")).toBeNull();
});

test("enablement changes preserve credential names without reading their values", async () => {
    await setup();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Disable “Tools” definition",
        }),
    );
    await screen.findByRole("button", {
        name: "Enable “Tools” definition",
    });
    expect(save).toHaveBeenCalledWith({
        id: row.id,
        expectedRevision: 1,
        displayName: "Tools",
        serverName: "tools",
        enabled: false,
        connection: {
            type: "stdio",
            command: "not-a-real-server",
            args: [],
            cwd: null,
            env: [{ name: "TOKEN", value: null }],
        },
    });
});

test("unknown toggle result blocks every mutation until a successful list reload", async () => {
    save.mockRejectedValue(new McpRequestError("outcome_unknown"));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Disable “Tools” definition",
        }),
    );
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "New" }));
    fireEvent.click(screen.getByRole("button", { name: "Edit “Tools”" }));
    fireEvent.click(
        screen.getByRole("button", { name: "Disable “Tools” definition" }),
    );
    expect(get).not.toHaveBeenCalled();
    expect(save).toHaveBeenCalledOnce();
    fireEvent.click(
        screen.getByRole("button", { name: "Reload before retrying" }),
    );
    await waitFor(() => {
        expect(screen.queryByRole("alert")).toBeNull();
    });
    fireEvent.click(screen.getByRole("button", { name: "Edit “Tools”" }));
    await screen.findByRole("form", { name: "Tools" });
});

test("failed edit reconciliation replaces stale editable rows with read error", async () => {
    get.mockRejectedValue(new McpRequestError("read_failed"));
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Edit “Tools”" }),
    );
    await screen.findByRole("alert");
    expect(screen.queryByRole("button", { name: "Edit “Tools”" })).toBeNull();
    expect(
        screen
            .getByRole("button", { name: "New" })
            .getAttribute("aria-disabled"),
    ).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await screen.findByText("Tools");
});

test("cleanup remains visible and retryable until old credentials are removed", async () => {
    list.mockResolvedValue(page([{ ...row, cleanupPending: true }]));
    clean
        .mockResolvedValueOnce({ ...row, cleanupPending: true })
        .mockResolvedValue(row);
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Retry credential cleanup" }),
    );
    await waitFor(() => {
        expect(clean).toHaveBeenCalledOnce();
    });
    await waitFor(() => {
        expect(
            screen
                .getByRole("button", { name: "Retry credential cleanup" })
                .getAttribute("aria-disabled"),
        ).toBe("false");
    });
    fireEvent.click(
        screen.getByRole("button", { name: "Retry credential cleanup" }),
    );
    await waitFor(() => {
        expect(
            screen.queryByRole("button", { name: "Retry credential cleanup" }),
        ).toBeNull();
    });
    expect(clean).toHaveBeenCalledTimes(2);
});

test("cursor pages append without replacing existing definitions", async () => {
    list.mockResolvedValueOnce(page([row], row.id)).mockResolvedValue(
        page([
            {
                ...row,
                id: "f".repeat(32),
                displayName: "Second",
                serverName: "second",
            },
        ]),
    );
    await setup();
    fireEvent.click(await screen.findByRole("button", { name: "Load more" }));
    await screen.findByText("Second");
    expect(screen.getByText("Tools")).toBeDefined();
    expect(list).toHaveBeenLastCalledWith(row.id);
});

test("unmounted lists ignore late read replies", async () => {
    let resolve: (result: ReturnType<typeof page>) => void = () => {};
    list.mockImplementation(
        () =>
            new Promise((done) => {
                resolve = done;
            }),
    );
    const view = await setup();
    await waitFor(() => {
        expect(list).toHaveBeenCalled();
    });
    view.unmount();
    await act(async () => {
        resolve(page([row]));
        await Promise.resolve();
    });
    expect(screen.queryByText("Tools")).toBeNull();
});

test("delete requires confirmation, cancellation restores focus, and success reloads counts", async () => {
    await setup();
    const trigger = await screen.findByRole("button", {
        name: "Delete “Tools”",
    });
    trigger.focus();
    fireEvent.click(trigger);
    await screen.findByRole("dialog");
    expect(deleteMcpDefinition).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
    });
    await waitFor(() => {
        expect(document.activeElement).toBe(trigger);
    });
    fireEvent.click(trigger);
    list.mockResolvedValue(page([]));
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    await screen.findByText("No MCP definitions saved yet.");
    expect(deleteMcpDefinition).toHaveBeenCalledExactlyOnceWith({
        id: row.id,
        expectedRevision: 1,
    });
    expect(screen.queryByRole("button", { name: "Delete “Tools”" })).toBeNull();
});

test("pending delete prevents duplicate submissions, dismissal and home navigation", async () => {
    const homeRef = createRef<ModuleNavigationHandle>();
    let resolve: (result: {
        id: string;
        cleanupPending: boolean;
    }) => void = () => {};
    vi.mocked(deleteMcpDefinition).mockReturnValue(
        new Promise((done) => {
            resolve = done;
        }),
    );
    await setup("en", homeRef);
    fireEvent.click(
        await screen.findByRole("button", { name: "Delete “Tools”" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    fireEvent.click(screen.getByRole("button", { name: "Deleting…" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(deleteMcpDefinition).toHaveBeenCalledOnce();
    act(() => {
        expect(homeRef.current?.openHome()).toBe(false);
    });
    expect(screen.getByRole("dialog")).toBeDefined();
    list.mockResolvedValue(page([]));
    await act(async () => {
        resolve({ id: row.id, cleanupPending: false });
        await Promise.resolve();
    });
    await screen.findByText("No MCP definitions saved yet.");
});

test.each(["outcome_unknown", "revision_conflict", "not_found"] as const)(
    "%s delete blocks mutations until reconciliation succeeds",
    async (code) => {
        vi.mocked(deleteMcpDefinition).mockRejectedValue(
            new McpRequestError(code),
        );
        await setup();
        fireEvent.click(
            await screen.findByRole("button", { name: "Delete “Tools”" }),
        );
        fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
        await screen.findByRole("alert");
        fireEvent.click(screen.getByRole("button", { name: "Delete “Tools”" }));
        expect(deleteMcpDefinition).toHaveBeenCalledOnce();
        expect(screen.queryByRole("dialog")).toBeNull();
        list.mockRejectedValueOnce(new McpRequestError("read_failed"));
        fireEvent.click(
            screen.getByRole("button", { name: "Reload before retrying" }),
        );
        await screen.findByRole("button", { name: "Try again" });
        expect(
            screen.queryByRole("button", { name: "Delete “Tools”" }),
        ).toBeNull();
        fireEvent.click(screen.getByRole("button", { name: "Try again" }));
        await screen.findByRole("button", { name: "Delete “Tools”" });
    },
);

test("known delete failure keeps the definition and permits another confirmation", async () => {
    vi.mocked(deleteMcpDefinition).mockRejectedValueOnce(
        new McpRequestError("write_failed"),
    );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Delete “Tools”" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    await screen.findByRole("alert");
    expect(screen.getByText("Tools")).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "Delete “Tools”" }));
    await screen.findByRole("dialog");
});

test("deleted credential cleanup remains retryable with an empty library after restart", async () => {
    list.mockResolvedValue({
        kind: "desktop",
        page: {
            items: [],
            nextCursor: null,
            total: 0,
            deletedCleanupPending: true,
        },
    });
    vi.mocked(cleanupDeletedMcpCredentials)
        .mockResolvedValueOnce(false)
        .mockResolvedValue(true);
    await setup();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Retry deleted credential cleanup",
        }),
    );
    await waitFor(() => {
        expect(cleanupDeletedMcpCredentials).toHaveBeenCalledOnce();
    });
    await waitFor(() => {
        expect(
            screen
                .getByRole("button", {
                    name: "Retry deleted credential cleanup",
                })
                .getAttribute("aria-disabled"),
        ).toBe("false");
    });
    list.mockResolvedValue(page([]));
    fireEvent.click(
        screen.getByRole("button", {
            name: "Retry deleted credential cleanup",
        }),
    );
    await waitFor(() => {
        expect(
            screen.queryByRole("button", {
                name: "Retry deleted credential cleanup",
            }),
        ).toBeNull();
    });
});
