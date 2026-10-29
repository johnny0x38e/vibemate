import type { CodexSettingsState } from "../../lib/desktop/codex";
import type { ActiveAgentModel } from "../../lib/desktop/history";

/**
 * List-row summary for Codex: vibemate's pair only while both the default
 * and its provider table are still exactly what vibemate wrote. A table or
 * default the user changed (or removed) is their own configuration.
 */
export function codexActiveModel(state: CodexSettingsState): ActiveAgentModel {
    const fallback = state.default;
    if (fallback === null) return { kind: "own" };
    if (fallback.state === "unreadable") return { kind: "unknown" };
    if (fallback.state === "modified" || fallback.state === "missing")
        return { kind: "own" };
    const binding = state.providers.find(
        (row) => row.providerId === fallback.providerId,
    );
    if (binding === undefined || binding.state === "unreadable")
        return { kind: "unknown" };
    if (binding.state !== "written") return { kind: "own" };
    return {
        kind: "ours",
        providerLabel: binding.displayName.slice(0, 64),
        modelId: fallback.modelId,
        modelLabel: fallback.modelId,
    };
}
