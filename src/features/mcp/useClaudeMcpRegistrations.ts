import { useEffect, useState } from "react";
import {
    ClaudeMcpError,
    readClaudeMcpState,
    type ClaudeMcpErrorCode,
    type ClaudeMcpState,
} from "../../lib/desktop/claudeMcp";

type RegistrationState =
    | { kind: "loading" }
    | { kind: "ready"; value: ClaudeMcpState }
    | { kind: "error"; code: ClaudeMcpErrorCode };
/** Read every root page for honest list summaries. Visibility/epoch changes
 * invalidate earlier reads; no partial page can imply an unregistered entry.
 * Errors stay separate from the central library's successful metadata read. */
export function useClaudeMcpRegistrations(
    hidden: boolean,
    epoch: number,
): RegistrationState {
    const key = `${String(hidden)}:${String(epoch)}`;
    const [answer, setAnswer] = useState<{
        key: string;
        state: RegistrationState;
    }>({ key, state: { kind: "loading" } });
    if (answer.key !== key) setAnswer({ key, state: { kind: "loading" } });
    useEffect(() => {
        if (hidden) return;
        let current = true;
        // Cleanup may invalidate this read while any IPC await is suspended.
        const isCurrent = (): boolean => current;
        async function load(): Promise<void> {
            try {
                let state = await readClaudeMcpState(null, null, 100);
                if (!isCurrent()) return;
                const rows = [...state.deployments];
                while (state.nextCursor !== null) {
                    const page = await readClaudeMcpState(
                        null,
                        state.nextCursor,
                        100,
                    );
                    if (!isCurrent()) return;
                    if (
                        page.configRoot !== state.configRoot ||
                        rows.length + page.deployments.length > 1000
                    )
                        throw new ClaudeMcpError("invalid_response");
                    rows.push(...page.deployments);
                    state = page;
                }
                if (isCurrent())
                    setAnswer({
                        key,
                        state: {
                            kind: "ready",
                            value: { ...state, deployments: rows },
                        },
                    });
            } catch (error: unknown) {
                if (isCurrent())
                    setAnswer({
                        key,
                        state: {
                            kind: "error",
                            code:
                                error instanceof ClaudeMcpError
                                    ? error.code
                                    : "outcome_unknown",
                        },
                    });
            }
        }
        void load();
        return () => {
            current = false;
        };
    }, [hidden, epoch, key]);
    return answer.key === key ? answer.state : { kind: "loading" };
}
