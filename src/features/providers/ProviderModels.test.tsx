import {
    act,
    cleanup,
    within,
    fireEvent,
    render,
    screen,
    waitFor,
} from "@testing-library/react";
import { StrictMode } from "react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { createAppI18n } from "../../i18n";
import {
    addManualProviderModel,
    browseUpstreamModelsPage,
    cancelProviderModelFetch,
    clearModelReference,
    getProviderModelFetchStatus,
    getProviderModelSnapshot,
    listProviderModels,
    modelDeletionImpact,
    refreshSavedProviderModels,
    saveProviderModelSelections,
    type ProviderModel,
    updateProviderModelEdit,
} from "../../lib/desktop/models";
import { getProviderSecretStatus } from "../../lib/desktop/providerSecrets";
import { ProviderModels } from "./ProviderModels";

vi.mock(import("../../lib/desktop/models"), async (importOriginal) => ({
    ...(await importOriginal()),
    addManualProviderModel: vi.fn(),
    clearModelReference: vi.fn(),
    listProviderModels: vi.fn(),
    modelDeletionImpact: vi.fn(),
    saveProviderModelSelections: vi.fn(),
    browseUpstreamModelsPage: vi.fn(),
    cancelProviderModelFetch: vi.fn(),
    getProviderModelFetchStatus: vi.fn(),
    getProviderModelSnapshot: vi.fn(),
    refreshSavedProviderModels: vi.fn(),
    updateProviderModelEdit: vi.fn(),
}));
vi.mock(
    import("../../lib/desktop/providerSecrets"),
    async (importOriginal) => ({
        ...(await importOriginal()),
        getProviderSecretStatus: vi.fn(),
    }),
);

const addManual = vi.mocked(addManualProviderModel);
const list = vi.mocked(listProviderModels);
const saveSelections = vi.mocked(saveProviderModelSelections);
const impact = vi.mocked(modelDeletionImpact);
const clearReference = vi.mocked(clearModelReference);
const browse = vi.mocked(browseUpstreamModelsPage);
const cancelFetch = vi.mocked(cancelProviderModelFetch);
const readFetchStatus = vi.mocked(getProviderModelFetchStatus);
const readKey = vi.mocked(getProviderSecretStatus);

const ID = "0123456789abcdef0123456789abcdef";

function model(overrides: Partial<ProviderModel> = {}): ProviderModel {
    return {
        providerId: ID,
        modelId: "deepseek-chat",
        source: "fetched",
        selected: true,
        alias: null,
        upstreamName: "DeepSeek Chat",
        contextWindow: 128000,
        maxOutputTokens: null,
        inputModalities: null,
        outputModalities: null,
        supportedEndpoints: null,
        capabilities: {
            thinking: { state: "unknown", source: null },
            imageInput: { state: "unknown", source: null },
            toolCalls: { state: "unknown", source: null },
        },
        capabilityOverrides: {
            thinking: null,
            imageInput: null,
            toolCalls: null,
        },
        thinkingEffort: null,
        routeSupport: "not_applicable",
        resolvedProtocol: "chat_completions",
        upstreamState: "listed",
        lastSeenAtMs: 1000,
        missingSinceMs: null,
        createdAtMs: 1000,
        updatedAtMs: 1000,
        ...overrides,
    };
}

function desktopPage(items: ProviderModel[]) {
    return {
        kind: "desktop" as const,
        page: { items, nextCursor: null, totalMatches: null },
    };
}

async function renderModels(hidden = false, locale: "en" | "zh-CN" = "en") {
    const i18n = await createAppI18n(locale);
    const tree = (isHidden: boolean) => (
        <StrictMode>
            <I18nextProvider i18n={i18n}>
                <ProviderModels providerId={ID} hidden={isHidden} />
            </I18nextProvider>
        </StrictMode>
    );
    const view = render(tree(hidden));
    return {
        ...view,
        rerenderHidden: (isHidden: boolean) => {
            view.rerender(tree(isHidden));
        },
    };
}

