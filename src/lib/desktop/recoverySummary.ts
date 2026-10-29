import { isTauri } from "@tauri-apps/api/core";
import type { ConfigurableAgent } from "./agents";
import { readClaudeMcpState } from "./claudeMcp";
import { readGrokMcpState } from "./grokMcp";
import { readConfigurationHistory, type HistoryEntry } from "./history";
import { readPiMcpState } from "./piMcp";
import { listSkillDeployRecoveries } from "./skills";

/** Sidebar module that owns resolving this interruption. */
export type RecoveryDestination = "agents" | "skills" | "mcp";

/** Which journal family produced the row. */
export type RecoveryDomain = "agentConfig" | "skillDeploy" | "mcp";

/**
 * One unfinished write discovered across Agent config, Skill deploy, or MCP.
 * The hub only discovers and navigates; it never replays a write.
 */
export interface RecoverySummaryItem {
    id: string;
    domain: RecoveryDomain;
    destination: RecoveryDestination;
    /** Configurable Agent wire id when the domain is Agent or MCP. */
    agentId: "claudeCode" | "pi" | "grokBuild" | null;
    /** Short subject: skill name, MCP server, or operation intent. */
    subject: string;
    /** Classification / observation / path for the list line. */
    detail: string;
}

const AGENTS: ConfigurableAgent[] = ["claudeCode", "pi", "grokBuild"];

function agentEntries(
    agent: ConfigurableAgent,
    currentEntry: HistoryEntry | null,
    entries: HistoryEntry[],
): RecoverySummaryItem[] {
    const seen = new Set<string>();
    const out: RecoverySummaryItem[] = [];
    for (const entry of [
        ...(currentEntry !== null ? [currentEntry] : []),
        ...entries,
    ]) {
        if (!entry.requiresResolution || seen.has(entry.entryKey)) continue;
        seen.add(entry.entryKey);
        out.push({
            id: `agent:${agent}:${entry.entryKey}`,
            domain: "agentConfig",
            destination: "agents",
            agentId: agent,
            subject: entry.intent,
            detail: entry.ownerLabel ?? entry.operationId,
        });
    }
    return out;
}

/**
 * Collect unfinished operations from existing read APIs. Browser preview reports
 * honesty without inventing rows. Individual domain failures are skipped so one
 * unavailable module does not hide the others.
 */
export async function listRecoverySummary(): Promise<
    { kind: "preview" } | { kind: "desktop"; items: RecoverySummaryItem[] }
> {
    if (!isTauri()) return { kind: "preview" };

    const items: RecoverySummaryItem[] = [];

    for (const agent of AGENTS) {
        try {
            const result = await readConfigurationHistory(agent, null, 20);
            if (result.kind === "preview") return { kind: "preview" };
            items.push(
                ...agentEntries(
                    agent,
                    result.page.currentEntry,
                    result.page.entries,
                ),
            );
        } catch {
            // Leave Agent rows empty when history is unavailable.
        }
    }

    try {
        const skills = await listSkillDeployRecoveries();
        if (skills.kind === "preview") return { kind: "preview" };
        for (const row of skills.items) {
            items.push({
                id: `skill:${row.id}`,
                domain: "skillDeploy",
                destination: "skills",
                agentId: row.agentId,
                subject: row.skillName,
                detail: `${row.kind}:${row.classification}`,
            });
        }
    } catch {
        // Skill journal unavailable.
    }

    try {
        const pi = await readPiMcpState();
        if (pi.recovery !== null) {
            items.push({
                id: `mcp:pi:${pi.recovery.id}`,
                domain: "mcp",
                destination: "mcp",
                agentId: "pi",
                subject: pi.recovery.serverName,
                detail: `${pi.recovery.action}:${pi.recovery.observation}`,
            });
        }
    } catch {
        // Pi MCP root unavailable.
    }

    try {
        const grok = await readGrokMcpState();
        if (grok.recovery !== null) {
            items.push({
                id: `mcp:grok:${grok.recovery.id}`,
                domain: "mcp",
                destination: "mcp",
                agentId: "grokBuild",
                subject: grok.recovery.serverName,
                detail: `${grok.recovery.action}:${grok.recovery.observation}`,
            });
        }
    } catch {
        // Grok MCP root unavailable.
    }

    try {
        const claude = await readClaudeMcpState();
        if (claude.recovery !== null) {
            items.push({
                id: `mcp:claude:${claude.recovery.id}`,
                domain: "mcp",
                destination: "mcp",
                agentId: "claudeCode",
                subject: claude.recovery.serverName,
                detail: `${claude.recovery.action}:${claude.recovery.observation}`,
            });
        }
    } catch {
        // Claude Code user-scope MCP target unavailable.
    }

    return { kind: "desktop", items };
}
