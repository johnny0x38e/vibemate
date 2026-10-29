import { useEffect, useState, type JSX } from "react";
import { useTranslation } from "react-i18next";
import { DetailPanel } from "../../components/DetailForm";
import { FieldSelect } from "../../components/FieldSelect";
import { IconButton } from "../../components/IconButton";
import { ModelCheckbox } from "../../components/ModelCheckbox";
import buttons from "../../components/actionButtons.module.css";
import detailForm from "../../components/detailForm.module.css";
import listActions from "../../components/listActions.module.css";
import {
    ownedBindingSelection,
    type CodexDefaultBinding,
    type CodexProviderBinding,
    type CodexSettingsAction,
    type CodexSettingsPlan,
    type CodexSettingsRecovery,
    type CodexSettingsSelection,
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
import styles from "./AgentList.module.css";
import codex from "./CodexSettings.module.css";
import card from "./ConfigPreview.module.css";
import { useCodexSettings, type CodexSettingsStep } from "./useCodexSettings";
import { VersionSummary } from "./VersionRestoreCard";

type Providers =
    | { kind: "loading" }
    | { kind: "preview" }
    | { kind: "error" }
    | { kind: "ready"; items: ProviderRecord[] };

/** Last model answer, tagged with its Provider; another id means loading. */
type Models =
    | { kind: "none" }
    | { kind: "preview"; providerId: string }
    | { kind: "error"; providerId: string }
    | { kind: "ready"; providerId: string; items: ProviderModel[] };

/**
 * Codex provider and default model, written into `~/.codex/config.toml`
 * through `codex.ts`. `write` is the list-row dialog body (pick a Provider,
 * optionally make it the Codex default); `manage` is the detail History
 * segment (owned providers, revoke / clear default, history). Both show the
 * profile override note, the interruption recovery entry and the same
 * review → confirm cards.
 */
export function CodexSettingsPanel({
    mode,
    blocked,
    onBusyChange,
    onApplied,
}: {
    mode: "write" | "manage";
    blocked: boolean;
    onBusyChange?: (busy: boolean) => void;
    onApplied?: () => void;
}): JSX.Element {
    const { t } = useTranslation();
    const settings = useCodexSettings({
        ...(onBusyChange === undefined ? {} : { onBusyChange }),
        ...(onApplied === undefined ? {} : { onApplied }),
    });
    const { read, step } = settings;
    const busy = blocked || settings.busy;
    const [providers, setProviders] = useState<Providers>({
        kind: "loading",
    });

    useEffect(() => {
        let current = true;
        listProviders({ after: null, limit: 100 }).then(
            (result) => {
                if (!current) return;
                setProviders(
                    result.kind === "preview"
                        ? { kind: "preview" }
                        : { kind: "ready", items: result.page.items },
                );
            },
            () => {
                if (current) setProviders({ kind: "error" });
            },
        );
        return () => {
            current = false;
        };
    }, []);

    const state = read.kind === "ready" ? read.state : null;

    return (
        <>
            {read.kind === "loading" && (
                <p className={styles["hint"]}>{t("agents.codex.loading")}</p>
            )}
            {read.kind === "error" && (
                <p className={styles["error"]} role="alert">
                    {t(`agents.codex.errors.${read.code}`)}
                </p>
            )}
            {state !== null && state.profileActive && (
                <p className={card["note"]}>
                    {t("agents.codex.profileActive")}
                </p>
            )}
            {state?.recovery != null && (
                <RecoveryEntry
                    recovery={state.recovery}
                    blocked={busy}
                    onReview={settings.reviewRecovery}
                />
            )}
            {state !== null && mode === "write" && (
                <WriteForm
                    state={state}
                    providers={providers}
                    blocked={busy || state.recovery !== null}
                    onReview={(selections) => {
                        void settings.review(selections);
                    }}
                    onChange={settings.dismiss}
                />
            )}
            {state !== null && mode === "manage" && (
                <Bindings
                    state={state}
                    blocked={busy || state.recovery !== null}
                    onReview={(binding, action) => {
                        // Owned-only actions check the revision vibemate wrote,
                        // so an outdated binding can still be undone.
                        void settings.review([
                            ownedBindingSelection(binding, action),
                        ]);
                    }}
                    onReload={settings.reload}
                />
            )}
            <StepCard
                step={step}
                targetPath={state?.targetPath ?? null}
                blocked={blocked}
                onConfirm={() => {
                    void settings.confirm();
                }}
                onCancel={settings.dismiss}
            />
            {read.kind === "ready" && mode === "manage" && (
                <History entries={read.history} />
            )}
        </>
    );
}

function WriteForm({
    state,
    providers,
    blocked,
    onReview,
    onChange,
}: {
    state: CodexSettingsState;
    providers: Providers;
    blocked: boolean;
    onReview: (selections: CodexSettingsSelection[]) => void;
    onChange: () => void;
}): JSX.Element {
    const { t } = useTranslation();
    const items = providers.kind === "ready" ? providers.items : [];
    const [providerId, setProviderId] = useState("");
    const [modelId, setModelId] = useState("");
    const [setDefault, setSetDefault] = useState(true);
    const [answer, setModels] = useState<Models>({ kind: "none" });
    // Start from the current Codex default when it is ours, else the first
    // saved Provider, so the form opens on something writable.
    const selectedId = items.some((item) => item.id === providerId)
        ? providerId
        : (items.find((item) => item.id === state.default?.providerId)?.id ??
          items[0]?.id ??
          "");
    const provider = items.find((item) => item.id === selectedId);

    useEffect(() => {
        if (selectedId === "") return;
        let current = true;
        listProviderModels({
            providerId: selectedId,
            after: null,
            limit: 200,
            filter: "selected",
        }).then(
            (result) => {
                if (!current) return;
                setModels(
                    result.kind === "preview"
                        ? { kind: "preview", providerId: selectedId }
                        : {
                              kind: "ready",
                              providerId: selectedId,
                              items: result.page.items,
                          },
                );
            },
            () => {
                if (current)
                    setModels({ kind: "error", providerId: selectedId });
            },
        );
        return () => {
            current = false;
        };
    }, [selectedId]);

    const models =
        answer.kind !== "none" && answer.providerId === selectedId
            ? answer
            : null;
    const modelItems = models?.kind === "ready" ? models.items : [];
    const selectedModel = modelItems.some((item) => item.modelId === modelId)
        ? modelId
        : (modelItems.find(
              (item) =>
                  state.default?.providerId === selectedId &&
                  item.modelId === state.default.modelId,
          )?.modelId ??
          modelItems[0]?.modelId ??
          "");

    if (providers.kind === "loading")
        return <p className={styles["hint"]}>{t("agents.codex.loading")}</p>;
    if (providers.kind === "error")
        return (
            <p className={styles["error"]} role="alert">
                {t("agents.codex.form.providersFailed")}
            </p>
        );
    if (providers.kind === "preview" || items.length === 0)
        return (
            <p className={styles["hint"]}>
                {t("agents.codex.form.noProviders")}
            </p>
        );

    const binding = state.providers.find(
        (row) => row.providerId === selectedId,
    );
    const needsDeploy =
        binding === undefined ||
        binding.outdated ||
        binding.state !== "written";
    const isDefault =
        state.default?.providerId === selectedId &&
        state.default.modelId === selectedModel &&
        state.default.state === "written";
    const selections: CodexSettingsSelection[] =
        provider === undefined
            ? []
            : [
                  ...(needsDeploy
                      ? [
                            {
                                providerId: provider.id,
                                expectedRevision: provider.revision,
                                action: "deploy" as const,
                                modelId: null,
                            },
                        ]
                      : []),
                  ...(setDefault && selectedModel !== "" && !isDefault
                      ? [
                            {
                                providerId: provider.id,
                                expectedRevision: provider.revision,
                                action: "set_default" as const,
                                modelId: selectedModel,
                            },
                        ]
                      : []),
              ];
    const missingModel = setDefault && selectedModel === "";
    const canReview = !blocked && selections.length > 0 && !missingModel;

    return (
        <DetailPanel label={t("agents.codex.form.title")}>
            <div className={detailForm["row"]}>
                <label className={detailForm["label"]} htmlFor="codex-provider">
                    {t("agents.codex.form.provider")}
                </label>
                <span
                    className={[detailForm["value"], card["selectValue"]].join(
                        " ",
                    )}
                >
                    <FieldSelect
                        id="codex-provider"
                        value={selectedId}
                        options={items.map((item) => ({
                            value: item.id,
                            label: item.displayName,
                        }))}
                        size="fill"
                        blocked={blocked}
                        onChange={(value) => {
                            setProviderId(value);
                            setModelId("");
                            onChange();
                        }}
                    />
                </span>
            </div>
            {binding !== undefined && (
                <p className={styles["hint"]}>{t(bindingStateKey(binding))}</p>
            )}
            <div className={detailForm["row"]}>
                <label className={detailForm["label"]} htmlFor="codex-model">
                    {t("agents.codex.form.model")}
                </label>
                <span
                    className={[detailForm["value"], card["selectValue"]].join(
                        " ",
                    )}
                >
                    {modelItems.length > 0 ? (
                        <FieldSelect
                            id="codex-model"
                            value={selectedModel}
                            options={modelItems.map((item) => ({
                                value: item.modelId,
                                label: item.alias ?? item.modelId,
                            }))}
                            size="fill"
                            blocked={blocked}
                            onChange={(value) => {
                                setModelId(value);
                                onChange();
                            }}
                        />
                    ) : (
                        <span className={styles["hint"]} id="codex-model">
                            {models?.kind === "error"
                                ? t("agents.codex.form.modelsFailed")
                                : models !== null
                                  ? t("agents.codex.form.noModels")
                                  : t("agents.codex.loading")}
                        </span>
                    )}
                </span>
            </div>
            {/* The checkbox carries its own accessible name; the text is a caption. */}
            <div className={codex["choice"]}>
                <ModelCheckbox
                    checked={setDefault}
                    disabled={blocked}
                    ariaLabel={t("agents.codex.form.setDefault")}
                    onCheckedChange={(checked) => {
                        setSetDefault(checked);
                        onChange();
                    }}
                />
                <span className={codex["choiceText"]}>
                    <span>{t("agents.codex.form.setDefault")}</span>
                    <span className={codex["choiceHint"]}>
                        {t("agents.codex.form.setDefaultHint")}
                    </span>
                </span>
            </div>
            {selections.length === 0 && !missingModel && (
                <p className={styles["hint"]}>
                    {t("agents.codex.form.upToDate")}
                </p>
            )}
            {missingModel && (
                <p className={styles["hint"]}>
                    {t("agents.codex.form.modelRequired")}
                </p>
            )}
            <div className={card["actions"]}>
                <button
                    type="button"
                    className={buttons["primary"]}
                    aria-disabled={!canReview}
                    onClick={() => {
                        if (canReview) onReview(selections);
                    }}
                >
                    {t("agents.codex.form.review")}
                </button>
            </div>
        </DetailPanel>
    );
}

/** Owned provider state; modified or missing tables now belong to the user. */
function bindingStateKey(
    binding: CodexProviderBinding,
):
    | "agents.codex.binding.written"
    | "agents.codex.binding.outdated"
    | "agents.codex.binding.own"
    | "agents.codex.binding.unreadable" {
    if (binding.state === "modified" || binding.state === "missing")
        return "agents.codex.binding.own";
    if (binding.state === "unreadable")
        return "agents.codex.binding.unreadable";
    return binding.outdated
        ? "agents.codex.binding.outdated"
        : "agents.codex.binding.written";
}

/** Default row value; a missing default never shows a stale provider · model. */
function DefaultValue({ state }: { state: CodexSettingsState }): JSX.Element {
    const { t } = useTranslation();
    const fallback: CodexDefaultBinding | null = state.default;
    if (fallback === null) return <>{t("agents.codex.binding.noDefault")}</>;
    switch (fallback.state) {
        case "modified":
            return <>{t("agents.codex.binding.defaultOwn")}</>;
        case "missing":
            return <>{t("agents.codex.binding.defaultMissing")}</>;
        case "unreadable":
            return <>{t("agents.codex.binding.unreadable")}</>;
        case "written":
            return (
                <>
                    {t("agents.summary.providerModel", {
                        provider:
                            state.providers.find(
                                (row) => row.providerId === fallback.providerId,
                            )?.displayName ?? fallback.providerKey,
                        model: fallback.modelId,
                    })}
                </>
            );
    }
}

function Bindings({
    state,
    blocked,
    onReview,
    onReload,
}: {
    state: CodexSettingsState;
    blocked: boolean;
    onReview: (
        binding: CodexProviderBinding,
        action: Extract<CodexSettingsAction, "revoke" | "clear_default">,
    ) => void;
    onReload: () => void;
}): JSX.Element {
    const { t } = useTranslation();
    const fallback = state.default;
    return (
        <DetailPanel label={t("agents.codex.binding.title")}>
            <div className={detailForm["row"]}>
                <span className={detailForm["label"]}>
                    {t("agents.codex.binding.target")}
                </span>
                <span className={[detailForm["value"], card["path"]].join(" ")}>
                    {state.targetPath}
                </span>
            </div>
            <div className={detailForm["row"]}>
                <span className={detailForm["label"]}>
                    {t("agents.codex.binding.default")}
                </span>
                <span className={detailForm["value"]}>
                    <DefaultValue state={state} />
                </span>
            </div>
            {state.providers.length === 0 ? (
                <p className={styles["hint"]}>
                    {t("agents.codex.binding.empty")}
                </p>
            ) : (
                <ul
                    className={codex["bindings"]}
                    aria-label={t("agents.codex.binding.listLabel")}
                >
                    {state.providers.map((binding) => {
                        const ownsDefault =
                            fallback?.providerId === binding.providerId;
                        const isDefault =
                            ownsDefault && fallback.state === "written";
                        // A changed or removed default only releases
                        // ownership; without it revoke stays default_in_use.
                        const releaseDefault =
                            ownsDefault &&
                            (fallback.state === "modified" ||
                                fallback.state === "missing");
                        return (
                            <li
                                key={binding.providerId}
                                className={codex["binding"]}
                            >
                                <span className={codex["bindingText"]}>
                                    <span className={codex["bindingName"]}>
                                        {binding.displayName}
                                    </span>
                                    <span className={codex["bindingMeta"]}>
                                        {[
                                            binding.owner.providerKey,
                                            t(bindingStateKey(binding)),
                                            ...(isDefault
                                                ? [
                                                      t(
                                                          "agents.codex.binding.isDefault",
                                                      ),
                                                  ]
                                                : []),
                                            ...(releaseDefault
                                                ? [
                                                      t(
                                                          "agents.codex.binding.defaultReleasable",
                                                      ),
                                                  ]
                                                : []),
                                        ].join(" · ")}
                                    </span>
                                </span>
                                <span className={listActions["group"]}>
                                    {(isDefault || releaseDefault) && (
                                        <IconButton
                                            icon="close"
                                            blocked={blocked}
                                            label={t(
                                                releaseDefault
                                                    ? "agents.codex.binding.releaseDefault"
                                                    : "agents.codex.binding.clearDefault",
                                                { name: binding.displayName },
                                            )}
                                            onClick={() => {
                                                onReview(
                                                    binding,
                                                    "clear_default",
                                                );
                                            }}
                                        />
                                    )}
                                    <IconButton
                                        icon="trash"
                                        destructive
                                        blocked={blocked}
                                        label={t(
                                            "agents.codex.binding.revoke",
                                            { name: binding.displayName },
                                        )}
                                        onClick={() => {
                                            onReview(binding, "revoke");
                                        }}
                                    />
                                </span>
                            </li>
                        );
                    })}
                </ul>
            )}
            <div className={card["actions"]}>
                <IconButton
                    icon="refresh"
                    blocked={blocked}
                    label={t("agents.codex.reload")}
                    onClick={onReload}
                />
            </div>
        </DetailPanel>
    );
}

function RecoveryEntry({
    recovery,
    blocked,
    onReview,
}: {
    recovery: CodexSettingsRecovery;
    blocked: boolean;
    onReview: () => void;
}): JSX.Element {
    const { t } = useTranslation();
    return (
        <DetailPanel label={t("agents.codex.recovery.title")}>
            <p className={styles["error"]} role="alert">
                {t(`agents.codex.recovery.${recovery.observation}`, {
                    action: t(`agents.codex.actions.${recovery.action}`),
                    provider: recovery.providerKey,
                })}
            </p>
            {recovery.approval === null ? (
                <p className={styles["hint"]}>
                    {t("agents.codex.recovery.manual")}
                </p>
            ) : (
                <div className={card["actions"]}>
                    <button
                        type="button"
                        className={buttons["secondary"]}
                        aria-disabled={blocked}
                        onClick={() => {
                            if (!blocked) onReview();
                        }}
                    >
                        {t("agents.codex.recovery.review")}
                    </button>
                </div>
            )}
        </DetailPanel>
    );
}

function StepCard({
    step,
    targetPath,
    blocked,
    onConfirm,
    onCancel,
}: {
    step: CodexSettingsStep;
    targetPath: string | null;
    blocked: boolean;
    onConfirm: () => void;
    onCancel: () => void;
}): JSX.Element | null {
    const { t } = useTranslation();
    switch (step.kind) {
        case "idle":
            return null;
        case "previewing":
            return (
                <p className={styles["hint"]} role="status">
                    {t("agents.codex.review.loading")}
                </p>
            );
        case "applying":
            return (
                <p className={styles["hint"]} role="status">
                    {t("agents.codex.review.applying")}
                </p>
            );
        case "error":
            return (
                <p className={styles["error"]} role="alert">
                    {t(`agents.codex.errors.${step.code}`)}
                </p>
            );
        case "done":
            return (
                <p className={styles["hint"]} role="status">
                    {t(
                        step.recovered
                            ? "agents.codex.done.recovered"
                            : step.applied.restartRequired
                              ? "agents.codex.done.restart"
                              : "agents.codex.done.noChange",
                    )}
                </p>
            );
        case "recovery":
            return (
                <div className={card["followOn"]}>
                    <DetailPanel label={t("agents.codex.recovery.reviewTitle")}>
                        <p className={styles["hint"]}>
                            {t("agents.codex.recovery.confirmHint", {
                                action: t(
                                    `agents.codex.actions.${step.recovery.action}`,
                                ),
                                provider: step.recovery.providerKey,
                            })}
                        </p>
                        {targetPath !== null && (
                            <span className={card["path"]}>{targetPath}</span>
                        )}
                        <ReviewActions
                            blocked={blocked}
                            confirmLabel={t("agents.codex.recovery.confirm")}
                            onConfirm={onConfirm}
                            onCancel={onCancel}
                        />
                    </DetailPanel>
                </div>
            );
        case "review":
            return (
                <div className={card["followOn"]}>
                    <PlanCard
                        plan={step.plan}
                        index={step.index}
                        total={step.total}
                        refreshed={step.refreshed}
                        blocked={blocked}
                        onConfirm={onConfirm}
                        onCancel={onCancel}
                    />
                </div>
            );
    }
}

/** The exact masked plan Rust will recheck: target, notices and changes. */
function PlanCard({
    plan,
    index,
    total,
    refreshed,
    blocked,
    onConfirm,
    onCancel,
}: {
    plan: CodexSettingsPlan;
    index: number;
    total: number;
    refreshed: boolean;
    blocked: boolean;
    onConfirm: () => void;
    onCancel: () => void;
}): JSX.Element {
    const { t } = useTranslation();
    const others = plan.notices.filter(
        (notice) => notice !== "plaintext_credential",
    );
    return (
        <DetailPanel label={t("agents.codex.review.title")}>
            {total > 1 && (
                <span className={codex["step"]}>
                    {t("agents.codex.review.step", { index, total })}
                </span>
            )}
            {refreshed && (
                <p className={card["note"]} role="status">
                    {t("agents.codex.review.refreshed")}
                </p>
            )}
            <div className={detailForm["row"]}>
                <span className={detailForm["label"]}>
                    {t("agents.codex.review.action")}
                </span>
                <span className={detailForm["value"]}>
                    {t(`agents.codex.actions.${plan.action}`)}
                </span>
            </div>
            <div className={detailForm["row"]}>
                <span className={detailForm["label"]}>
                    {t("agents.codex.review.providerKey")}
                </span>
                <span className={detailForm["value"]}>{plan.providerKey}</span>
            </div>
            {plan.modelId !== null && (
                <div className={detailForm["row"]}>
                    <span className={detailForm["label"]}>
                        {t("agents.codex.review.model")}
                    </span>
                    <span className={detailForm["value"]}>{plan.modelId}</span>
                </div>
            )}
            <div className={card["summary"]}>
                <span className={card["path"]}>{plan.target.path}</span>
                {!plan.target.exists && (
                    <span className={card["fileState"]}>
                        {t("agents.codex.review.newFile")}
                    </span>
                )}
            </div>
            {plan.notices.includes("plaintext_credential") && (
                <p className={codex["warning"]} role="note">
                    {t("agents.codex.notices.plaintext_credential")}
                </p>
            )}
            {others.length > 0 && (
                <ul className={card["notices"]}>
                    {others.map((notice) => (
                        <li key={notice}>
                            {t(`agents.codex.notices.${notice}`)}
                        </li>
                    ))}
                </ul>
            )}
            {!plan.writesFile && !plan.notices.includes("release_only") && (
                <p className={card["note"]}>
                    {t("agents.codex.review.noWrite")}
                </p>
            )}
            <VersionSummary file={{ summary: plan.summary }} />
            <ReviewActions
                blocked={blocked}
                confirmLabel={t(
                    plan.writesFile
                        ? "agents.codex.review.confirm"
                        : "agents.codex.review.confirmRecord",
                )}
                onConfirm={onConfirm}
                onCancel={onCancel}
            />
        </DetailPanel>
    );
}

function ReviewActions({
    blocked,
    confirmLabel,
    onConfirm,
    onCancel,
}: {
    blocked: boolean;
    confirmLabel: string;
    onConfirm: () => void;
    onCancel: () => void;
}): JSX.Element {
    const { t } = useTranslation();
    return (
        <div className={card["actions"]}>
            <button
                type="button"
                className={buttons["secondary"]}
                aria-disabled={blocked}
                onClick={() => {
                    if (!blocked) onCancel();
                }}
            >
                {t("agents.codex.review.cancel")}
            </button>
            <button
                type="button"
                className={buttons["primary"]}
                aria-disabled={blocked}
                onClick={() => {
                    if (!blocked) onConfirm();
                }}
            >
                {confirmLabel}
            </button>
        </div>
    );
}

function History({
    entries,
}: {
    entries:
        | {
              id: string;
              action: CodexSettingsAction;
              providerKey: string;
              modelId: string | null;
              createdAt: number;
          }[]
        | null;
}): JSX.Element {
    const { t, i18n } = useTranslation();
    const format = new Intl.DateTimeFormat(i18n.language, {
        dateStyle: "medium",
        timeStyle: "short",
    });
    return (
        <DetailPanel label={t("agents.codex.history.title")}>
            {entries === null ? (
                <p className={styles["hint"]}>
                    {t("agents.codex.history.unavailable")}
                </p>
            ) : entries.length === 0 ? (
                <p className={styles["hint"]}>
                    {t("agents.codex.history.empty")}
                </p>
            ) : (
                <ul
                    className={codex["history"]}
                    aria-label={t("agents.codex.history.title")}
                >
                    {entries.map((entry) => (
                        <li key={entry.id} className={codex["historyRow"]}>
                            <span className={codex["historyAction"]}>
                                {entry.modelId === null
                                    ? t("agents.codex.history.entry", {
                                          action: t(
                                              `agents.codex.actions.${entry.action}`,
                                          ),
                                          provider: entry.providerKey,
                                      })
                                    : t("agents.codex.history.entryModel", {
                                          action: t(
                                              `agents.codex.actions.${entry.action}`,
                                          ),
                                          provider: entry.providerKey,
                                          model: entry.modelId,
                                      })}
                            </span>
                            <time
                                className={codex["historyTime"]}
                                dateTime={new Date(
                                    entry.createdAt,
                                ).toISOString()}
                            >
                                {format.format(entry.createdAt)}
                            </time>
                        </li>
                    ))}
                </ul>
            )}
        </DetailPanel>
    );
}