afterEach(() => {
    cleanup();
});
beforeEach(() => {
    addManual.mockReset();
    impact.mockReset().mockImplementation((providerId, modelId) =>
        Promise.resolve({
            providerId,
            modelId,
            references: [],
            rememberedPreferences: 0,
            canDelete: true,
        }),
    );
    clearReference.mockReset();
    vi.mocked(getProviderModelSnapshot).mockReset();
    vi.mocked(updateProviderModelEdit).mockReset();
    vi.mocked(refreshSavedProviderModels).mockReset();
    list.mockReset();
    saveSelections.mockReset().mockResolvedValue(undefined);
    browse.mockReset();
    cancelFetch.mockReset().mockResolvedValue({ wasRunning: false });
    readFetchStatus.mockReset().mockResolvedValue({
        kind: "desktop",
        status: { running: false, lastFetch: null },
    });
    readKey.mockReset().mockResolvedValue({
        kind: "desktop",
        status: { providerId: ID, state: "set", updatedAtMs: 1000 },
    });
});

describe("ProviderModels", () => {
    test("edits a saved model inline without changing membership and blocks competing actions", async () => {
        const row = model();
        list.mockResolvedValue(desktopPage([row]));
        vi.mocked(getProviderModelSnapshot).mockResolvedValue({
            model: row,
            contentDigest: "a".repeat(64),
        });
        vi.mocked(updateProviderModelEdit).mockResolvedValue({
            model: { ...row, alias: "Work model" },
            contentDigest: "b".repeat(64),
        });
        await renderModels();
        const edit = await screen.findByRole("button", {
            name: "Edit model deepseek-chat",
        });
        await waitFor(() => {
            expect(edit.getAttribute("aria-disabled")).toBe("false");
        });
        const checkbox = screen.getByRole("checkbox", {
            name: "Deselect deepseek-chat",
        });
        fireEvent.click(checkbox);
        expect(edit.getAttribute("aria-disabled")).toBe("true");
        fireEvent.click(edit);
        expect(getProviderModelSnapshot).not.toHaveBeenCalled();
        fireEvent.click(checkbox);
        // The row action remembers itself for focus return; jsdom click alone
        // does not move focus the way a real click does.
        edit.focus();
        fireEvent.click(edit);
        const editor = await screen.findByRole("form", {
            name: "Edit model deepseek-chat",
        });
        const field = within(editor).getByRole<HTMLInputElement>("textbox", {
            name: "Alias",
        });
        await waitFor(() => {
            expect(field.readOnly).toBe(false);
        });
        // While the editor is open, competing model operations stay put.
        fireEvent.click(checkbox);
        expect(checkbox.getAttribute("aria-checked")).toBe("true");
        fireEvent.change(field, { target: { value: "Work model" } });
        fireEvent.click(screen.getByRole("button", { name: "Save" }));
        await screen.findByText("Work model");
        await waitFor(() => {
            expect(document.activeElement).toBe(edit);
        });
        expect(
            screen
                .getByRole("checkbox", { name: "Deselect deepseek-chat" })
                .getAttribute("aria-checked"),
        ).toBe("true");
        expect(saveSelections).not.toHaveBeenCalled();
        expect(browse).not.toHaveBeenCalled();

        // Reopen and cancel with Escape: the inline form collapses and returns
        // focus to the row action that opened it.
        edit.focus();
        fireEvent.click(edit);
        const reopened = await screen.findByRole("form", {
            name: "Edit model deepseek-chat",
        });
        const reopenedField = within(reopened).getByRole<HTMLInputElement>(
            "textbox",
            { name: "Alias" },
        );
        await waitFor(() => {
            expect(reopenedField.readOnly).toBe(false);
        });
        fireEvent.keyDown(reopenedField, { key: "Escape" });
        await waitFor(() => {
            expect(
                screen.queryByRole("form", {
                    name: "Edit model deepseek-chat",
                }),
            ).toBeNull();
        });
        await waitFor(() => {
            expect(document.activeElement).toBe(edit);
        });
        expect(updateProviderModelEdit).toHaveBeenCalledTimes(1);
    });

    test("hiding models closes the model editor and a late reply cannot rename a row", async () => {
        const row = model();
        list.mockResolvedValue(desktopPage([row]));
        vi.mocked(getProviderModelSnapshot).mockResolvedValue({
            model: row,
            contentDigest: "a".repeat(64),
        });
        let finish:
            | ((value: { model: ProviderModel; contentDigest: string }) => void)
            | undefined;
        vi.mocked(updateProviderModelEdit).mockImplementation(
            () =>
                new Promise((resolve) => {
                    finish = resolve;
                }),
        );
        const { rerenderHidden } = await renderModels();
        const edit = await screen.findByRole("button", {
            name: "Edit model deepseek-chat",
        });
        await waitFor(() => {
            expect(edit.getAttribute("aria-disabled")).toBe("false");
        });
        edit.focus();
        fireEvent.click(edit);
        const editor = await screen.findByRole("form", {
            name: "Edit model deepseek-chat",
        });
        const alias = within(editor).getByRole<HTMLInputElement>("textbox", {
            name: "Alias",
        });
        await waitFor(() => {
            expect(alias.readOnly).toBe(false);
        });
        fireEvent.click(screen.getByRole("button", { name: "Save" }));
        rerenderHidden(true);
        await act(async () => {
            finish?.({
                model: { ...row, alias: "Late reply" },
                contentDigest: "b".repeat(64),
            });
            await Promise.resolve();
        });
        rerenderHidden(false);
        await screen.findByText("DeepSeek Chat");
        expect(screen.queryByText("Late reply")).toBeNull();
        expect(
            screen.queryByRole("form", {
                name: "Edit model deepseek-chat",
            }),
        ).toBeNull();
    });

    test("prompts to fetch when no models are selected", async () => {
        list.mockResolvedValue(desktopPage([]));
        await renderModels();
        await screen.findByText(/Fetch models to browse/i);
    });

    test("shows only supported capabilities with accessible source labels", async () => {
        list.mockResolvedValue(
            desktopPage([
                model({
                    contextWindow: 1_048_576,
                    maxOutputTokens: 393_216,
                    capabilities: {
                        thinking: {
                            state: "supported",
                            source: "deepseek_model_effort_levels",
                        },
                        imageInput: {
                            state: "unsupported",
                            source: "deepseek_model_input_modalities",
                        },
                        toolCalls: { state: "unknown", source: null },
                    },
                }),
            ]),
        );
        await renderModels(false, "en");
        await screen.findByRole("checkbox", { name: "Deselect deepseek-chat" });
        const capability = screen.getByRole("img", {
            name: "Reasoning — declared by the provider catalog",
        });
        expect(capability).toBeTruthy();
        expect(
            screen.queryByRole("img", {
                name: "Image input — declared by the provider catalog",
            }),
        ).toBeNull();
        expect(
            screen.queryByRole("img", {
                name: "Tool calls — declared by the provider catalog",
            }),
        ).toBeNull();
        expect(
            screen.getByRole("img", { name: "Context window 1M" }),
        ).toBeTruthy();
        expect(
            screen.getByRole("img", { name: "Maximum output 500K" }),
        ).toBeTruthy();
    });

    test("user declarations hide catalog support and add icons over unknown", async () => {
        list.mockResolvedValue(
            desktopPage([
                model({
                    capabilities: {
                        thinking: {
                            state: "supported",
                            source: "deepseek_model_effort_levels",
                        },
                        imageInput: { state: "unknown", source: null },
                        toolCalls: { state: "unknown", source: null },
                    },
                    capabilityOverrides: {
                        thinking: "unsupported",
                        imageInput: null,
                        toolCalls: "supported",
                    },
                }),
            ]),
        );
        await renderModels(false, "en");
        await screen.findByRole("checkbox", { name: "Deselect deepseek-chat" });
        expect(
            screen.queryByRole("img", {
                name: "Reasoning — declared by the provider catalog",
            }),
        ).toBeNull();
        expect(
            screen.getByRole("img", { name: "Tool calls — declared by you" }),
        ).toBeTruthy();
    });

    test("refreshes saved model metadata and re-reads the list", async () => {
        list.mockResolvedValue(desktopPage([model()]));
        vi.mocked(refreshSavedProviderModels).mockResolvedValue({
            complete: true,
            incompleteReason: null,
            refreshed: 1,
            markedMissing: 0,
            skippedInvalid: 0,
            fetchedAtMs: 2000,
        });
        await renderModels();
        await screen.findByRole("checkbox", {
            name: "Deselect deepseek-chat",
        });
        const readsBefore = list.mock.calls.length;
        fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
        await waitFor(() => {
            expect(refreshSavedProviderModels).toHaveBeenCalledWith(ID);
        });
        await waitFor(() => {
            expect(list.mock.calls.length).toBeGreaterThan(readsBefore);
        });
        expect(saveSelections).not.toHaveBeenCalled();
        expect(browse).not.toHaveBeenCalled();
    });

    test("blocks refresh while checkbox drafts exist", async () => {
        list.mockResolvedValue(desktopPage([model()]));
        await renderModels();
        fireEvent.click(
            await screen.findByRole("checkbox", {
                name: "Deselect deepseek-chat",
            }),
        );
        const refreshButton = screen.getByRole("button", {
            name: "Refresh",
        });
        expect(refreshButton.getAttribute("aria-disabled")).toBe("true");
        fireEvent.click(refreshButton);
        expect(refreshSavedProviderModels).not.toHaveBeenCalled();
    });

    test("shows preview notice in browser mode", async () => {
        list.mockResolvedValue({ kind: "preview" });
        readFetchStatus.mockResolvedValue({ kind: "preview" });
        await renderModels();
        await screen.findByText(/browser preview cannot read saved models/i);
    });

    test("enters upstream browse when Fetch models is clicked", async () => {
        list.mockResolvedValue(desktopPage([]));
        browse.mockResolvedValue({
            items: [
                {
                    modelId: "new-model",
                    upstreamName: "New",
                    contextWindow: null,
                    maxOutputTokens: null,
                    inputModalities: null,
                    outputModalities: null,
                    supportedEndpoints: null,
                    capabilities: {
                        thinking: { state: "unknown", source: null },
                        imageInput: { state: "unknown", source: null },
                        toolCalls: { state: "unknown", source: null },
                    },
                    thinkingEffort: null,
                    routeSupport: "not_applicable",
                    resolvedProtocol: "chat_completions",
                },
            ],
            nextOffset: null,
            morePages: false,
        });
        await renderModels();
        await screen.findByText(/No models added yet/i);
        fireEvent.click(screen.getByRole("button", { name: "Fetch models" }));
        await waitFor(() => {
            expect(browse).toHaveBeenCalledWith({
                providerId: ID,
                offset: null,
            });
        });
        await screen.findByText("New");
    });

    test("save sends removals for unchecked selected rows", async () => {
        list.mockResolvedValue(desktopPage([model()]));
        await renderModels();
        const checkbox = await screen.findByRole("checkbox", {
            name: "Deselect deepseek-chat",
        });
        fireEvent.click(checkbox);
        fireEvent.click(screen.getByRole("button", { name: "Save" }));
        await waitFor(() => {
            expect(saveSelections).toHaveBeenCalledWith({
                providerId: ID,
                removeModelIds: ["deepseek-chat"],
                add: [],
            });
        });
    });

    test("save pauses for a live reference and resumes after a stale binding is cleared", async () => {
        list.mockResolvedValue(desktopPage([model()]));
        impact
            .mockResolvedValueOnce({
                providerId: ID,
                modelId: "deepseek-chat",
                references: [
                    {
                        agentId: "grokBuild",
                        configRoot: "/tmp/grok",
                        label: "vibemate-chat",
                        modelId: "deepseek-chat",
                        state: "stale",
                        clearable: true,
                    },
                ],
                rememberedPreferences: 0,
                canDelete: false,
            })
            .mockResolvedValue({
                providerId: ID,
                modelId: "deepseek-chat",
                references: [],
                rememberedPreferences: 0,
                canDelete: true,
            });
        await renderModels();
        fireEvent.click(
            await screen.findByRole("checkbox", {
                name: "Deselect deepseek-chat",
            }),
        );
        fireEvent.click(screen.getByRole("button", { name: "Save" }));

        expect(
            await screen.findByText(
                "Some models are still used by an Agent configuration, so they were not removed.",
            ),
        ).toBeDefined();
        expect(saveSelections).not.toHaveBeenCalled();
        fireEvent.click(
            screen.getByRole("button", { name: "Clear reference" }),
        );
        await waitFor(() => {
            expect(clearReference).toHaveBeenCalledWith({
                providerId: ID,
                modelId: "deepseek-chat",
                agentId: "grokBuild",
                configRoot: "/tmp/grok",
                label: "vibemate-chat",
            });
        });
        await waitFor(() => {
            expect(
                screen.queryByText(
                    "Some models are still used by an Agent configuration, so they were not removed.",
                ),
            ).toBeNull();
        });
        fireEvent.click(screen.getByRole("button", { name: "Save" }));
        await waitFor(() => {
            expect(saveSelections).toHaveBeenCalledWith({
                providerId: ID,
                removeModelIds: ["deepseek-chat"],
                add: [],
            });
        });
    });
});

