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
    CodexSettingsError,
    applyCodexSettingsChange,
    previewCodexSettingsChange,
    readCodexSettingsHistory,
    readCodexSettingsState,
    recoverCodexSettingsChange,
    type CodexSettingsErrorCode,
    type CodexSettingsState,
} from "../../lib/desktop/codex";
import {
    listProviderModels,
    type ProviderModel,
} from "../../lib/desktop/models";
import {
    listProviders,
    type ProviderRecord,
} from "../../lib/desktop/providers";
import {
    codexDeployPlan,
    codexProviderId,
    codexSetDefaultPlan,
    codexSettingsHistory,
    codexSettingsRecovery,
    codexSettingsState,
} from "../../test/codexFixtures";
import { CodexSettingsPanel } from "./CodexSettingsPanel";
import { CodexWriteDialog } from "./CodexWriteDialog";

vi.mock(import("../../lib/desktop/codex"), async (original) => ({
    ...(await original()),
    readCodexSettingsState: vi.fn(),
    readCodexSettingsHistory: vi.fn(),
    previewCodexSettingsChange: vi.fn(),
    applyCodexSettingsChange: vi.fn(),
    recoverCodexSettingsChange: vi.fn(),
}));
vi.mock(import("../../lib/desktop/providers"), async (original) => ({
    ...(await original()),
    listProviders: vi.fn(),
}));
vi.mock(import("../../lib/desktop/models"), async (original) => ({
    ...(await original()),
    listProviderModels: vi.fn(),
}));
const readState = vi.mocked(readCodexSettingsState);
const readHistory = vi.mocked(readCodexSettingsHistory);
const preview = vi.mocked(previewCodexSettingsChange);
const apply = vi.mocked(applyCodexSettingsChange);
const recover = vi.mocked(recoverCodexSettingsChange);

const provider: ProviderRecord = {
    id: codexProviderId,
    kind: "deepseek",
    displayName: "Deep Seek",
    baseUrl: "https://api.deepseek.com",
    protocol: "responses",
    extensions: {},
    revision: 1,
    createdAtMs: 1,
    updatedAtMs: 2,
    selectedModelCount: 1,
};

function model(modelId: string): ProviderModel {
    const evidence = { state: "unknown" as const, source: null };
    return {
        providerId: codexProviderId,
        modelId,
        source: "fetched",
        selected: true,
        alias: null,
        upstreamName: null,
        contextWindow: null,
        maxOutputTokens: null,
        inputModalities: null,
        outputModalities: null,
        supportedEndpoints: null,
        capabilities: {
            thinking: evidence,
            imageInput: evidence,
            toolCalls: evidence,
        },
        capabilityOverrides: {
            thinking: null,
            imageInput: null,
            toolCalls: null,
        },
        thinkingEffort: null,
        routeSupport: "supported",
        resolvedProtocol: "responses",
        upstreamState: "listed",
        lastSeenAtMs: null,
        missingSinceMs: null,
        createdAtMs: 1,
        updatedAtMs: 2,
    };
}

const ownedDefault = codexSettingsState.default;
if (ownedDefault === null) throw new Error("fixture default required");

/** Nothing deployed yet and no profile: the first write. */
const emptyState: CodexSettingsState = {
    ...codexSettingsState,
    providers: [],
    default: null,
    profileActive: false,
};

async function mountDialog() {
    const i18n = await createAppI18n("en");
    const onApplied = vi.fn();
    render(
        <I18nextProvider i18n={i18n}>
            <CodexWriteDialog
                providerLabel={null}
                modelLabel={null}
                blocked={false}
                onBusyChange={vi.fn()}
                onApplied={onApplied}
                onClose={vi.fn()}
                returnFocus={() => null}
            />
        </I18nextProvider>,
    );
    return { onApplied };
}

async function mountManage(locale: "en" | "zh-CN" = "en") {
    const i18n = await createAppI18n(locale);
    render(
        <I18nextProvider i18n={i18n}>
            <CodexSettingsPanel mode="manage" blocked={false} />
        </I18nextProvider>,
    );
}

