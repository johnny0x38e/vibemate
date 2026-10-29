/**
 * Codex and Grok refuse every action on a `modified` registration row
 * (`ownership_conflict`), because rewriting the entry would drop the hand
 * edits, including keys vibemate does not manage. The managers disable those
 * actions up front and explain why; recovery stays available.
 */
export function managedMcpRowLocked(
    row: { state: string } | undefined,
): boolean {
    return row?.state === "modified";
}
