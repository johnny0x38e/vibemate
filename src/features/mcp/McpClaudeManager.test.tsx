import {
    cleanup,
    fireEvent,
    render,
    screen,
    within,
} from "@testing-library/react";
import { StrictMode, useState, type JSX } from "react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createAppI18n } from "../../i18n";
import {
    applyClaudeMcpChange,
    CLAUDE_MCP_TOGGLE_SUPPORTED,
    ClaudeMcpError,
    previewClaudeMcpChange,
    readClaudeMcpHistory,
    readClaudeMcpState,
    recoverClaudeMcpChange,
} from "../../lib/desktop/claudeMcp";
import {
    claudeMcpDefinition,
    claudeMcpDeployment,
    claudeMcpHistory,
    claudeMcpPlan,
    claudeMcpRecovery,
    emptyClaudeMcpState,
} from "../../test/claudeMcpFixtures";
import {
    McpClaudeManager,
    type McpClaudeManagerIntent,
} from "./McpClaudeManager";
vi.mock(import("../../lib/desktop/claudeMcp"), async (original) => ({
    ...(await original()),
    readClaudeMcpState: vi.fn(),
    readClaudeMcpHistory: vi.fn(),
    previewClaudeMcpChange: vi.fn(),
    applyClaudeMcpChange: vi.fn(),
    recoverClaudeMcpChange: vi.fn(),
}));
const busy = vi.fn(),
    changed = vi.fn(),
    reload = vi.fn();
const registeredState = {
    ...emptyClaudeMcpState,
    deployments: [claudeMcpDeployment],
};
beforeEach(() => {
    busy.mockReset();
    changed.mockReset();
    reload.mockReset();
    vi.mocked(readClaudeMcpState)
        .mockReset()
        .mockResolvedValue(emptyClaudeMcpState);
    vi.mocked(readClaudeMcpHistory)
        .mockReset()
        .mockResolvedValue({ items: [], nextCursor: null });
    vi.mocked(previewClaudeMcpChange)
        .mockReset()
        .mockImplementation((selection) =>
            Promise.resolve({
                ...claudeMcpPlan,
                action:
                    selection.action === "revoke" ||
                    selection.action === "restore"
                        ? selection.action
                        : "deploy",
                beforeEntry:
                    selection.action === "deploy"
                        ? null
                        : claudeMcpPlan.afterEntry,
                afterEntry:
                    selection.action === "revoke"
                        ? null
                        : claudeMcpPlan.afterEntry,
            }),
        );
    vi.mocked(applyClaudeMcpChange)
        .mockReset()
        .mockResolvedValue({
            changeId: "b".repeat(32),
            registered: true,
            reloadRequired: true,
        });
    vi.mocked(recoverClaudeMcpChange).mockReset().mockResolvedValue({
        changeId: claudeMcpRecovery.id,
        registered: true,
        reloadRequired: false,
    });
});
afterEach(cleanup);
async function setup(
    locale: "en" | "zh-CN" = "en",
    intent: McpClaudeManagerIntent = "manage",
) {
    const i18n = await createAppI18n(locale);
    function Harness(): JSX.Element {
        const [open, setOpen] = useState(true);
        return open ? (
            <McpClaudeManager
                record={claudeMcpDefinition}
                intent={intent}
                onClose={() => {
                    setOpen(false);
                }}
                onReload={reload}
                onBusyChange={busy}
                onChanged={changed}
            />
        ) : (
            <p>closed</p>
        );
    }
    return render(
        <StrictMode>
            <I18nextProvider i18n={i18n}>
                <Harness />
            </I18nextProvider>
        </StrictMode>,
    );
}

test("the adapter reports no toggle capability and the manager renders none", async () => {
    expect(CLAUDE_MCP_TOGGLE_SUPPORTED).toBe(false);
    vi.mocked(readClaudeMcpState).mockResolvedValue(registeredState);
    await setup();
    const dialog = await screen.findByRole("dialog", {
        name: "Claude Code registration · Tools",
    });
    await within(dialog).findByRole("button", { name: "Review update" });
    expect(
        within(dialog).getByRole("button", { name: "Revoke from Claude Code" }),
    ).toBeDefined();
    expect(
        within(dialog).queryByRole("button", { name: /enable|disable/i }),
    ).toBeNull();
    expect(
        within(dialog).getByText(/no user-level enable\/disable/),
    ).toBeDefined();
    expect(
        within(dialog).getByText(/same name in a project's Local scope/),
    ).toBeDefined();
    expect(
        within(dialog).getByText(emptyClaudeMcpState.targetPath),
    ).toBeDefined();
    expect(within(dialog).getByText("Matches the managed entry")).toBeDefined();
});

test("deploy confirms the reviewed approval and then shows the new-session or /mcp hint", async () => {
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Review deployment" }),
    );
    await screen.findByRole("region", { name: "Reviewed entry change" });
    expect(applyClaudeMcpChange).not.toHaveBeenCalled();
    fireEvent.click(
        screen.getByRole("button", { name: "Confirm file change" }),
    );
    await screen.findByText(
        /Start a new Claude session or reconnect with \/mcp/,
    );
    expect(applyClaudeMcpChange).toHaveBeenCalledWith(
        {
            definitionId: claudeMcpDefinition.id,
            expectedRevision: 1,
            action: "deploy",
            historyId: null,
        },
        expect.objectContaining({ approval: claudeMcpPlan.approval }),
    );
    expect(busy.mock.calls).toEqual([[true], [false]]);
    expect(changed).toHaveBeenCalledTimes(1);
});

