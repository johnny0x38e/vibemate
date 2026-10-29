import { expect, test } from "vitest";
import { codexMcpRowLocked } from "./codexMcpRow";
import { managedMcpRowLocked } from "./mcpRowLock";

test("only a row changed outside vibemate is locked", () => {
    expect(managedMcpRowLocked(undefined)).toBe(false);
    for (const state of ["written", "missing", "unreadable"]) {
        expect(managedMcpRowLocked({ state })).toBe(false);
    }
    expect(managedMcpRowLocked({ state: "modified" })).toBe(true);
});

test("Codex and Grok share the same lock rule", () => {
    const codexRow = (state: "written" | "modified") => ({
        state,
        nativeEnabled: true,
        ownership: {
            serverName: "tools",
            definitionRevision: 1,
            generation: "b".repeat(32),
            enabled: true,
        },
    });
    expect(codexMcpRowLocked(codexRow("modified"))).toBe(true);
    expect(codexMcpRowLocked(codexRow("written"))).toBe(false);
    expect(codexMcpRowLocked(undefined)).toBe(false);
});
