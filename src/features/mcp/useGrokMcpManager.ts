import { useEffect, useRef, useState } from "react";
import {
    applyGrokMcpChange,
    GrokMcpError,
    previewGrokMcpChange,
    readGrokMcpHistory,
    readGrokMcpState,
    recoverGrokMcpChange,
    type GrokMcpAction,
    type GrokMcpApplied,
    type GrokMcpErrorCode,
    type GrokMcpHistory,
    type GrokMcpPlan,
    type GrokMcpRecovery,
    type GrokMcpSelection,
    type GrokMcpState,
} from "../../lib/desktop/grokMcp";
import type { McpRecord } from "../../lib/desktop/mcp";

export type GrokMcpReadState =
    | { kind: "loading" }
    | { kind: "error"; code: GrokMcpErrorCode }
    | { kind: "ready"; state: GrokMcpState; history: GrokMcpHistory };

export type GrokMcpStep =
    | { kind: "idle" }
    | { kind: "previewing" | "running" }
    | { kind: "review"; selection: GrokMcpSelection; plan: GrokMcpPlan }
    | { kind: "recovery"; recovery: GrokMcpRecovery }
    | { kind: "error"; code: GrokMcpErrorCode }
    | { kind: "done"; result: GrokMcpApplied };

function code(error: unknown): GrokMcpErrorCode {
    return error instanceof GrokMcpError ? error.code : "outcome_unknown";
}

/** Shared Grok Build registration session for Manager dialogs. */
export function useGrokMcpManager(
    record: McpRecord,
    onBusyChange: (busy: boolean) => void,
    onChanged: () => void,
): {
    read: GrokMcpReadState;
    step: GrokMcpStep;
    locked: boolean;
    more: boolean;
    current: GrokMcpState["deployments"][number] | undefined;
    pending: GrokMcpRecovery | null | undefined;
    reviewing: boolean;
    review: (action: GrokMcpAction, historyId?: string | null) => Promise<void>;
    confirm: () => Promise<void>;
    refresh: () => void;
    loadMore: () => Promise<void>;
    setStep: (step: GrokMcpStep) => void;
    mutating: () => boolean;
} {
    const [read, setRead] = useState<GrokMcpReadState>({ kind: "loading" });
    const [step, setStep] = useState<GrokMcpStep>({ kind: "idle" });
    const [attempt, setAttempt] = useState(0);
    const [more, setMore] = useState(false);
    const alive = useRef(false),
        generation = useRef(0),
        mutation = useRef(false),
        readingMore = useRef(false);
    useEffect(() => {
        alive.current = true;
        return () => {
            alive.current = false;
            generation.current += 1;
        };
    }, []);
    useEffect(() => {
        // IPC has no cancellation signal. Only this mounted read generation may
        // replace data, including after StrictMode's initial cleanup.
        const token = ++generation.current;
        Promise.all([
            readGrokMcpState(record.id),
            readGrokMcpHistory(record.id),
        ]).then(
            ([state, history]) => {
                if (alive.current && generation.current === token)
                    setRead({ kind: "ready", state, history });
            },
            (error: unknown) => {
                if (alive.current && generation.current === token)
                    setRead({ kind: "error", code: code(error) });
            },
        );
        return () => {
            generation.current += 1;
        };
    }, [record.id, record.revision, attempt]);
    const locked =
        step.kind === "previewing" || step.kind === "running" || more;
    function refresh(): void {
        if (mutation.current || readingMore.current) return;
        generation.current++;
        setRead({ kind: "loading" });
        setStep({ kind: "idle" });
        setAttempt((value) => value + 1);
    }
    async function review(
        action: GrokMcpAction,
        historyId: string | null = null,
    ): Promise<void> {
        if (locked || mutation.current) return;
        const token = ++generation.current;
        const selection: GrokMcpSelection = {
            definitionId: record.id,
            expectedRevision: record.revision,
            action,
            historyId,
        };
        setStep({ kind: "previewing" });
        try {
            const plan = await previewGrokMcpChange(selection);
            if (alive.current && token === generation.current)
                setStep({ kind: "review", selection, plan });
        } catch (error: unknown) {
            if (alive.current && token === generation.current)
                setStep({ kind: "error", code: code(error) });
        }
    }
    async function confirm(): Promise<void> {
        if (
            mutation.current ||
            (step.kind !== "review" && step.kind !== "recovery")
        )
            return;
        const reviewed = step;
        mutation.current = true;
        onBusyChange(true);
        setStep({ kind: "running" });
        try {
            const result =
                reviewed.kind === "review"
                    ? await applyGrokMcpChange(
                          reviewed.selection,
                          reviewed.plan,
                      )
                    : await recoverGrokMcpChange(reviewed.recovery);
            if (alive.current) {
                setStep({ kind: "done", result });
                setRead({ kind: "loading" });
                setAttempt((value) => value + 1);
            }
        } catch (error: unknown) {
            if (alive.current) {
                setStep({ kind: "error", code: code(error) });
                setRead({ kind: "loading" });
                setAttempt((value) => value + 1);
            }
        } finally {
            mutation.current = false;
            onBusyChange(false);
            onChanged();
        }
    }
    async function loadMore(): Promise<void> {
        if (
            read.kind !== "ready" ||
            read.history.nextCursor === null ||
            locked ||
            readingMore.current ||
            mutation.current
        )
            return;
        const snapshot = read;
        const token = generation.current;
        readingMore.current = true;
        setMore(true);
        try {
            const page = await readGrokMcpHistory(
                record.id,
                snapshot.history.nextCursor,
            );
            if (alive.current && generation.current === token) {
                const seen = new Set(
                    snapshot.history.items.map((row) => row.id),
                );
                setRead({
                    ...snapshot,
                    history: {
                        items: [
                            ...snapshot.history.items,
                            ...page.items.filter((row) => !seen.has(row.id)),
                        ],
                        nextCursor: page.nextCursor,
                    },
                });
            }
        } catch (error: unknown) {
            if (alive.current && generation.current === token)
                setStep({ kind: "error", code: code(error) });
        } finally {
            readingMore.current = false;
            if (alive.current) setMore(false);
        }
    }
    const data = read.kind === "ready" ? read : null;
    const current = data?.state.deployments.find(
        (row) => row.configRoot === data.state.configRoot,
    );
    const pending = data?.state.recovery;
    const reviewing = step.kind === "review" || step.kind === "recovery";
    return {
        read,
        step,
        locked,
        more,
        current,
        pending,
        reviewing,
        review,
        confirm,
        refresh,
        loadMore,
        setStep,
        mutating: () => mutation.current,
    };
}
