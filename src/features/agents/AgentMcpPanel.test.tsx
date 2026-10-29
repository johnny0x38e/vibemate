import {
    act,
    cleanup,
    fireEvent,
    render,
    screen,
    waitFor,
} from "@testing-library/react";
import { StrictMode } from "react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createAppI18n } from "../../i18n";
import {
    PiMcpError,
    readPiMcpState,
    type PiMcpState,
} from "../../lib/desktop/piMcp";
import { readGrokMcpState } from "../../lib/desktop/grokMcp";
import { emptyPiMcpState, piMcpDeployment } from "../../test/piMcpFixtures";
import { emptyGrokMcpState } from "../../test/grokMcpFixtures";
import { readClaudeMcpState } from "../../lib/desktop/claudeMcp";
import {
    claudeMcpDeployment,
    emptyClaudeMcpState,
} from "../../test/claudeMcpFixtures";
import { AgentMcpPanel } from "./AgentMcpPanel";
import { readCodexMcpState } from "../../lib/desktop/codexMcp";
import {
    codexMcpDeployment,
    emptyCodexMcpState,
} from "../../test/codexFixtures";
vi.mock(import("../../lib/desktop/codexMcp"), async (original) => ({
    ...(await original()),
    readCodexMcpState: vi.fn(),
}));
vi.mock(import("../../lib/desktop/claudeMcp"), async (original) => ({
    ...(await original()),
    readClaudeMcpState: vi.fn(),
}));
vi.mock(import("../../lib/desktop/piMcp"), async (original) => ({
    ...(await original()),
    readPiMcpState: vi.fn(),
}));
vi.mock(import("../../lib/desktop/grokMcp"), async (original) => ({
    ...(await original()),
    readGrokMcpState: vi.fn(),
}));
beforeEach(() => {
    vi.mocked(readPiMcpState)
        .mockReset()
        .mockResolvedValue({
            ...emptyPiMcpState,
            deployments: [piMcpDeployment],
        });
    vi.mocked(readGrokMcpState)
        .mockReset()
        .mockResolvedValue(emptyGrokMcpState);
});
afterEach(cleanup);
beforeEach(() => {
    vi.mocked(readClaudeMcpState)
        .mockReset()
        .mockResolvedValue({
            ...emptyClaudeMcpState,
            deployments: [claudeMcpDeployment],
        });
});

test("only Pi observations are listed, refresh rereads, and management navigates without writes", async () => {
    const i18n = await createAppI18n("en"),
        open = vi.fn();
    render(
        <StrictMode>
            <I18nextProvider i18n={i18n}>
                <AgentMcpPanel agentId="pi" hidden={false} onOpenMcp={open} />
            </I18nextProvider>
        </StrictMode>,
    );
    await screen.findByRole("list", { name: "Managed Pi MCP registrations" });
    expect(screen.getByText(/Tools · Matches the managed entry/)).toBeDefined();
    vi.mocked(readPiMcpState).mockResolvedValue(emptyPiMcpState);
    fireEvent.click(
        screen.getByRole("button", { name: "Refresh registration" }),
    );
    await screen.findByText("No managed Pi MCP entries in this directory.");
    fireEvent.click(
        screen.getByRole("button", { name: "Open MCP management" }),
    );
    expect(open).toHaveBeenCalledTimes(1);
});

test("unsupported Agents and hidden Pi panels do not read native files", async () => {
    const i18n = await createAppI18n("en");
    const view = (agentId: "pi" | "grokBuild", hidden: boolean) => (
        <I18nextProvider i18n={i18n}>
            <AgentMcpPanel
                agentId={agentId}
                hidden={hidden}
                onOpenMcp={() => {}}
            />
        </I18nextProvider>
    );
    const rendered = render(view("grokBuild", false));
    expect(readPiMcpState).not.toHaveBeenCalled();
    rendered.rerender(view("pi", true));
    expect(readPiMcpState).not.toHaveBeenCalled();
    rendered.rerender(view("pi", false));
    await screen.findByRole("list");
    expect(readPiMcpState).toHaveBeenCalledTimes(1);
});

