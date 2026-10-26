import {
    act,
    cleanup,
    fireEvent,
    render,
    screen,
    waitFor,
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
    GrokMcpPreviewError,
    previewGrokMcpRegistration,
    type GrokMcpPreview,
} from "../../lib/desktop/grokMcpPreview";
import { grokMcpPreview } from "../../test/grokMcpFixtures";
vi.mock(import("../../lib/desktop/grokMcpPreview"), async (original) => ({
    ...(await original()),
    previewGrokMcpRegistration: vi.fn(),
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
beforeEach(() => {
    vi.mocked(previewGrokMcpRegistration)
        .mockReset()
        .mockResolvedValue({
            ...grokMcpPreview,
            proposal: { ...grokMcpPreview.proposal, revision: row.revision },
        });
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

test("Grok preview uses the list revision and closes without central or Pi mutation", async () => {
    const homeRef = createRef<ModuleNavigationHandle>();
    await setup("en", homeRef);
    const trigger = await screen.findByRole("button", {
        name: "Preview “Tools” for Grok Build",
    });
    trigger.focus();
    fireEvent.click(trigger);
    expect(await screen.findByText(grokMcpPreview.target.path)).toBeDefined();
    expect(previewGrokMcpRegistration).toHaveBeenCalledWith(
        row.id,
        row.revision,
    );
    expect(homeRef.current?.openHome()).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
        expect(document.activeElement).toBe(trigger);
    });
    expect(save).not.toHaveBeenCalled();
    expect(deleteMcpDefinition).not.toHaveBeenCalled();
    expect(applyPiMcpChange).not.toHaveBeenCalled();
    expect(previewPiMcpChange).not.toHaveBeenCalled();
});

test("Grok stale revision reloads the list instead of editing or deploying", async () => {
    vi.mocked(previewGrokMcpRegistration).mockRejectedValue(
        new GrokMcpPreviewError("revision_conflict"),
    );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Preview “Tools” for Grok Build",
        }),
    );
    await screen.findByRole("alert");
    const before = list.mock.calls.length;
    fireEvent.click(screen.getByRole("button", { name: "Reload definitions" }));
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
        expect(list.mock.calls.length).toBeGreaterThan(before);
    });
    expect(save).not.toHaveBeenCalled();
});

test("hiding Grok preview invalidates its pending result and never reopens it", async () => {
    let finish: (value: GrokMcpPreview) => void = () => {};
    vi.mocked(previewGrokMcpRegistration).mockReturnValue(
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
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Preview “Tools” for Grok Build",
        }),
    );
    expect(screen.getByRole("dialog")).toBeDefined();
    rendered.rerender(view(true));
    await act(async () => {
        finish(grokMcpPreview);
        await Promise.resolve();
    });
    rendered.rerender(view(false));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByText(grokMcpPreview.target.path)).toBeNull();
    expect(save).not.toHaveBeenCalled();
});

test("hiding MCP closes its preview and ignores a late native result", async () => {
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
    fireEvent.click(
        await screen.findByRole("button", { name: "Manage Pi · Tools" }),
    );
    expect(screen.getByRole("dialog")).toBeDefined();
    rendered.rerender(view(true));
    await act(async () => {
        fail(new PiMcpError("outcome_unknown"));
        await Promise.resolve();
    });
    rendered.rerender(view(false));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(save).not.toHaveBeenCalled();
});

test("Pi preview reads a list revision and reloads without mutating the definition", async () => {
    vi.mocked(previewPiMcpChange).mockRejectedValue(
        new PiMcpError("definition_changed"),
    );
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Manage Pi · Tools" }),
    );
    fireEvent.click(
        await screen.findByRole("button", { name: "Review deployment" }),
    );
    await screen.findByRole("alert");
    expect(previewPiMcpChange).toHaveBeenCalledWith({
        definitionId: row.id,
        expectedRevision: row.revision,
        action: "deploy",
        historyId: null,
    });
    fireEvent.click(screen.getByRole("button", { name: "Reload definitions" }));
    await waitFor(() => {
        expect(list).toHaveBeenCalledTimes(2);
    });
    expect(save).not.toHaveBeenCalled();
    expect(clean).not.toHaveBeenCalled();
    expect(deleteMcpDefinition).not.toHaveBeenCalled();
    expect(
        screen.getByText("Not registered in this Pi directory"),
    ).toBeDefined();
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
    fireEvent.click(
        await screen.findByRole("button", { name: "Manage Pi · Tools" }),
    );
    fireEvent.click(
        await screen.findByRole("button", { name: "Review deployment" }),
    );
    fireEvent.click(
        await screen.findByRole("button", { name: "Confirm file change" }),
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
    expect(
        screen.getByText("Not registered in this Pi directory"),
    ).toBeDefined();
    expect(screen.queryByText("mcp.list.registeredAgents")).toBeNull();
    expect(save).not.toHaveBeenCalled();
    fireEvent.click(
        screen.getByRole("button", { name: "Disable “Tools” definition" }),
    );
    await screen.findByRole("button", {
        name: "Enable “Tools” definition",
    });
    expect(
        screen.getByText("Not registered in this Pi directory"),
    ).toBeDefined();
    expect(clean).not.toHaveBeenCalled();
    expect(get).not.toHaveBeenCalled();
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