test("the success hint is localized for zh-CN", async () => {
    await setup("zh-CN");
    fireEvent.click(await screen.findByRole("button", { name: "审阅部署" }));
    fireEvent.click(
        await screen.findByRole("button", { name: "确认修改文件" }),
    );
    await screen.findByText(/新开 Claude 会话或用 \/mcp 重连/);
});

test("revoke uses the same review/confirm boundary", async () => {
    vi.mocked(readClaudeMcpState).mockResolvedValue(registeredState);
    vi.mocked(applyClaudeMcpChange).mockResolvedValue({
        changeId: "b".repeat(32),
        registered: false,
        reloadRequired: true,
    });
    await setup();
    fireEvent.click(
        await screen.findByRole("button", { name: "Revoke from Claude Code" }),
    );
    const review = await screen.findByRole("region", {
        name: "Reviewed entry change",
    });
    expect(within(review).getByText("Entry removed")).toBeDefined();
    expect(previewClaudeMcpChange).toHaveBeenLastCalledWith(
        expect.objectContaining({ action: "revoke" }),
    );
    fireEvent.click(
        screen.getByRole("button", { name: "Confirm file change" }),
    );
    await screen.findByText(/reconnect with \/mcp/);
});

test("history rows show the retained registered result and restore via review", async () => {
    vi.mocked(readClaudeMcpHistory).mockResolvedValue({
        items: [
            {
                id: "c".repeat(32),
                action: "revoke",
                createdAt: 3,
                serverName: "tools",
                registered: false,
            },
            ...claudeMcpHistory.items,
        ],
        nextCursor: null,
    });
    await setup("en", "history");
    const dialog = await screen.findByRole("dialog", {
        name: "Claude Code registration history · Tools",
    });
    const history = await within(dialog).findByRole("region", {
        name: "Entry history",
    });
    const rows = within(history).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(
        within(rows[0] as HTMLElement).getByText(
            "Not registered after this change",
        ),
    ).toBeDefined();
    expect(
        within(rows[1] as HTMLElement).getByText(
            "Registered after this change",
        ),
    ).toBeDefined();
    fireEvent.click(
        within(rows[1] as HTMLElement).getByRole("button", {
            name: `Review entry result ${"b".repeat(8)}`,
        }),
    );
    await screen.findByRole("region", { name: "Reviewed entry change" });
    expect(previewClaudeMcpChange).toHaveBeenLastCalledWith({
        definitionId: claudeMcpDefinition.id,
        expectedRevision: 1,
        action: "restore",
        historyId: claudeMcpHistory.items[0]?.id,
    });
});

test.each([
    ["unsupported_field", /cwd would be silently dropped/],
    ["toggle_unsupported", /no user-level enable\/disable for a single server/],
] as const)("%s has dedicated English copy", async (code, copy) => {
    vi.mocked(previewClaudeMcpChange).mockRejectedValue(
        new ClaudeMcpError(code),
    );
    await setup("en", "register");
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toMatch(copy);
    expect(
        screen.queryByRole("button", { name: "Confirm file change" }),
    ).toBeNull();
    expect(applyClaudeMcpChange).not.toHaveBeenCalled();
});

test.each([
    ["unsupported_field", /cwd 会被静默丢弃/],
    ["toggle_unsupported", /没有针对单个服务器的用户级启用\/停用/],
] as const)("%s has dedicated zh-CN copy", async (code, copy) => {
    vi.mocked(previewClaudeMcpChange).mockRejectedValue(
        new ClaudeMcpError(code),
    );
    await setup("zh-CN", "register");
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toMatch(copy);
});

test("an interrupted change is reconciled without replaying the write", async () => {
    vi.mocked(readClaudeMcpState).mockResolvedValue({
        ...emptyClaudeMcpState,
        recovery: claudeMcpRecovery,
    });
    await setup();
    const recovery = await screen.findByRole("region", {
        name: "Interrupted Claude Code change",
    });
    expect(
        screen.getByRole("button", { name: "Review deployment" }),
    ).toHaveProperty("disabled", true);
    fireEvent.click(
        within(recovery).getByRole("button", { name: "Review interruption" }),
    );
    fireEvent.click(
        await screen.findByRole("button", { name: "Confirm observed state" }),
    );
    await screen.findByText(/Observation recorded/);
    expect(recoverClaudeMcpChange).toHaveBeenCalledWith(claudeMcpRecovery);
    expect(applyClaudeMcpChange).not.toHaveBeenCalled();
});