test("all pages are read before any row can imply a complete registration summary", async () => {
    const i18n = await createAppI18n("en");
    const cursor = "a".repeat(32);
    let finish: (value: PiMcpState) => void = () => {};
    vi.mocked(readPiMcpState)
        .mockResolvedValueOnce({
            ...emptyPiMcpState,
            deployments: [piMcpDeployment],
            nextCursor: cursor,
        })
        .mockReturnValueOnce(
            new Promise((done) => {
                finish = done;
            }),
        );
    render(
        <I18nextProvider i18n={i18n}>
            <AgentMcpPanel agentId="pi" hidden={false} onOpenMcp={() => {}} />
        </I18nextProvider>,
    );
    await waitFor(() => {
        expect(readPiMcpState).toHaveBeenCalledWith(null, cursor, 100);
    });
    expect(screen.queryByRole("list")).toBeNull();
    await act(async () => {
        finish({
            ...emptyPiMcpState,
            deployments: [
                {
                    ...piMcpDeployment,
                    definitionId: "b".repeat(32),
                    displayName: "Second tools",
                },
            ],
        });
        await Promise.resolve();
    });
    await screen.findByText(/Second tools/);
    expect(screen.getByText(/Tools · Matches/)).toBeDefined();
});

test("a hidden old page cannot publish or continue pagination", async () => {
    const i18n = await createAppI18n("en");
    let finish: (value: PiMcpState) => void = () => {};
    vi.mocked(readPiMcpState).mockReturnValueOnce(
        new Promise((done) => {
            finish = done;
        }),
    );
    const view = (hidden: boolean) => (
        <I18nextProvider i18n={i18n}>
            <AgentMcpPanel agentId="pi" hidden={hidden} onOpenMcp={() => {}} />
        </I18nextProvider>
    );
    const rendered = render(view(false));
    rendered.rerender(view(true));
    await act(async () => {
        finish({
            ...emptyPiMcpState,
            deployments: [piMcpDeployment],
            nextCursor: "a".repeat(32),
        });
        await Promise.resolve();
    });
    expect(readPiMcpState).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("list")).toBeNull();
});

test("browser errors never turn into fake empty or connected registrations", async () => {
    vi.mocked(readPiMcpState).mockRejectedValue(
        new PiMcpError("desktop_required"),
    );
    const i18n = await createAppI18n("en");
    render(
        <I18nextProvider i18n={i18n}>
            <AgentMcpPanel agentId="pi" hidden={false} onOpenMcp={() => {}} />
        </I18nextProvider>,
    );
    await screen.findByRole("alert");
    expect(screen.queryByRole("list")).toBeNull();
    expect(
        screen.queryByText("No managed Pi MCP entries in this directory."),
    ).toBeNull();
});

test("Claude Code lists user-scope registrations without an enabled flag", async () => {
    const i18n = await createAppI18n("en");
    render(
        <StrictMode>
            <I18nextProvider i18n={i18n}>
                <AgentMcpPanel
                    agentId="claudeCode"
                    hidden={false}
                    onOpenMcp={() => {}}
                />
            </I18nextProvider>
        </StrictMode>,
    );
    const list = await screen.findByRole("list", {
        name: "Managed Claude Code MCP registrations",
    });
    expect(list.textContent).toBe("Tools · Matches the managed entry");
    expect(readPiMcpState).not.toHaveBeenCalled();
    expect(readGrokMcpState).not.toHaveBeenCalled();
    expect(
        screen.getByText(/new Claude session or reconnect with \/mcp/),
    ).toBeDefined();
    vi.mocked(readClaudeMcpState).mockResolvedValue(emptyClaudeMcpState);
    fireEvent.click(
        screen.getByRole("button", { name: "Refresh registration" }),
    );
    await screen.findByText(
        "No managed Claude Code MCP entries in this user scope.",
    );
});

test.each([
    ["en", "Manually disabled in Codex", "false"],
    ["zh-CN", "已在 Codex 中手动禁用", null],
] as const)(
    "a Codex entry turned off by hand reads as manually disabled (%s)",
    async (locale, label, enabled) => {
        vi.mocked(readCodexMcpState).mockResolvedValue({
            ...emptyCodexMcpState,
            deployments: [
                {
                    ...codexMcpDeployment,
                    state: "modified",
                    nativeEnabled: false,
                    unmanagedKeys: ["enabled_tools"],
                },
            ],
        });
        const i18n = await createAppI18n(locale);
        render(
            <I18nextProvider i18n={i18n}>
                <AgentMcpPanel
                    agentId="codex"
                    hidden={false}
                    onOpenMcp={() => {}}
                />
            </I18nextProvider>,
        );
        const row = await screen.findByText(new RegExp(label, "u"));
        expect(row.textContent).toContain("Remote tools");
        if (enabled !== null) expect(row.textContent).toContain(enabled);
    },
);
