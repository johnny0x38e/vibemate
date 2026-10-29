import { cleanup, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, expect, test } from "vitest";
import { createAppI18n } from "../../i18n";
import { codexMcpPlan } from "../../test/codexFixtures";
import { CodexMcpReview } from "./CodexMcpReview";

afterEach(cleanup);

async function mount(row?: { unmanagedKeys: string[] }) {
    const i18n = await createAppI18n("en");
    render(
        <I18nextProvider i18n={i18n}>
            <CodexMcpReview plan={codexMcpPlan} row={row} />
        </I18nextProvider>,
    );
}

test("unmanaged keys come from the observed registration row", async () => {
    await mount({ unmanagedKeys: ["enabled_tools", "tool_timeout_sec"] });
    expect(
        screen.getByText(
            "Keys vibemate does not manage are kept as they are: enabled_tools, tool_timeout_sec.",
        ),
    ).toBeDefined();
});

test("no row or an empty list shows no unmanaged-keys notice", async () => {
    await mount();
    expect(screen.queryByText(/does not manage/u)).toBeNull();
    cleanup();
    await mount({ unmanagedKeys: [] });
    expect(screen.queryByText(/does not manage/u)).toBeNull();
});
