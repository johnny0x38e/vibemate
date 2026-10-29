import { expect, test } from "vitest";
import type { CodexSettingsNotice } from "../../lib/desktop/codex";
import en from "../../locales/en.json";
import zhCN from "../../locales/zh-CN.json";

// Every notice copy key must match a notice the backend can still send.
const notices: readonly CodexSettingsNotice[] = [
    "plaintext_credential",
    "profile_overrides_default",
    "release_only",
];

test("Codex notice copy has no keys the backend no longer sends", () => {
    for (const locale of [en, zhCN]) {
        const keys = Object.keys(locale.agents.codex.notices).sort();
        expect(keys).toEqual([...notices].sort());
        expect(keys).not.toContain("reasoning_effort_cleared");
    }
});
