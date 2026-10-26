import {
    cleanup,
    fireEvent,
    render,
    screen,
    waitFor,
} from "@testing-library/react";
import { StrictMode, type ReactNode } from "react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createAppI18n } from "../../i18n";
import { discoverAgents } from "../../lib/desktop/agents";
import { countProviderModels } from "../../lib/desktop/models";
import { listMcpDefinitions } from "../../lib/desktop/mcp";
import { listProviders } from "../../lib/desktop/providers";
import { listRecoverySummary } from "../../lib/desktop/recoverySummary";
import { listSkills } from "../../lib/desktop/skills";
import { agentReport, automaticAgentPaths } from "../../test/agentDiscovery";
import { OverviewView, type OverviewDestination } from "./OverviewView";

vi.mock(import("../../lib/desktop/providers"), async (original) => ({
    ...(await original()),
    listProviders: vi.fn(),
}));

vi.mock(import("../../lib/desktop/models"), async (original) => ({
    ...(await original()),
    countProviderModels: vi.fn(),
}));

vi.mock(import("../../lib/desktop/agents"), async (original) => ({
    ...(await original()),
    discoverAgents: vi.fn(),
}));

vi.mock(import("../../lib/desktop/skills"), async (original) => ({
    ...(await original()),
    listSkills: vi.fn(),
}));

vi.mock(import("../../lib/desktop/mcp"), async (original) => ({
    ...(await original()),
    listMcpDefinitions: vi.fn(),
}));

vi.mock(import("../../lib/desktop/recoverySummary"), async (original) => ({
    ...(await original()),
    listRecoverySummary: vi.fn(),
}));

afterEach(() => {
    cleanup();
});

async function mount(
    onNavigate: (destination: OverviewDestination) => void = vi.fn(),
): Promise<void> {
    const i18n = await createAppI18n("en");
    function Tree({ children }: { children: ReactNode }) {
        return (
            <StrictMode>
                <I18nextProvider i18n={i18n}>{children}</I18nextProvider>
            </StrictMode>
        );
    }
    render(<OverviewView onNavigate={onNavigate} />, { wrapper: Tree });
}

beforeEach(() => {
    vi.mocked(listProviders)
        .mockReset()
        .mockResolvedValue({
            kind: "desktop",
            page: { items: [], nextCursor: null, total: 3 },
        });
    vi.mocked(countProviderModels)
        .mockReset()
        .mockResolvedValue({
            kind: "desktop",
            count: { total: 7 },
        });
    vi.mocked(discoverAgents)
        .mockReset()
        .mockResolvedValue({
            kind: "desktop",
            items: agentReport([
                {
                    id: "claudeCode",
                    state: "found",
                    source: "path",
                    entryPath: "/bin/claude",
                    resolvedPath: "/bin/claude",
                    version: null,
                    versionEvidence: "none",
                    capabilityEvidence: "unverified",
                    configDirectory: null,
                    directorySource: "unavailable",
                    files: [],
                },
                {
                    id: "pi",
                    state: "found",
                    source: "path",
                    entryPath: "/bin/pi",
                    resolvedPath: "/bin/pi",
                    version: null,
                    versionEvidence: "none",
                    capabilityEvidence: "unverified",
                    configDirectory: null,
                    directorySource: "unavailable",
                    files: [],
                },
            ]),
        });
    vi.mocked(listSkills)
        .mockReset()
        .mockResolvedValue({
            kind: "desktop",
            page: { items: [], nextCursor: null, total: 0 },
        });
    vi.mocked(listMcpDefinitions)
        .mockReset()
        .mockResolvedValue({
            kind: "desktop",
            page: {
                items: [],
                nextCursor: null,
                total: 5,
                deletedCleanupPending: false,
            },
        });
    vi.mocked(listRecoverySummary)
        .mockReset()
        .mockResolvedValue({ kind: "desktop", items: [] });
});

test("shows five totals including zero selected models, without inventing preview numbers", async () => {
    await mount();
    await screen.findByRole("button", { name: "Providers: 3" });
    expect(
        screen.getByRole("button", { name: "Added models: 7" }),
    ).toBeDefined();
    expect(
        screen.getByRole("button", { name: "Discovered agents: 2" }),
    ).toBeDefined();
    expect(screen.getByRole("button", { name: "Skills: 0" })).toBeDefined();
    expect(
        screen.getByRole("button", { name: "MCP servers: 5" }),
    ).toBeDefined();
    // Skills/MCP copy is the central-library name only — no "injected" claim.
    expect(document.body.textContent).not.toMatch(/inject|deployed into/i);
    expect(discoverAgents).toHaveBeenCalledWith(automaticAgentPaths);
    expect(countProviderModels).toHaveBeenCalled();
    expect(listProviders).toHaveBeenCalledWith({ after: null, limit: 1 });
    expect(listSkills).toHaveBeenCalledWith(null, 1);
    expect(listMcpDefinitions).toHaveBeenCalledWith(null, 1);
});

test("clicking a card navigates to the matching module; models open Providers", async () => {
    const onNavigate = vi.fn<(destination: OverviewDestination) => void>();
    await mount(onNavigate);
    await screen.findByRole("button", { name: "Providers: 3" });
    fireEvent.click(screen.getByRole("button", { name: "Providers: 3" }));
    fireEvent.click(screen.getByRole("button", { name: "Added models: 7" }));
    fireEvent.click(
        screen.getByRole("button", { name: "Discovered agents: 2" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Skills: 0" }));
    fireEvent.click(screen.getByRole("button", { name: "MCP servers: 5" }));
    expect(onNavigate.mock.calls).toEqual([
        ["providers"],
        ["providers"],
        ["agents"],
        ["skills"],
        ["mcp"],
    ]);
});

test("a failed read shows unavailable with retry, without a fake number", async () => {
    vi.mocked(listProviders).mockRejectedValueOnce(new Error("boom"));
    await mount();
    expect(
        await screen.findByRole("button", { name: "Providers: Unavailable" }),
    ).toBeDefined();
    expect(
        screen.getByRole("button", { name: "Retry Providers" }),
    ).toBeDefined();
    vi.mocked(listProviders).mockResolvedValue({
        kind: "desktop",
        page: { items: [], nextCursor: null, total: 4 },
    });
    fireEvent.click(screen.getByRole("button", { name: "Retry Providers" }));
    await waitFor(() => {
        expect(
            screen.getByRole("button", { name: "Providers: 4" }),
        ).toBeDefined();
    });
});

test("browser preview stays honest and never invents totals", async () => {
    vi.mocked(listProviders).mockResolvedValue({ kind: "preview" });
    vi.mocked(countProviderModels).mockResolvedValue({ kind: "preview" });
    vi.mocked(discoverAgents).mockResolvedValue({ kind: "preview" });
    vi.mocked(listSkills).mockResolvedValue({ kind: "preview" });
    vi.mocked(listMcpDefinitions).mockResolvedValue({ kind: "preview" });
    vi.mocked(listRecoverySummary).mockResolvedValue({ kind: "preview" });
    await mount();
    await screen.findByRole("button", {
        name: "Providers: Open the desktop app to read this count.",
    });
    expect(
        screen.getAllByRole("button", {
            name: /Open the desktop app to read this count/,
        }),
    ).toHaveLength(5);
    expect(screen.queryByRole("button", { name: /Providers: \d+/ })).toBeNull();
    expect(
        screen.queryByRole("button", { name: /Added models: \d+/ }),
    ).toBeNull();
});
