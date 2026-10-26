import {
    act,
    cleanup,
    fireEvent,
    render,
    screen,
    waitFor,
    within,
} from "@testing-library/react";
import { StrictMode, useState, type JSX } from "react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createAppI18n } from "../../i18n";
import {
    GrokMcpPreviewError,
    previewGrokMcpRegistration,
    type GrokMcpPreview,
} from "../../lib/desktop/grokMcpPreview";
import { McpGrokPreview } from "./McpGrokPreview";
vi.mock(import("../../lib/desktop/grokMcpPreview"), async (original) => ({
    ...(await original()),
    previewGrokMcpRegistration: vi.fn(),
}));
import {
    grokMcpRecord as record,
    grokMcpPreview as preview,
} from "../../test/grokMcpFixtures";
const read = vi.mocked(previewGrokMcpRegistration);
beforeEach(() => {
    read.mockReset().mockResolvedValue(preview);
});
afterEach(cleanup);
async function setup() {
    const i18n = await createAppI18n("en");
    function Harness(): JSX.Element {
        const [trigger, setTrigger] = useState<HTMLElement | null>(null);
        return (
            <>
                <button
                    type="button"
                    onClick={(event) => {
                        setTrigger(event.currentTarget);
                    }}
                >
                    Review
                </button>
                {trigger !== null && (
                    <McpGrokPreview
                        record={record}
                        returnFocus={trigger}
                        onClose={() => {
                            setTrigger(null);
                        }}
                        onReload={() => {
                            setTrigger(null);
                        }}
                    />
                )}
            </>
        );
    }
    render(
        <StrictMode>
            <I18nextProvider i18n={i18n}>
                <Harness />
            </I18nextProvider>
        </StrictMode>,
    );
    const trigger = screen.getByRole("button", { name: "Review" });
    trigger.focus();
    fireEvent.click(trigger);
    return i18n;
}

test("reviews the fixed target, opens only masked entry details, and restores focus", async () => {
    await setup();
    expect(await screen.findByText(preview.target.path)).toBeDefined();
    expect(read).toHaveBeenCalledWith(record.id, record.revision);
    fireEvent.click(screen.getByRole("button", { name: "View entry details" }));
    const details = await screen.findByRole("dialog", {
        name: "Grok Build MCP entry details",
    });
    expect(within(details).getByText(/"TOKEN": "••••••••"/)).toBeDefined();
    expect(within(details).getByText(/"mcp_servers":/)).toBeDefined();
    expect(
        screen.queryByRole("button", { name: /apply|deploy|confirm/i }),
    ).toBeNull();
    fireEvent.click(within(details).getByRole("button", { name: "Close" }));
    await waitFor(() => {
        expect(
            screen.queryByRole("dialog", {
                name: "Grok Build MCP entry details",
            }),
        ).toBeNull();
    });
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
        expect(document.activeElement).toBe(
            screen.getByRole("button", { name: "Review" }),
        );
    });
});

test("failed preflight exposes retry but never details until a fresh read succeeds", async () => {
    read.mockRejectedValue(new GrokMcpPreviewError("name_conflict"));
    await setup();
    expect(await screen.findByRole("alert")).toBeDefined();
    expect(
        screen.queryByRole("button", { name: "View entry details" }),
    ).toBeNull();
    read.mockResolvedValue(preview);
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText(preview.target.path)).toBeDefined();
    expect(screen.queryByRole("alert")).toBeNull();
});

test("closing a pending preview ignores a late completion and allows another review", async () => {
    let finish: (value: GrokMcpPreview) => void = () => {};
    read.mockReturnValue(
        new Promise((resolve) => {
            finish = resolve;
        }),
    );
    await setup();
    expect(screen.getByRole("status")).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await act(async () => {
        finish(preview);
        await Promise.resolve();
    });
    expect(screen.queryByText(preview.target.path)).toBeNull();
    read.mockResolvedValue(preview);
    fireEvent.click(screen.getByRole("button", { name: "Review" }));
    expect(await screen.findByText(preview.target.path)).toBeDefined();
});

test("language changes preserve the current proposal and nested detail state without another read", async () => {
    const i18n = await setup();
    await screen.findByText(preview.target.path);
    fireEvent.click(screen.getByRole("button", { name: "View entry details" }));
    await screen.findByRole("dialog", { name: "Grok Build MCP entry details" });
    const calls = read.mock.calls.length;
    await act(async () => {
        await i18n.changeLanguage("zh-CN");
    });
    const details = screen.getByRole("dialog", {
        name: "Grok Build MCP 条目详情",
    });
    expect(within(details).getByText(/"TOKEN": "••••••••"/)).toBeDefined();
    expect(read.mock.calls.length).toBe(calls);
    fireEvent.click(within(details).getByRole("button", { name: "关闭" }));
    expect(await screen.findByText(preview.target.path)).toBeDefined();
});

test("reload definitions closes a stale-revision preview", async () => {
    read.mockRejectedValue(new GrokMcpPreviewError("revision_conflict"));
    await setup();
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "Reload definitions" }));
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
    });
});
