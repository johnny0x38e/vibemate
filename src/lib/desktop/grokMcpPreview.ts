import { invoke, isTauri } from "@tauri-apps/api/core";
import * as z from "zod";
import { isSafePath } from "./agents";
import { MCP_SECRET_PLACEHOLDER } from "./mcpPreview";

const CODES = [
    "invalid_request",
    "definition_unavailable",
    "revision_conflict",
    "target_unavailable",
    "invalid_config",
    "name_conflict",
    "unsupported_server_name",
    "dynamic_value_unsupported",
    "path_semantics_unverified",
    "operation_failed",
] as const;
/** Preview failures contain stable codes, never native values or diagnostics. */
export type GrokMcpPreviewErrorCode =
    (typeof CODES)[number] | "desktop_required" | "invalid_response";
/** Discards original IPC/schema errors so they cannot reach the rendered UI. */
export class GrokMcpPreviewError extends Error {
    readonly code: GrokMcpPreviewErrorCode;
    constructor(code: GrokMcpPreviewErrorCode) {
        super("Grok MCP preview failed.");
        this.name = "GrokMcpPreviewError";
        this.code = code;
    }
}
const identity = z.string().regex(/^[0-9a-f]{32}$/u);
const revision = z.int().min(1);
const selection = z.strictObject({
    definitionId: identity,
    expectedRevision: revision,
});
const text = z
    .string()
    .max(2048)
    .refine(
        (value) =>
            !Array.from(value).some((character) => {
                const code = character.charCodeAt(0);
                return code < 32 || (code >= 127 && code <= 159);
            }),
    );
const literal = text.refine(
    (value) => !value.includes("${") && !value.startsWith("~/"),
);
const protectedValues = (kind: "env" | "headers") =>
    z
        .record(
            z
                .string()
                .max(128)
                .regex(
                    kind === "env"
                        ? /^[A-Za-z_][A-Za-z0-9_]*$/u
                        : /^[!#$%&'*+.^_`|~a-z0-9-]+$/u,
                ),
            z.literal(MCP_SECRET_PLACEHOLDER),
        )
        .refine((values) => Object.keys(values).length <= 32);
const entry = z.union([
    z.strictObject({
        command: literal.min(1),
        args: z.array(literal).max(64),
        cwd: literal.refine(isSafePath).optional(),
        env: protectedValues("env"),
        enabled: z.boolean(),
    }),
    z.strictObject({
        url: text.min(1).refine((value) => {
            try {
                const url = new URL(value);
                return (
                    ["https:", "http:"].includes(url.protocol) &&
                    url.username === "" &&
                    url.password === "" &&
                    url.search === "" &&
                    url.hash === ""
                );
            } catch {
                return false;
            }
        }),
        headers: protectedValues("headers"),
        enabled: z.boolean(),
    }),
]);
const schema = z.strictObject({
    target: z
        .strictObject({
            path: z
                .string()
                .refine(isSafePath)
                .refine((value) => /[\\/]config\.toml$/u.test(value)),
            exists: z.boolean(),
            contentDigest: z
                .string()
                .regex(/^[0-9a-f]{64}$/u)
                .nullable(),
            symlink: z.boolean(),
        })
        .refine(
            (value) =>
                value.exists === (value.contentDigest !== null) &&
                (!value.symlink || value.exists),
        ),
    proposal: z.strictObject({
        definitionId: identity,
        revision,
        serverName: z
            .string()
            .max(64)
            .regex(/^[A-Za-z_][A-Za-z0-9_-]*$/u)
            .refine((value) => !value.includes("__") && !value.endsWith("_")),
        redactedEntry: entry,
        disabledByUserList: z.boolean(),
    }),
});
/** Read-only Grok proposal. Details describe a TOML entry, not a complete file. */
export type GrokMcpPreview = z.infer<typeof schema>;

/** Read the Rust-selected user target; never sends paths, values or write approvals.
 * Browser preview is explicitly unsupported. Responses must be exact and masked. */
export async function previewGrokMcpRegistration(
    definitionId: string,
    expectedRevision: number,
): Promise<GrokMcpPreview> {
    if (!isTauri()) throw new GrokMcpPreviewError("desktop_required");
    const request = selection.safeParse({ definitionId, expectedRevision });
    if (!request.success) throw new GrokMcpPreviewError("invalid_request");
    let response: unknown;
    try {
        response = await invoke<unknown>("preview_grok_mcp_registration", {
            request: request.data,
        });
    } catch (error: unknown) {
        throw new GrokMcpPreviewError(
            CODES.find((code) => code === error) ?? "operation_failed",
        );
    }
    const result = schema.safeParse(response);
    if (
        !result.success ||
        result.data.proposal.definitionId !== definitionId ||
        result.data.proposal.revision !== expectedRevision
    )
        throw new GrokMcpPreviewError("invalid_response");
    return result.data;
}
