import {
    cleanup,
    fireEvent,
    render,
    screen,
    waitFor,
} from "@testing-library/react";
import { StrictMode } from "react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createAppI18n } from "../../i18n";
import {
    dismissSkillDeployRecovery,
    finishSkillDeployRecovery,
    listSkillDeployRecoveries,
    SkillRequestError,
    type SkillDeployRecoveryItem,
} from "../../lib/desktop/skills";
import { SkillRecoveryPanel } from "./SkillRecoveryPanel";

vi.mock(import("../../lib/desktop/skills"), async (importOriginal) => ({
    ...(await importOriginal()),
    listSkillDeployRecoveries: vi.fn(),
    finishSkillDeployRecovery: vi.fn(),
    dismissSkillDeployRecovery: vi.fn(),
}));

const list = vi.mocked(listSkillDeployRecoveries);
const finish = vi.mocked(finishSkillDeployRecovery);
const dismiss = vi.mocked(dismissSkillDeployRecovery);

const ID = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const item: SkillDeployRecoveryItem = {
    id: ID,
    kind: "apply",
    state: "prepared",
    classification: "needs_record",
    skillId: "0123456789abcdef0123456789abcdef",
    skillName: "demo-skill",
    agentId: "pi",
    configRoot: "/tmp/pi",
    targetPath: "/tmp/pi/skills/demo-skill",
    mode: "symlink",
    canFinish: true,
    canDismiss: false,
};

async function renderPanel(): Promise<void> {
    const i18n = await createAppI18n("en");
    render(
        <StrictMode>
            <I18nextProvider i18n={i18n}>
                <SkillRecoveryPanel />
            </I18nextProvider>
        </StrictMode>,
    );
}

beforeEach(() => {
    list.mockReset();
    finish.mockReset();
    dismiss.mockReset();
});

afterEach(() => {
    cleanup();
});

test("stays hidden when there are no unfinished rows", async () => {
    list.mockResolvedValue({ kind: "desktop", items: [] });
    await renderPanel();
    await waitFor(() => {
        expect(list).toHaveBeenCalled();
    });
    expect(
        screen.queryByRole("region", {
            name: "Unfinished skill deployments",
        }),
    ).toBeNull();
});

test("finish records ownership repair and reloads", async () => {
    list.mockResolvedValue({ kind: "desktop", items: [item] });
    finish.mockImplementation(() => {
        list.mockResolvedValue({ kind: "desktop", items: [] });
        return Promise.resolve({
            id: ID,
            classification: "needs_record",
        });
    });
    await renderPanel();
    expect(await screen.findByRole("button", { name: "Finish" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
        expect(finish).toHaveBeenCalledWith({
            id: ID,
            expectedClassification: "needs_record",
        });
    });
    await waitFor(() => {
        expect(
            screen.queryByRole("region", {
                name: "Unfinished skill deployments",
            }),
        ).toBeNull();
    });
});

test("dismiss clears a conflict without rewriting files", async () => {
    const conflict: SkillDeployRecoveryItem = {
        ...item,
        classification: "conflict",
        canFinish: false,
        canDismiss: true,
    };
    list.mockResolvedValue({ kind: "desktop", items: [conflict] });
    dismiss.mockImplementation(() => {
        list.mockResolvedValue({ kind: "desktop", items: [] });
        return Promise.resolve({
            id: ID,
            classification: "conflict",
        });
    });
    await renderPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Dismiss" }));
    await waitFor(() => {
        expect(dismiss).toHaveBeenCalledWith({
            id: ID,
            expectedClassification: "conflict",
        });
    });
});

test("shows a local error and keeps the row when finish is refused", async () => {
    list.mockResolvedValue({ kind: "desktop", items: [item] });
    finish.mockRejectedValue(new SkillRequestError("recovery_conflict"));
    await renderPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Finish" }));
    expect((await screen.findByRole("alert")).textContent).toMatch(
        /not allowed for the current state/i,
    );
    expect(screen.getByRole("button", { name: "Finish" })).toBeTruthy();
});

test("reloads when the classification changed under the review", async () => {
    list.mockResolvedValue({ kind: "desktop", items: [item] });
    finish.mockImplementation(() => {
        list.mockResolvedValue({
            kind: "desktop",
            items: [
                {
                    ...item,
                    classification: "conflict",
                    canFinish: false,
                    canDismiss: true,
                },
            ],
        });
        return Promise.reject(new SkillRequestError("target_changed"));
    });
    await renderPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Finish" }));
    expect(await screen.findByRole("button", { name: "Dismiss" })).toBeTruthy();
});
