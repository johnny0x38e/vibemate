import { expect, test } from "vitest";
import { codexSettingsState } from "../../test/codexFixtures";
import { codexActiveModel } from "./codexSummary";

const fallback = codexSettingsState.default;
if (fallback === null) throw new Error("fixture default required");

test("a written default and table are vibemate's provider · model", () => {
    expect(codexActiveModel(codexSettingsState)).toEqual({
        kind: "ours",
        providerLabel: "Deep Seek",
        modelId: "deepseek-flash",
        modelLabel: "deepseek-flash",
    });
});

test.each(["modified", "missing"] as const)(
    "a %s default is the user's own configuration",
    (state) => {
        expect(
            codexActiveModel({
                ...codexSettingsState,
                default: { ...fallback, state },
            }),
        ).toEqual({ kind: "own" });
    },
);

test("no default is own and an unreadable default is unknown", () => {
    expect(codexActiveModel({ ...codexSettingsState, default: null })).toEqual({
        kind: "own",
    });
    expect(
        codexActiveModel({
            ...codexSettingsState,
            default: { ...fallback, state: "unreadable" },
        }),
    ).toEqual({ kind: "unknown" });
});