test("manual addition from upstream browse is saved and visible in added models", async () => {
    list.mockResolvedValue(desktopPage([]));
    browse.mockResolvedValue({
        items: [],
        nextOffset: null,
        morePages: false,
    });
    const manual = model({
        modelId: "vendor/manual",
        source: "manual",
        alias: "Work",
    });
    addManual.mockResolvedValue(manual);
    await renderModels(false, "en");
    const fetchButton = await screen.findByRole("button", {
        name: "Fetch models",
    });
    await waitFor(() => {
        expect(fetchButton.getAttribute("aria-disabled")).toBe("false");
    });
    fireEvent.click(fetchButton);
    await screen.findByRole("button", { name: "View added models" });
    fireEvent.click(
        screen.getByRole("button", { name: "Add a model manually" }),
    );
    fireEvent.change(screen.getByLabelText("Model ID"), {
        target: { value: "vendor/manual" },
    });
    fireEvent.change(screen.getByLabelText("Alias"), {
        target: { value: "Work" },
    });
    await waitFor(() => {
        expect(
            screen
                .getByRole("button", { name: "Add and save" })
                .getAttribute("aria-disabled"),
        ).toBe("false");
    });
    fireEvent.click(screen.getByRole("button", { name: "Add and save" }));
    await waitFor(() => {
        expect(addManual).toHaveBeenCalledWith({
            providerId: ID,
            modelId: "vendor/manual",
            alias: "Work",
            capabilityOverrides: {
                thinking: null,
                imageInput: null,
                toolCalls: null,
            },
            contextWindow: null,
            maxOutputTokens: null,
            thinkingEffort: null,
        });
    });
    await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
    });
    list.mockResolvedValue(desktopPage([manual]));
    fireEvent.click(screen.getByRole("button", { name: "View added models" }));
    await screen.findByRole("checkbox", { name: "Deselect vendor/manual" });
    expect(saveSelections).not.toHaveBeenCalled();
});