beforeEach(() => {
    readState.mockReset().mockResolvedValue(emptyState);
    readHistory.mockReset().mockResolvedValue({ items: [], nextCursor: null });
    preview
        .mockReset()
        .mockImplementation((selection) =>
            Promise.resolve(
                selection.action === "deploy"
                    ? codexDeployPlan
                    : codexSetDefaultPlan,
            ),
        );
    apply.mockReset().mockResolvedValue({
        changeId: "3".repeat(32),
        restartRequired: true,
    });
    recover.mockReset().mockResolvedValue({
        changeId: codexSettingsRecovery.id,
        restartRequired: false,
    });
    vi.mocked(listProviders)
        .mockReset()
        .mockResolvedValue({
            kind: "desktop",
            page: { items: [provider], nextCursor: null, total: 1 },
        });
    vi.mocked(listProviderModels)
        .mockReset()
        .mockResolvedValue({
            kind: "desktop",
            page: {
                items: [model("deepseek-flash"), model("deepseek-v4-pro")],
                nextCursor: null,
                totalMatches: null,
            },
        });
});
afterEach(() => {
    cleanup();
});

async function review(): Promise<HTMLElement> {
    const button = await screen.findByRole("button", {
        name: "Review changes",
    });
    await waitFor(() => {
        expect(button.getAttribute("aria-disabled")).not.toBe("true");
    });
    fireEvent.click(button);
    return screen.findByRole("region", { name: "Review Codex change" });
}

test("deploy shows the plaintext warning, then the set-default step", async () => {
    const { onApplied } = await mountDialog();
    const setDefault = await screen.findByRole("checkbox", {
        name: "Set as Codex default",
    });
    expect(setDefault.getAttribute("aria-checked")).toBe("true");
    const card = await review();
    expect(within(card).getByText("Step 1 of 2")).toBeDefined();
    expect(within(card).getByRole("note").textContent).toMatch(
        /stored there in plain text/u,
    );
    expect(within(card).getByText("/test/codex/config.toml")).toBeDefined();
    expect(preview).toHaveBeenCalledWith({
        providerId: codexProviderId,
        expectedRevision: 1,
        action: "deploy",
        modelId: null,
    });
    fireEvent.click(
        within(card).getByRole("button", { name: "Confirm and write" }),
    );
    await waitFor(() => {
        expect(apply).toHaveBeenCalledWith(
            expect.objectContaining({ action: "deploy" }),
            codexDeployPlan,
        );
    });
    const next = await screen.findByText("Step 2 of 2");
    const second = next.closest("section");
    expect(second).not.toBeNull();
    expect(preview).toHaveBeenLastCalledWith({
        providerId: codexProviderId,
        expectedRevision: 1,
        action: "set_default",
        modelId: "deepseek-flash",
    });
    expect(
        within(second as HTMLElement).getByText(
            "An active Codex profile overrides this default.",
        ),
    ).toBeDefined();
    fireEvent.click(
        within(second as HTMLElement).getByRole("button", {
            name: "Confirm and write",
        }),
    );
    expect(
        await screen.findByText(
            "Written. Start a new Codex session to use it.",
        ),
    ).toBeDefined();
    expect(apply).toHaveBeenCalledTimes(2);
    expect(onApplied).toHaveBeenCalledTimes(2);
});

test("unchecking the default choice only deploys the provider", async () => {
    await mountDialog();
    fireEvent.click(
        await screen.findByRole("checkbox", { name: "Set as Codex default" }),
    );
    const card = await review();
    expect(within(card).queryByText(/Step 1/u)).toBeNull();
    fireEvent.click(
        within(card).getByRole("button", { name: "Confirm and write" }),
    );
    await screen.findByText("Written. Start a new Codex session to use it.");
    expect(preview).toHaveBeenCalledTimes(1);
});

test("an active profile is called out before any write", async () => {
    readState.mockResolvedValue(codexSettingsState);
    await mountDialog();
    expect(
        await screen.findByText(
            "A Codex profile is active in config.toml. Its values override the default model and provider written here.",
        ),
    ).toBeDefined();
    // Already deployed and already the default: nothing to review.
    expect(
        await screen.findByText(
            "This provider is already in config.toml and nothing else would change.",
        ),
    ).toBeDefined();
});

