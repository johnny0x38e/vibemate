import { invoke, isTauri } from "@tauri-apps/api/core";
import { beforeEach, expect, test, vi } from "vitest";
import { GrokMcpError, readGrokMcpState } from "./grokMcp";
import {
    emptyGrokMcpState,
    grokMcpDeployment,
} from "../../test/grokMcpFixtures";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: vi.fn() }));
const call = vi.mocked(invoke);
beforeEach(() => {
    call.mockReset();
    vi.mocked(isTauri).mockReturnValue(true);
});
async function failure(operation: () => Promise<unknown>): Promise<string> {
    try {
        await operation();
    } catch (error: unknown) {
        if (error instanceof GrokMcpError) return error.code;
    }
    return "no error";
}

test("hand-added keys arrive as names on a modified row only", async () => {
    const modified = {
        ...grokMcpDeployment,
        state: "modified",
        unmanagedKeys: ["startup_timeout_sec"],
    };
    call.mockResolvedValueOnce({
        ...emptyGrokMcpState,
        deployments: [modified],
    });
    expect((await readGrokMcpState()).deployments).toEqual([modified]);
    for (const deployments of [
        [{ ...grokMcpDeployment, unmanagedKeys: ["startup_timeout_sec"] }],
        [{ ...grokMcpDeployment, unmanagedKeys: undefined }],
        [{ ...grokMcpDeployment, unmanagedKeys: [""] }],
    ]) {
        call.mockResolvedValueOnce({ ...emptyGrokMcpState, deployments });
        expect(await failure(() => readGrokMcpState())).not.toBe("no error");
    }
});
