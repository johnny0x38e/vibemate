import { useEffect, useRef, useState } from "react";
import {
    applyPiMcpChange,
    PiMcpError,
    previewPiMcpChange,
    readPiMcpHistory,
    readPiMcpState,
    recoverPiMcpChange,
    type PiMcpAction,
    type PiMcpApplied,
    type PiMcpErrorCode,
    type PiMcpHistory,
    type PiMcpPlan,
    type PiMcpRecovery,
    type PiMcpSelection,
    type PiMcpState,
} from "../../lib/desktop/piMcp";
import type { McpRecord } from "../../lib/desktop/mcp";

export type PiMcpReadState =
    | { kind: "loading" }
    | { kind: "error"; code: PiMcpErrorCode }
    | { kind: "ready"; state: PiMcpState; history: PiMcpHistory };

export type PiMcpStep =
    | { kind: "idle" }
    | { kind: "previewing" | "running" }
    | { kind: "review"; selection: PiMcpSelection; plan: PiMcpPlan }
    | { kind: "recovery"; recovery: PiMcpRecovery }
    | { kind: "error"; code: PiMcpErrorCode }
    | { kind: "done"; result: PiMcpApplied };

function code(error: unknown): PiMcpErrorCode {
    return error instanceof PiMcpError ? error.code : "outcome_unknown";
}

/** Shared Pi registration session for Manager dialogs. */
export function usePiMcpManager(
    record: McpRecord,
    onBusyChange: (busy: boolean) => void,
    onChanged: () => void,
): {
    read: PiMcpReadState;
    step: PiMcpStep;
    locked: boolean;
    more: boolean;
    current: PiMcpState["deployments"][number] | undefined;
    pending: PiMcpRecovery | null | undefined;
    reviewing: boolean;
    review: (action: PiMcpAction, historyId?: string | null) => Promise<void>;
    confirm: () => Promise<void>;
    refresh: () => void;
    loadMore: () => Promise<void>;
    setStep: (step: PiMcpStep) => void;
    mutating: () => boolean;
} {
    const [read, setRead] = useState<PiMcpReadState>({ kind: "loading" });
    const [step, setStep] = useState<PiMcpStep>({ kind: "idle" });
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
            readPiMcpState(record.id),
            readPiMcpHistory(record.id),
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
        action: PiMcpAction,
        historyId: string | null = null,
    ): Promise<void> {
        if (locked || mutation.current) return;
        const token = ++generation.current;
        const selection: PiMcpSelection = {
            definitionId: record.id,
            expectedRevision: record.revision,
            action,
            historyId,
        };
        setStep({ kind: "previewing" });
        try {
            const plan = await previewPiMcpChange(selection);
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
                    ? await applyPiMcpChange(reviewed.selection, reviewed.plan)
                    : await recoverPiMcpChange(reviewed.recovery);
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
            const page = await readPiMcpHistory(
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