test("plan_stale re-previews the same selection instead of failing", async () => {
    apply.mockRejectedValueOnce(new CodexSettingsError("plan_stale"));
    await mountDialog();
    fireEvent.click(
        await screen.findByRole("checkbox", { name: "Set as Codex default" }),
    );
    const card = await review();
    fireEvent.click(
        within(card).getByRole("button", { name: "Confirm and write" }),
    );
    expect(
        await screen.findByText(
            "config.toml or the provider changed after the review. This is a fresh review; check it again before confirming.",
        ),
    ).toBeDefined();
    expect(preview).toHaveBeenCalledTimes(2);
    expect(preview.mock.calls[1]?.[0]).toEqual(preview.mock.calls[0]?.[0]);
    expect(screen.queryByRole("alert")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Confirm and write" }));
    await screen.findByText("Written. Start a new Codex session to use it.");
});

test("an interrupted change offers a recovery review and blocks new writes", async () => {
    readState.mockResolvedValue({
        ...emptyState,
        recovery: codexSettingsRecovery,
    });
    await mountDialog();
    expect(
        await screen.findByText(
            '"Add provider" for vibemate-deep-seek was interrupted before it wrote config.toml.',
        ),
    ).toBeDefined();
    expect(
        (
            await screen.findByRole("button", { name: "Review changes" })
        ).getAttribute("aria-disabled"),
    ).toBe("true");
    fireEvent.click(
        screen.getByRole("button", { name: "Review interrupted change" }),
    );
    const card = await screen.findByRole("region", {
        name: "Confirm interrupted change",
    });
    fireEvent.click(within(card).getByRole("button", { name: "Confirm" }));
    await waitFor(() => {
        expect(recover).toHaveBeenCalledWith(codexSettingsRecovery);
    });
    expect(
        await screen.findByText("The interrupted change was confirmed."),
    ).toBeDefined();
});

test.each<[CodexSettingsErrorCode, string]>([
    [
        "provider_changed",
        "The provider changed since it was loaded. Reload and review again.",
    ],
    [
        "model_unsupported",
        "This model isn't available for Codex on this provider.",
    ],
    [
        "default_in_use",
        "This provider is the Codex default. Clear the default before removing it.",
    ],
    [
        "not_deployed",
        "Add this provider to Codex before setting it as the default.",
    ],
])("the %s error has its own copy", async (code, text) => {
    preview.mockRejectedValue(new CodexSettingsError(code));
    await mountDialog();
    await review().catch(() => null);
    expect((await screen.findByRole("alert")).textContent).toBe(text);
    expect(apply).not.toHaveBeenCalled();
});

test("manage lists owned providers, user-owned state and history", async () => {
    const owned = codexSettingsState.providers[0];
    if (owned === undefined) throw new Error("missing fixture provider");
    readState.mockResolvedValue({
        ...codexSettingsState,
        providers: [
            ...codexSettingsState.providers,
            {
                ...owned,
                providerId: "9".repeat(32),
                displayName: "Edited",
                owner: {
                    ...owned.owner,
                    providerKey: "vibemate-edited",
                },
                state: "modified",
            },
        ],
    });
    readHistory.mockResolvedValue(codexSettingsHistory);
    await mountManage();
    const list = await screen.findByRole("list", {
        name: "Providers vibemate added to Codex",
    });
    expect(
        within(list).getByText(
            "vibemate-deep-seek · Written by vibemate · Codex default",
        ),
    ).toBeDefined();
    expect(
        within(list).getByText(
            "vibemate-edited · Own configuration (changed or removed outside vibemate)",
        ),
    ).toBeDefined();
    expect(screen.getByText("Deep Seek · deepseek-flash")).toBeDefined();
    const history = screen.getByRole("list", { name: "Codex change history" });
    expect(
        within(history).getByText(
            "Set as Codex default · vibemate-deep-seek · deepseek-flash",
        ),
    ).toBeDefined();

    fireEvent.click(
        screen.getByRole("button", {
            name: "Clear Deep Seek as the Codex default",
        }),
    );
    await screen.findByRole("region", { name: "Review Codex change" });
    expect(preview).toHaveBeenCalledWith({
        providerId: codexProviderId,
        expectedRevision: 1,
        action: "clear_default",
        modelId: null,
    });
    expect(
        screen.getByRole("button", { name: "Remove Edited from Codex" }),
    ).toBeDefined();
});

test("the Chinese copy covers the new error codes", async () => {
    preview.mockRejectedValue(new CodexSettingsError("default_in_use"));
    readState.mockResolvedValue(codexSettingsState);
    await mountManage("zh-CN");
    fireEvent.click(
        await screen.findByRole("button", { name: "从 Codex 移除 Deep Seek" }),
    );
    expect((await screen.findByRole("alert")).textContent).toBe(
        "该 Provider 是 Codex 默认值，请先清除默认再移除。",
    );
});

test("revoke and clear_default send the owned revision, not the central one", async () => {
    // The central provider moved on to revision 5; the binding still owns 1.
    vi.mocked(listProviders).mockResolvedValue({
        kind: "desktop",
        page: {
            items: [{ ...provider, revision: 5 }],
            nextCursor: null,
            total: 1,
        },
    });
    const owned = codexSettingsState.providers[0];
    if (owned === undefined) throw new Error("missing fixture provider");
    readState.mockResolvedValue({
        ...codexSettingsState,
        providers: [{ ...owned, outdated: true }],
    });
    await mountManage();
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Remove Deep Seek from Codex",
        }),
    );
    await screen.findByRole("region", { name: "Review Codex change" });
    expect(preview).toHaveBeenLastCalledWith({
        providerId: codexProviderId,
        expectedRevision: 1,
        action: "revoke",
        modelId: null,
    });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(
        await screen.findByRole("button", {
            name: "Clear Deep Seek as the Codex default",
        }),
    );
    await waitFor(() => {
        expect(preview).toHaveBeenLastCalledWith({
            providerId: codexProviderId,
            expectedRevision: 1,
            action: "clear_default",
            modelId: null,
        });
    });
});

