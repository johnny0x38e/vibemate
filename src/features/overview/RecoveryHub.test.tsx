import {
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
import { listRecoverySummary } from "../../lib/desktop/recoverySummary";
import { RecoveryHub } from "./RecoveryHub";

vi.mock(
    import("../../lib/desktop/recoverySummary"),
    async (importOriginal) => ({
        ...(await importOriginal()),
        listRecoverySummary: vi.fn(),
    }),
);

const list = vi.mocked(listRecoverySummary);
const navigate = vi.fn();

async function renderHub(): Promise<void> {
    const i18n = await createAppI18n("en");
    render(
        <StrictMode>
            <I18nextProvider i18n={i18n}>
                <RecoveryHub onNavigate={navigate} />
            </I18nextProvider>
        </StrictMode>,
    );
}

beforeEach(() => {
    list.mockReset();
    navigate.mockReset();
});

afterEach(() => {
    cleanup();
});

test("stays hidden when nothing is unfinished", async () => {
    list.mockResolvedValue({ kind: "desktop", items: [] });
    await renderHub();
    await waitFor(() => {
        expect(list).toHaveBeenCalled();
    });
    expect(
        screen.queryByRole("region", { name: "Unfinished operations" }),
    ).toBeNull();
});

test("lists unfinished rows and opens the owning module", async () => {
    list.mockResolvedValue({
        kind: "desktop",
        items: [
            {
                id: "skill:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                domain: "skillDeploy",
                destination: "skills",
                agentId: "pi",
                subject: "demo-skill",
                detail: "apply:needs_record",
            },
            {
                id: "mcp:pi:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                domain: "mcp",
                destination: "mcp",
                agentId: "pi",
                subject: "tools",
                detail: "deploy:intended",
            },
        ],
    });
    await renderHub();
    expect(
        await screen.findByRole("region", { name: "Unfinished operations" }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Open Skills" }));
    expect(navigate).toHaveBeenCalledWith("skills");
    fireEvent.click(screen.getByRole("button", { name: "Open MCP" }));
    expect(navigate).toHaveBeenCalledWith("mcp");
});

test("shows a local error with retry", async () => {
    // Persistent reject: StrictMode remounts would consume a Once mock.
    list.mockRejectedValue(new Error("boom"));
    await renderHub();
    expect(await screen.findByRole("alert")).toBeTruthy();
    list.mockResolvedValue({ kind: "desktop", items: [] });
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => {
        expect(
            screen.queryByRole("region", { name: "Unfinished operations" }),
        ).toBeNull();
    });
});