test("manual creation updates the selected row and count without a provider request", async () => {
    list.mockResolvedValue(desktopPage([]));
    const manual = model({
        modelId: "vendor/manual",
        source: "manual",
        alias: "Work",
    });
    addManual.mockResolvedValue(manual);
    const count = vi.fn();
    const i18n = await createAppI18n("en");
    render(
        <I18nextProvider i18n={i18n}>
            <ProviderModels providerId={ID} onSelectedCountChange={count} />
        </I18nextProvider>,
    );
    fireEvent.click(
        await screen.findByRole("button", { name: "Add a model manually" }),
    );
    fireEvent.change(screen.getByLabelText("Model ID"), {
        target: { value: "vendor/manual" },
    });
    fireEvent.change(screen.getByLabelText("Alias"), {
        target: { value: "Work" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add and save" }));
    await screen.findByRole("checkbox", { name: "Deselect vendor/manual" });
    expect(count).toHaveBeenCalledWith({ count: 1, hasMore: false });
    expect(browse).not.toHaveBeenCalled();
    expect(saveSelections).not.toHaveBeenCalled();
    expect(
        screen
            .getByRole("button", { name: "Save" })
            .getAttribute("aria-disabled"),
    ).toBe("true");
});

test("manual rows are removed only when their checkbox draft is saved", async () => {
    list.mockResolvedValue(
        desktopPage([model({ source: "manual", alias: "Custom" })]),
    );
    await renderModels();
    fireEvent.click(
        await screen.findByRole("checkbox", { name: "Deselect deepseek-chat" }),
    );
    expect(saveSelections).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
        expect(saveSelections).toHaveBeenCalledWith({
            providerId: ID,
            removeModelIds: ["deepseek-chat"],
            add: [],
        });
    });
});

test("pending checkbox drafts block manual creation", async () => {
    list.mockResolvedValue(desktopPage([model()]));
    await renderModels();
    fireEvent.click(
        await screen.findByRole("checkbox", { name: "Deselect deepseek-chat" }),
    );
    const addButton = screen.getByRole("button", {
        name: "Add a model manually",
    });
    expect(addButton.getAttribute("aria-disabled")).toBe("true");
    fireEvent.click(addButton);
    expect(addManual).not.toHaveBeenCalled();
    expect(screen.getByText(/Save or discard checkbox changes/)).toBeDefined();
});

test("manual writes block browsing and late hidden replies do not add rows", async () => {
    list.mockResolvedValue(desktopPage([]));
    let resolve: (model: ProviderModel) => void = () => {};
    addManual.mockImplementation(
        () =>
            new Promise((done) => {
                resolve = done;
            }),
    );
    const view = await renderModels();
    fireEvent.click(
        await screen.findByRole("button", { name: "Add a model manually" }),
    );
    fireEvent.change(screen.getByLabelText("Model ID"), {
        target: { value: "vendor/manual" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add and save" }));
    const fetchButton = screen.getByRole("button", {
        name: "Fetch models",
        hidden: true,
    });
    expect(fetchButton.getAttribute("aria-disabled")).toBe("true");
    fireEvent.click(fetchButton);
    expect(browse).not.toHaveBeenCalled();
    view.rerenderHidden(true);
    await act(async () => {
        resolve(model({ source: "manual", modelId: "vendor/manual" }));
        await Promise.resolve();
    });
    await waitFor(() => {
        expect(
            screen.queryByRole("checkbox", { name: "Deselect vendor/manual" }),
        ).toBeNull();
    });
});

test("a Codex default on a removed model blocks the save and says where to clear it", async () => {
    list.mockResolvedValue(desktopPage([model()]));
    impact.mockResolvedValue({
        providerId: ID,
        modelId: "deepseek-chat",
        references: [],
        codexReferences: [
            {
                configRoot: "/home/test/.codex",
                providerKey: "vibemate-deep-seek",
                modelId: "deepseek-chat",
                binding: "default",
                state: "active",
            },
        ],
        rememberedPreferences: 0,
        canDelete: false,
    });
    await renderModels();
    fireEvent.click(
        await screen.findByRole("checkbox", {
            name: "Deselect deepseek-chat",
        }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const panel = await screen.findByRole("region", {
        name: "Agent references blocking removal",
    });
    expect(
        within(panel).getByText(
            "Codex · default model deepseek-chat · provider vibemate-deep-seek",
        ),
    ).toBeDefined();
    expect(
        within(panel).getByText(
            "/home/test/.codex · still present in the Agent file",
        ),
    ).toBeDefined();
    expect(
        within(panel).getByText(
            "Clear the Codex default in Agents › Codex › Settings first.",
        ),
    ).toBeDefined();
    // Codex rows are never cleared from here.
    expect(
        within(panel).queryByRole("button", { name: "Clear reference" }),
    ).toBeNull();
    expect(saveSelections).not.toHaveBeenCalled();
});