test("deploy and set_default still send the central revision", async () => {
    vi.mocked(listProviders).mockResolvedValue({
        kind: "desktop",
        page: {
            items: [{ ...provider, revision: 5 }],
            nextCursor: null,
            total: 1,
        },
    });
    await mountDialog();
    await review();
    expect(preview).toHaveBeenCalledWith({
        providerId: codexProviderId,
        expectedRevision: 5,
        action: "deploy",
        modelId: null,
    });
});

test.each(["missing", "modified"] as const)(
    "a %s default offers a release-only clear that won't change the file",
    async (observed) => {
        const releasePlan = {
            ...codexSetDefaultPlan,
            action: "clear_default" as const,
            modelId: null,
            notices: ["release_only" as const],
            writesFile: false,
            summary: {
                ...codexSetDefaultPlan.summary,
                changes: [],
            },
        };
        preview.mockResolvedValue(releasePlan);
        readState.mockResolvedValue({
            ...codexSettingsState,
            default: { ...ownedDefault, state: observed },
        });
        await mountManage();
        const panel = await screen.findByRole("list", {
            name: "Providers vibemate added to Codex",
        });
        expect(
            within(panel).getByText(
                "vibemate-deep-seek · Written by vibemate · Codex default changed outside vibemate",
            ),
        ).toBeDefined();
        // The stale pair is never shown as if it were still configured.
        expect(screen.queryByText("Deep Seek · deepseek-flash")).toBeNull();
        expect(
            screen.getByText(
                observed === "missing"
                    ? "Removed from config.toml (Codex uses its own default)"
                    : "Own configuration (changed in Codex)",
            ),
        ).toBeDefined();
        fireEvent.click(
            screen.getByRole("button", {
                name: "Stop managing Deep Seek as the Codex default (config.toml won't change)",
            }),
        );
        const card = await screen.findByRole("region", {
            name: "Review Codex change",
        });
        expect(preview).toHaveBeenCalledWith({
            providerId: codexProviderId,
            expectedRevision: 1,
            action: "clear_default",
            modelId: null,
        });
        expect(
            within(card).getByText(
                "Only vibemate's record is released; config.toml is not changed.",
            ),
        ).toBeDefined();
        expect(
            within(card).queryByText(
                "config.toml already matches; confirming only records the change.",
            ),
        ).toBeNull();
        fireEvent.click(within(card).getByRole("button", { name: "Confirm" }));
        await waitFor(() => {
            expect(apply).toHaveBeenCalledWith(
                expect.objectContaining({ action: "clear_default" }),
                releasePlan,
            );
        });
    },
);

test("an unreadable default offers no clear action", async () => {
    readState.mockResolvedValue({
        ...codexSettingsState,
        default: { ...ownedDefault, state: "unreadable" },
    });
    await mountManage();
    await screen.findByRole("list", {
        name: "Providers vibemate added to Codex",
    });
    expect(screen.queryByRole("button", { name: /Codex default/u })).toBeNull();
    expect(
        screen.getByRole("button", { name: "Remove Deep Seek from Codex" }),
    ).toBeDefined();
});

test("the Chinese copy names a missing default", async () => {
    readState.mockResolvedValue({
        ...codexSettingsState,
        default: { ...ownedDefault, state: "missing" },
    });
    await mountManage("zh-CN");
    expect(
        await screen.findByText(
            "已从 config.toml 移除（Codex 使用其自身默认值）",
        ),
    ).toBeDefined();
    expect(
        screen.getByRole("button", {
            name: "不再将 Deep Seek 作为 Codex 默认值管理（不修改 config.toml）",
        }),
    ).toBeDefined();
});
