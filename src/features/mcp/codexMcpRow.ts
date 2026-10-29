import type { CodexMcpDeployment } from "../../lib/desktop/codexMcp";
import { managedMcpRowLocked } from "./mcpRowLock";

type Row = Pick<CodexMcpDeployment, "state" | "nativeEnabled" | "ownership">;

/**
 * Codex keeps a user's disable inside the entry (`enabled = false`). A
 * `modified` row whose native flag is off while vibemate wrote it on means the
 * user turned it off in Codex by hand.
 */
export function codexMcpManuallyDisabled(row: Row): boolean {
    return (
        row.state === "modified" &&
        row.nativeEnabled === false &&
        row.ownership.enabled
    );
}

/** Codex form of {@link managedMcpRowLocked}. */
export function codexMcpRowLocked(row: Row | undefined): boolean {
    return managedMcpRowLocked(row);
}

/** Observed-state copy key, with the manual disable called out on its own. */
export function codexMcpStateKey(
    row: Row,
):
    | "mcp.codex.states.manuallyDisabled"
    | `mcp.codex.states.${CodexMcpDeployment["state"]}` {
    return codexMcpManuallyDisabled(row)
        ? "mcp.codex.states.manuallyDisabled"
        : `mcp.codex.states.${row.state}`;
}

/** Enabled flag as Codex currently reads it, falling back to what vibemate wrote. */
export function codexMcpEnabled(row: Row): boolean {
    return row.nativeEnabled ?? row.ownership.enabled;
}
