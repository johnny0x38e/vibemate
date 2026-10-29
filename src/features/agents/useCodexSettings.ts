import { useEffect, useRef, useState } from "react";
import {
    CodexSettingsError,
    applyCodexSettingsChange,
    previewCodexSettingsChange,
    readCodexSettingsHistory,
    readCodexSettingsState,
    recoverCodexSettingsChange,
    type CodexSettingsApplied,
    type CodexSettingsErrorCode,
    type CodexSettingsHistoryEntry,
    type CodexSettingsPlan,
    type CodexSettingsRecovery,
    type CodexSettingsSelection,
    type CodexSettingsState,
} from "../../lib/desktop/codex";

/** Root state plus the newest history page; history failures stay local. */
export type CodexSettingsRead =
    | { kind: "loading" }
    | { kind: "error"; code: CodexSettingsErrorCode }
    | {
          kind: "ready";
          state: CodexSettingsState;
          history: CodexSettingsHistoryEntry[] | null;
      };

/**
 * One review step. `queue` holds the follow-up selections (deploy, then
 * set_default) that are previewed only after this one is confirmed, because
 * Rust refuses `set_default` for a provider table that is not written yet.
 * `refreshed` marks a plan rebuilt after `plan_stale`.
 */
export type CodexSettingsStep =
    | { kind: "idle" }
    | { kind: "previewing"; selection: CodexSettingsSelection }
    | {
          kind: "review";
          selection: CodexSettingsSelection;
          plan: CodexSettingsPlan;
          queue: CodexSettingsSelection[];
          index: number;
          total: number;
          refreshed: boolean;
      }
    | { kind: "recovery"; recovery: CodexSettingsRecovery }
    | { kind: "applying" }
    | { kind: "error"; code: CodexSettingsErrorCode }
    | { kind: "done"; applied: CodexSettingsApplied; recovered: boolean };

/** Stable Codex code for any thrown value; never an IPC message. */
export function codexSettingsErrorCode(error: unknown): CodexSettingsErrorCode {
    return error instanceof CodexSettingsError
        ? error.code
        : "invalid_response";
}

/**
 * Codex provider/default writes: read, review, confirm, recover. Mirrors the
 * Pi/Grok/Claude flow (preview → explicit confirm) but on `codex.ts`, which
 * owns its own review contract and history.
 */
export function useCodexSettings({
    onBusyChange,
    onApplied,
}: {
    onBusyChange?: (busy: boolean) => void;
    onApplied?: () => void;
}): {
    read: CodexSettingsRead;
    step: CodexSettingsStep;
    busy: boolean;
    reload: () => void;
    review: (selections: CodexSettingsSelection[]) => Promise<void>;
    confirm: () => Promise<void>;
    reviewRecovery: () => void;
    dismiss: () => void;
} {
    const [read, setRead] = useState<CodexSettingsRead>({ kind: "loading" });
    const [step, setStep] = useState<CodexSettingsStep>({ kind: "idle" });
    const [epoch, setEpoch] = useState(0);
    const mounted = useRef(true);
    const busyRef = useRef(false);
    const busyCallback = useRef(onBusyChange);
    const appliedCallback = useRef(onApplied);

    // Latest callbacks for async completions; parents pass inline closures.
    useEffect(() => {
        busyCallback.current = onBusyChange;
        appliedCallback.current = onApplied;
    });

    useEffect(() => {
        mounted.current = true;
        return () => {
            mounted.current = false;
            if (busyRef.current) busyCallback.current?.(false);
        };
    }, []);

    useEffect(() => {
        let current = true;
        // A failed history page leaves the root state usable.
        void readCodexSettingsState().then(
            async (state) => {
                const history = await readCodexSettingsHistory(
                    null,
                    null,
                    20,
                ).then(
                    (page) => page.items,
                    () => null,
                );
                if (!current) return;
                setRead({ kind: "ready", state, history });
            },
            (error: unknown) => {
                if (!current) return;
                setRead({ kind: "error", code: codexSettingsErrorCode(error) });
            },
        );
        return () => {
            current = false;
        };
    }, [epoch]);

    function setBusy(next: boolean): void {
        if (busyRef.current === next) return;
        busyRef.current = next;
        busyCallback.current?.(next);
    }

    async function previewFrom(
        queue: CodexSettingsSelection[],
        index: number,
        total: number,
        refreshed: boolean,
    ): Promise<void> {
        const [selection, ...rest] = queue;
        if (selection === undefined) return;
        setStep({ kind: "previewing", selection });
        setBusy(true);
        try {
            const plan = await previewCodexSettingsChange(selection);
            if (!mounted.current) return;
            setStep({
                kind: "review",
                selection,
                plan,
                queue: rest,
                index,
                total,
                refreshed,
            });
        } catch (error) {
            if (mounted.current)
                setStep({ kind: "error", code: codexSettingsErrorCode(error) });
        } finally {
            setBusy(false);
        }
    }

    async function review(selections: CodexSettingsSelection[]): Promise<void> {
        if (busyRef.current || selections.length === 0) return;
        await previewFrom(selections, 1, selections.length, false);
    }

    async function confirm(): Promise<void> {
        if (busyRef.current) return;
        const current = step;
        if (current.kind !== "review" && current.kind !== "recovery") return;
        setStep({ kind: "applying" });
        setBusy(true);
        try {
            const applied =
                current.kind === "recovery"
                    ? await recoverCodexSettingsChange(current.recovery)
                    : await applyCodexSettingsChange(
                          current.selection,
                          current.plan,
                      );
            if (!mounted.current) return;
            setEpoch((value) => value + 1);
            appliedCallback.current?.();
            if (current.kind === "review" && current.queue.length > 0) {
                setBusy(false);
                await previewFrom(
                    current.queue,
                    current.index + 1,
                    current.total,
                    false,
                );
                return;
            }
            setStep({
                kind: "done",
                applied,
                recovered: current.kind === "recovery",
            });
        } catch (error) {
            if (!mounted.current) return;
            const code = codexSettingsErrorCode(error);
            if (code === "plan_stale" && current.kind === "review") {
                // The file or provider moved since review: rebuild the plan
                // for the same selection and ask again, never auto-apply.
                setBusy(false);
                await previewFrom(
                    [current.selection, ...current.queue],
                    current.index,
                    current.total,
                    true,
                );
                return;
            }
            setStep({ kind: "error", code });
            setEpoch((value) => value + 1);
        } finally {
            setBusy(false);
        }
    }

    function reviewRecovery(): void {
        if (busyRef.current || read.kind !== "ready") return;
        const recovery = read.state.recovery;
        if (recovery === null || recovery.approval === null) return;
        setStep({ kind: "recovery", recovery });
    }

    return {
        read,
        step,
        busy: step.kind === "previewing" || step.kind === "applying",
        reload: () => {
            if (!busyRef.current) setEpoch((value) => value + 1);
        },
        review,
        confirm,
        reviewRecovery,
        dismiss: () => {
            if (!busyRef.current) setStep({ kind: "idle" });
        },
    };
}
