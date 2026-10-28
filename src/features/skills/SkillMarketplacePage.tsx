import { Dialog } from "@base-ui/react/dialog";
import { useEffect, useId, useMemo, useRef, useState, type JSX } from "react";
import { useTranslation } from "react-i18next";
import { FieldSelect } from "../../components/FieldSelect";
import { Icon } from "../../components/Icon";
import { SecondaryPageActions } from "../../components/SecondaryPageActions";
import hierarchy from "../../components/textHierarchy.module.css";
import { ProviderPage } from "../providers/ProviderPage";
import {
    getMarketplaceSkillDetail,
    installMarketplaceSkill,
    listMarketplaceSkills,
    previewMarketplaceInstall,
    searchMarketplaceSkills,
    SkillRequestError,
    type MarketplaceBoard,
    type MarketplaceSkill,
    type SkillErrorCode,
    type SkillInstallPreview,
    type SkillInstallReport,
} from "../../lib/desktop/skills";
import buttons from "../../components/actionButtons.module.css";
import editForm from "../../components/editForm.module.css";
import dialog from "./SkillDeleteDialog.module.css";
import styles from "./SkillMarketplacePage.module.css";

/** Cards per page for the catalog grid (client-side slice of the loaded board). */
export const MARKETPLACE_PAGE_SIZE = 12;

/** Max concurrent get_marketplace_skill_detail calls for the visible page. */
export const MARKETPLACE_DETAIL_PREFETCH_CONCURRENCY = 3;

type CatalogState =
    | { kind: "idle" | "loading" }
    | { kind: "ready"; skills: MarketplaceSkill[] }
    | { kind: "preview" }
    | { kind: "error"; code: SkillErrorCode };

type InstallState =
    | { kind: "idle" | "reviewing" | "installing" }
    | { kind: "reviewed"; preview: SkillInstallPreview }
    | { kind: "installed"; report: SkillInstallReport }
    | { kind: "error"; code: SkillErrorCode; uncertain: boolean };

/** Lazy skills.sh page description shown in the detail dialog body. */
type DetailCopyState =
    | { kind: "idle" | "loading" | "preview" }
    | { kind: "ready"; text: string }
    | { kind: "error"; code: SkillErrorCode };

/** Card summary clamp; CSS line-clamp still hides overflow visually. */
const CARD_SUMMARY_MAX = 120;

/** Truncate a catalog description for the card; never invents missing text. */
function truncateCardSummary(text: string): string {
    const trimmed = text.trim();
    if (trimmed.length <= CARD_SUMMARY_MAX) return trimmed;
    return `${trimmed.slice(0, CARD_SUMMARY_MAX - 1)}…`;
}

/** Props for {@link SkillMarketplacePage}. */
export interface SkillMarketplacePageProps {
    /** Return to the skills list; the library restores focus there. */
    onCancel: () => void;
    /** Called once after a successful install so the list can be re-read. */
    onInstalled: (report: SkillInstallReport) => void;
    /**
     * Protect sidebar / home navigation while a review or install is pending,
     * or while an uncertain outcome is still on screen.
     */
    onNavigationBlockedChange?: (blocked: boolean) => void;
}

function errorCode(error: unknown): SkillErrorCode {
    return error instanceof SkillRequestError ? error.code : "operation_failed";
}

/** Failures that may have changed something beyond this window's knowledge. */
function uncertain(code: SkillErrorCode): boolean {
    return ["operation_failed", "invalid_response", "outcome_unknown"].includes(
        code,
    );
}

/**
 * Browse and install skills from the skills.sh catalog on a secondary page.
 *
 * Leaderboards and search only return GitHub-installable rows. Review shows the
 * exact Git HEAD command; Install is the only action that contacts GitHub and
 * writes the library. Non-GitHub catalog hosts are refused by Rust.
 *
 * The catalog is shown as a fixed-size card grid with client-side paging. Board
 * changes and search both reset to page 1 so a stale page index cannot hide new
 * results. List/search rows leave description null. After the current page is
 * visible, a background batch lazy-loads `get_marketplace_skill_detail` for those
 * cards (limited concurrency) so truncated summaries appear without opening the
 * dialog. Page/board/search changes cancel or ignore stale prefetch results.
 * Opening a card still loads detail when the summary is not ready yet. Failures
 * never invent description text. The IPC type only carries description (plus
 * id/source/skillId/name/installs); skills.sh HTML also has an install command
 * and a longer Summary/SKILL.md body that Rust does not return yet.
 * Closing returns to this page without losing the selection or the
 * Review→Install footer flow.
 *
 * Back is blocked while a review/install mutation is in flight and while an
 * uncertain install result is shown, so the outcome cannot disappear behind the
 * list. Catalog loading does not block Back.
 */
export function SkillMarketplacePage({
    onCancel,
    onInstalled,
    onNavigationBlockedChange,
}: SkillMarketplacePageProps): JSX.Element {
    const { t } = useTranslation();
    const prefix = useId();
    const [board, setBoard] = useState<MarketplaceBoard>("all_time");
    const [query, setQuery] = useState("");
    const [selected, setSelected] = useState<MarketplaceSkill | null>(null);
    // Catalog card detail overlay; close returns to the marketplace page.
    const [detailOpen, setDetailOpen] = useState(false);
    // Lazy description for the open detail dialog; never invents text on failure.
    const [detailCopy, setDetailCopy] = useState<DetailCopyState>({
        kind: "idle",
    });
    const [catalog, setCatalog] = useState<CatalogState>({ kind: "loading" });
    const [install, setInstall] = useState<InstallState>({ kind: "idle" });
    // 1-based page index into the ready catalog; reset whenever the catalog reloads.
    const [page, setPage] = useState(1);
    const catalogLoading = catalog.kind === "loading";
    const mutationBusy =
        install.kind === "reviewing" || install.kind === "installing";
    // Toolbar stays blocked while the catalog loads; Back must not, or a hung
    // catalog request would trap the user on this page.
    const busy = catalogLoading || mutationBusy;
    // Uncertain outcomes must stay visible; Back and module home both stay put.
    const navBlocked =
        mutationBusy || (install.kind === "error" && install.uncertain);
    const mutationPending = useRef(false);
    const active = useRef(true);
    const loadGeneration = useRef(0);
    // Ignore stale detail responses when the user opens another card quickly.
    const detailGeneration = useRef(0);
    // Ignore stale background card-summary prefetch when the page/board/search changes.
    const prefetchGeneration = useRef(0);
    // Skill ids on the current page whose description prefetch is still in flight.
    const [prefetchPending, setPrefetchPending] = useState<ReadonlySet<string>>(
        () => new Set(),
    );
    const searchInput = useRef<HTMLInputElement>(null);
    const confirmButton = useRef<HTMLButtonElement>(null);
    const doneButton = useRef<HTMLButtonElement>(null);
    const listPane = useRef<HTMLDivElement>(null);

    const pageCount =
        catalog.kind === "ready"
            ? Math.max(
                  1,
                  Math.ceil(catalog.skills.length / MARKETPLACE_PAGE_SIZE),
              )
            : 1;
    const safePage = Math.min(page, pageCount);
    const pageSkills = useMemo(() => {
        if (catalog.kind !== "ready") return [];
        const start = (safePage - 1) * MARKETPLACE_PAGE_SIZE;
        return catalog.skills.slice(start, start + MARKETPLACE_PAGE_SIZE);
    }, [catalog, safePage]);

    // Stable identity for the visible page rows (ids only). Merging descriptions
    // must not restart prefetch; paging / board / search changes must.
    const pageIdentity = useMemo(() => {
        if (catalog.kind !== "ready") return "";
        return pageSkills.map((row) => row.id).join("\0");
    }, [catalog.kind, pageSkills]);
    // Prefetch effect reads the latest page slice without re-running on merges.
    const pageSkillsRef = useRef(pageSkills);
    useEffect(() => {
        pageSkillsRef.current = pageSkills;
    }, [pageSkills]);

    useEffect(() => {
        active.current = true;
        return () => {
            active.current = false;
        };
    }, []);

    useEffect(() => {
        onNavigationBlockedChange?.(navBlocked);
        return () => {
            onNavigationBlockedChange?.(false);
        };
    }, [navBlocked, onNavigationBlockedChange]);

    useEffect(() => {
        if (install.kind === "reviewed") confirmButton.current?.focus();
        if (install.kind === "installed") doneButton.current?.focus();
    }, [install.kind]);

    /**
     * Background-prefetch descriptions for the visible page only. List/search
     * leave description null; this fills card summaries ASAP without waiting for
     * the detail Dialog. Limited concurrency; stale page results are ignored.
     * Deferred with setTimeout so the effect body does not setState synchronously
     * (react-hooks/set-state-in-effect).
     */
    useEffect(() => {
        const generation = ++prefetchGeneration.current;
        let cancelled = false;

        const timer = window.setTimeout(() => {
            if (cancelled || generation !== prefetchGeneration.current) return;

            if (catalog.kind !== "ready" || pageIdentity === "") {
                setPrefetchPending(new Set());
                return;
            }

            // Snapshot the rows that still need a description on this page.
            const targets = pageSkillsRef.current.filter(
                (row) => row.description === null,
            );
            if (targets.length === 0) {
                setPrefetchPending(new Set());
                return;
            }
            setPrefetchPending(new Set(targets.map((row) => row.id)));
            let nextIndex = 0;

            function mergeDetail(
                skillId: string,
                detail: MarketplaceSkill,
            ): void {
                setSelected((current) =>
                    current?.id === skillId
                        ? { ...current, ...detail }
                        : current,
                );
                setCatalog((current) => {
                    if (current.kind !== "ready") return current;
                    return {
                        kind: "ready",
                        skills: current.skills.map((skill) =>
                            skill.id === skillId
                                ? { ...skill, ...detail }
                                : skill,
                        ),
                    };
                });
            }

            function clearPending(skillId: string): void {
                if (cancelled || generation !== prefetchGeneration.current)
                    return;
                setPrefetchPending((current) => {
                    if (!current.has(skillId)) return current;
                    const next = new Set(current);
                    next.delete(skillId);
                    return next;
                });
            }

            async function worker(): Promise<void> {
                while (
                    !cancelled &&
                    generation === prefetchGeneration.current
                ) {
                    const index = nextIndex;
                    nextIndex += 1;
                    if (index >= targets.length) return;
                    const row = targets[index];
                    if (row === undefined) return;
                    try {
                        const result = await getMarketplaceSkillDetail(
                            row.source,
                            row.skillId,
                        );
                        // Cleanup bumps prefetchGeneration; that alone drops stale pages.
                        if (
                            generation !== prefetchGeneration.current ||
                            !active.current
                        ) {
                            return;
                        }
                        if (result.kind === "desktop") {
                            const text =
                                result.skill.summary ??
                                result.skill.description;
                            // Never invent copy; skip empty/missing detail payloads.
                            if (typeof text === "string" && text.trim() !== "") {
                                mergeDetail(row.id, result.skill);
                            }
                        }
                        // preview / null: leave description unset; never invent copy.
                    } catch {
                        // Prefetch failure stays silent on the card; Dialog can retry.
                    } finally {
                        clearPending(row.id);
                    }
                }
            }

            const workerCount = Math.min(
                MARKETPLACE_DETAIL_PREFETCH_CONCURRENCY,
                targets.length,
            );
            const workers: Promise<void>[] = [];
            for (let i = 0; i < workerCount; i += 1) {
                workers.push(worker());
            }
            void Promise.all(workers);
        }, 0);

        return () => {
            cancelled = true;
            window.clearTimeout(timer);
            // Drop in-flight results from a previous page/board/search.
            if (prefetchGeneration.current === generation) {
                prefetchGeneration.current += 1;
            }
        };
    }, [catalog.kind, pageIdentity]);

    // Mount loads the all-time leaderboard. A running installer is not cancelled
    // by leaving the page when Back is blocked; its result is still reported.
    useEffect(() => {
        const generation = ++loadGeneration.current;
        const timer = window.setTimeout(() => {
            setCatalog({ kind: "loading" });
            setSelected(null);
            setDetailOpen(false);
            setDetailCopy({ kind: "idle" });
            setInstall({ kind: "idle" });
            setPage(1);
            void listMarketplaceSkills("all_time")
                .then((result) => {
                    if (
                        !active.current ||
                        generation !== loadGeneration.current
                    )
                        return;
                    setCatalog(
                        result.kind === "preview"
                            ? { kind: "preview" }
                            : { kind: "ready", skills: result.skills },
                    );
                })
                .catch((error: unknown) => {
                    if (
                        active.current &&
                        generation === loadGeneration.current
                    ) {
                        setCatalog({ kind: "error", code: errorCode(error) });
                    }
                });
        }, 0);
        return () => {
            clearTimeout(timer);
        };
    }, []);

    async function loadBoard(next: MarketplaceBoard): Promise<void> {
        const generation = ++loadGeneration.current;
        setCatalog({ kind: "loading" });
        setSelected(null);
        setDetailOpen(false);
        setDetailCopy({ kind: "idle" });
        setInstall({ kind: "idle" });
        setPage(1);
        try {
            const result = await listMarketplaceSkills(next);
            if (!active.current || generation !== loadGeneration.current)
                return;
            setCatalog(
                result.kind === "preview"
                    ? { kind: "preview" }
                    : { kind: "ready", skills: result.skills },
            );
        } catch (error: unknown) {
            if (active.current && generation === loadGeneration.current) {
                setCatalog({ kind: "error", code: errorCode(error) });
            }
        }
    }

    async function search(): Promise<void> {
        const trimmed = query.trim();
        if (busy || mutationPending.current || trimmed.length < 2) return;
        const generation = ++loadGeneration.current;
        setCatalog({ kind: "loading" });
        setSelected(null);
        setDetailOpen(false);
        setDetailCopy({ kind: "idle" });
        setInstall({ kind: "idle" });
        setPage(1);
        try {
            const result = await searchMarketplaceSkills(trimmed);
            if (!active.current || generation !== loadGeneration.current)
                return;
            setCatalog(
                result.kind === "preview"
                    ? { kind: "preview" }
                    : { kind: "ready", skills: result.skills },
            );
        } catch (error: unknown) {
            if (active.current && generation === loadGeneration.current) {
                setCatalog({ kind: "error", code: errorCode(error) });
            }
        }
    }

    /** Prefer the longer Summary when present; fall back to meta description. */
    function detailBodyText(skill: MarketplaceSkill): string | null {
        return skill.summary ?? skill.description;
    }

    /**
     * Load skills.sh page detail for one catalog row. List/search leave page
     * fields null; a prior successful open may already have filled them in.
     * Failures keep an honest error state and never invent body text.
     */
    async function loadDetail(row: MarketplaceSkill): Promise<void> {
        const cached = detailBodyText(row);
        if (cached !== null) {
            setDetailCopy({ kind: "ready", text: cached });
            return;
        }
        const generation = ++detailGeneration.current;
        setDetailCopy({ kind: "loading" });
        try {
            const result = await getMarketplaceSkillDetail(
                row.source,
                row.skillId,
            );
            if (!active.current || generation !== detailGeneration.current)
                return;
            if (result.kind === "preview") {
                setDetailCopy({ kind: "preview" });
                return;
            }
            const text = detailBodyText(result.skill);
            // Desktop wrapper already rejects null description; treat a missing
            // body as a contract break.
            if (text === null) {
                setDetailCopy({ kind: "error", code: "invalid_response" });
                return;
            }
            setDetailCopy({ kind: "ready", text });
            // Merge enriched fields into selection + catalog for reuse.
            setSelected((current) =>
                current?.id === row.id ? { ...current, ...result.skill } : current,
            );
            setCatalog((current) => {
                if (current.kind !== "ready") return current;
                return {
                    kind: "ready",
                    skills: current.skills.map((skill) =>
                        skill.id === row.id ? { ...skill, ...result.skill } : skill,
                    ),
                };
            });
        } catch (error: unknown) {
            if (active.current && generation === detailGeneration.current) {
                setDetailCopy({ kind: "error", code: errorCode(error) });
            }
        }
    }

    function choose(row: MarketplaceSkill): void {
        if (busy || mutationPending.current) return;
        setSelected(row);
        setInstall({ kind: "idle" });
        setDetailOpen(true);
        const cached = detailBodyText(row);
        if (cached !== null) {
            setDetailCopy({ kind: "ready", text: cached });
        } else {
            setDetailCopy({ kind: "loading" });
            void loadDetail(row);
        }
    }

    /** Dismiss the detail overlay; selection and footer Review stay available. */
    function closeDetail(): void {
        if (mutationBusy || mutationPending.current) return;
        if (install.kind === "error" && install.uncertain) return;
        setDetailOpen(false);
        setDetailCopy({ kind: "idle" });
    }

    function goToPage(next: number): void {
        if (busy || mutationPending.current) return;
        if (next < 1 || next > pageCount || next === safePage) return;
        setPage(next);
        // Keep the new page's first cards in view after paging (jsdom has no scrollTo).
        const pane = listPane.current;
        if (pane !== null && typeof pane.scrollTo === "function") {
            pane.scrollTo({ top: 0 });
        } else if (pane !== null) {
            pane.scrollTop = 0;
        }
    }

    async function review(): Promise<void> {
        if (busy || mutationPending.current || selected === null) return;
        mutationPending.current = true;
        setInstall({ kind: "reviewing" });
        try {
            const result = await previewMarketplaceInstall({
                source: selected.source,
                skillId: selected.skillId,
            });
            if (!active.current) return;
            setInstall(
                result.kind === "preview"
                    ? {
                          kind: "error",
                          code: "desktop_required",
                          uncertain: false,
                      }
                    : { kind: "reviewed", preview: result.preview },
            );
        } catch (error: unknown) {
            if (active.current) {
                const code = errorCode(error);
                setInstall({ kind: "error", code, uncertain: uncertain(code) });
            }
        } finally {
            mutationPending.current = false;
        }
    }

    async function installSelected(): Promise<void> {
        if (mutationPending.current || selected === null) return;
        mutationPending.current = true;
        // Close the modal first so Back and the footer stay reachable mid-install.
        setDetailOpen(false);
        setInstall({ kind: "installing" });
        try {
            const report = await installMarketplaceSkill({
                source: selected.source,
                skillId: selected.skillId,
            });
            if (!active.current) return;
            setInstall({ kind: "installed", report });
            onInstalled(report);
        } catch (error: unknown) {
            if (active.current) {
                const code = errorCode(error);
                setInstall({ kind: "error", code, uncertain: uncertain(code) });
            }
        } finally {
            mutationPending.current = false;
        }
    }

    function leave(): void {
        if (navBlocked || mutationPending.current) return;
        onCancel();
    }

    const boardOptions = [
        { value: "all_time", label: t("skills.marketplace.boardAllTime") },
        { value: "trending", label: t("skills.marketplace.boardTrending") },
        { value: "hot", label: t("skills.marketplace.boardHot") },
    ];

    return (
        <ProviderPage
            title={t("skills.marketplace.title")}
            onBack={leave}
            backBlocked={navBlocked}
            backAriaLabel={t("skills.backLabel")}
        >
            <div className={styles["layout"]}>
                {install.kind === "installed" ? (
                    <>
                        <InstallResult report={install.report} />
                        <SecondaryPageActions>
                            <div className={styles["headerActions"]}>
                                <button
                                    ref={doneButton}
                                    type="button"
                                    className={buttons["primary"]}
                                    onClick={() => {
                                        leave();
                                    }}
                                >
                                    {t("skills.marketplace.done")}
                                </button>
                            </div>
                        </SecondaryPageActions>
                    </>
                ) : (
                    <>
                        <p className={styles["description"]}>
                            {t("skills.marketplace.description")}
                        </p>

                        <div className={styles["toolbar"]}>
                            <label
                                className={styles["label"]}
                                htmlFor={`${prefix}-board`}
                            >
                                {t("skills.marketplace.board")}
                            </label>
                            <FieldSelect
                                id={`${prefix}-board`}
                                value={board}
                                options={boardOptions}
                                blocked={busy}
                                size="fill"
                                onChange={(value) => {
                                    const next: MarketplaceBoard =
                                        value === "trending"
                                            ? "trending"
                                            : value === "hot"
                                              ? "hot"
                                              : "all_time";
                                    if (next === board) return;
                                    setBoard(next);
                                    setQuery("");
                                    void loadBoard(next);
                                }}
                            />
                            <label
                                className={styles["label"]}
                                htmlFor={`${prefix}-search`}
                            >
                                {t("skills.marketplace.search")}
                            </label>
                            <div className={styles["searchRow"]}>
                                <input
                                    ref={searchInput}
                                    id={`${prefix}-search`}
                                    className={editForm["input"]}
                                    value={query}
                                    readOnly={busy}
                                    autoComplete="off"
                                    spellCheck={false}
                                    placeholder={t(
                                        "skills.marketplace.searchPlaceholder",
                                    )}
                                    onChange={(event) => {
                                        setQuery(event.currentTarget.value);
                                    }}
                                    onKeyDown={(event) => {
                                        if (event.key === "Enter") {
                                            event.preventDefault();
                                            void search();
                                        }
                                    }}
                                />
                                <button
                                    type="button"
                                    className={buttons["secondary"]}
                                    aria-disabled={
                                        busy || query.trim().length < 2
                                    }
                                    onClick={() => {
                                        void search();
                                    }}
                                >
                                    {t("skills.marketplace.searchAction")}
                                </button>
                            </div>
                        </div>

                        <div ref={listPane} className={styles["listPane"]}>
                            {catalog.kind === "loading" && (
                                <p className={styles["hint"]} role="status">
                                    {t("skills.marketplace.loading")}
                                </p>
                            )}
                            {catalog.kind === "preview" && (
                                <p className={styles["hint"]} role="status">
                                    {t("skills.marketplace.desktopRequired")}
                                </p>
                            )}
                            {catalog.kind === "error" && (
                                <p className={styles["error"]} role="alert">
                                    {t(`skills.errors.${catalog.code}`)}
                                </p>
                            )}
                            {catalog.kind === "ready" &&
                                catalog.skills.length === 0 && (
                                    <p className={styles["hint"]} role="status">
                                        {t("skills.marketplace.empty")}
                                    </p>
                                )}
                            {catalog.kind === "ready" &&
                                catalog.skills.length > 0 && (
                                    <>
                                        <ul
                                            className={styles["grid"]}
                                            aria-label={t(
                                                "skills.marketplace.listLabel",
                                            )}
                                        >
                                            {pageSkills.map((row) => {
                                                const activeRow =
                                                    selected?.id === row.id;
                                                return (
                                                    <li
                                                        key={row.id}
                                                        className={
                                                            styles["gridItem"]
                                                        }
                                                    >
                                                        <button
                                                            type="button"
                                                            className={[
                                                                styles["card"],
                                                                activeRow
                                                                    ? styles[
                                                                          "cardSelected"
                                                                      ]
                                                                    : "",
                                                            ]
                                                                .filter(Boolean)
                                                                .join(" ")}
                                                            aria-pressed={
                                                                activeRow
                                                            }
                                                            disabled={busy}
                                                            onClick={() => {
                                                                choose(row);
                                                            }}
                                                        >
                                                            <span
                                                                className={[
                                                                    styles[
                                                                        "cardName"
                                                                    ],
                                                                    hierarchy[
                                                                        "value"
                                                                    ],
                                                                ].join(" ")}
                                                            >
                                                                {row.name}
                                                            </span>
                                                            {row.description !==
                                                            null ? (
                                                                <span
                                                                    className={[
                                                                        styles[
                                                                            "cardSummary"
                                                                        ],
                                                                        hierarchy[
                                                                            "hint"
                                                                        ],
                                                                    ].join(
                                                                        " ",
                                                                    )}
                                                                >
                                                                    {truncateCardSummary(
                                                                        row.description,
                                                                    )}
                                                                </span>
                                                            ) : prefetchPending.has(
                                                                  row.id,
                                                              ) ? (
                                                                <span
                                                                    className={
                                                                        styles[
                                                                            "cardSummarySkeleton"
                                                                        ]
                                                                    }
                                                                    aria-hidden="true"
                                                                />
                                                            ) : null}
                                                            {row.installs !==
                                                            null ? (
                                                                <span
                                                                    className={[
                                                                        styles[
                                                                            "cardMeta"
                                                                        ],
                                                                        hierarchy[
                                                                            "hint"
                                                                        ],
                                                                    ].join(
                                                                        " ",
                                                                    )}
                                                                >
                                                                    {t(
                                                                        "skills.marketplace.installsCount",
                                                                        {
                                                                            count: row.installs,
                                                                        },
                                                                    )}
                                                                </span>
                                                            ) : null}
                                                        </button>
                                                    </li>
                                                );
                                            })}
                                        </ul>
                                        {pageCount > 1 && (
                                            <nav
                                                className={styles["pagination"]}
                                                aria-label={t(
                                                    "skills.marketplace.paginationLabel",
                                                )}
                                            >
                                                <button
                                                    type="button"
                                                    className={
                                                        buttons["secondary"]
                                                    }
                                                    disabled={
                                                        busy || safePage <= 1
                                                    }
                                                    onClick={() => {
                                                        goToPage(safePage - 1);
                                                    }}
                                                >
                                                    {t(
                                                        "skills.marketplace.prevPage",
                                                    )}
                                                </button>
                                                <ol
                                                    className={
                                                        styles["pageList"]
                                                    }
                                                >
                                                    {Array.from(
                                                        {
                                                            length: pageCount,
                                                        },
                                                        (_, index) => {
                                                            const number =
                                                                index + 1;
                                                            const current =
                                                                number ===
                                                                safePage;
                                                            return (
                                                                <li
                                                                    key={number}
                                                                >
                                                                    <button
                                                                        type="button"
                                                                        className={[
                                                                            styles[
                                                                                "pageButton"
                                                                            ],
                                                                            current
                                                                                ? styles[
                                                                                      "pageButtonCurrent"
                                                                                  ]
                                                                                : "",
                                                                        ]
                                                                            .filter(
                                                                                Boolean,
                                                                            )
                                                                            .join(
                                                                                " ",
                                                                            )}
                                                                        aria-label={t(
                                                                            "skills.marketplace.pageStatus",
                                                                            {
                                                                                page: number,
                                                                                pages: pageCount,
                                                                            },
                                                                        )}
                                                                        aria-current={
                                                                            current
                                                                                ? "page"
                                                                                : undefined
                                                                        }
                                                                        disabled={
                                                                            busy
                                                                        }
                                                                        onClick={() => {
                                                                            goToPage(
                                                                                number,
                                                                            );
                                                                        }}
                                                                    >
                                                                        {number}
                                                                    </button>
                                                                </li>
                                                            );
                                                        },
                                                    )}
                                                </ol>
                                                <button
                                                    type="button"
                                                    className={
                                                        buttons["secondary"]
                                                    }
                                                    disabled={
                                                        busy ||
                                                        safePage >= pageCount
                                                    }
                                                    onClick={() => {
                                                        goToPage(safePage + 1);
                                                    }}
                                                >
                                                    {t(
                                                        "skills.marketplace.nextPage",
                                                    )}
                                                </button>
                                            </nav>
                                        )}
                                    </>
                                )}
                        </div>

                        {/* While the detail dialog is open it owns review/install UI. */}
                        <div className={styles["footer"]}>
                            {selected !== null &&
                                !detailOpen &&
                                install.kind !== "reviewed" && (
                                    <p className={styles["hint"]} role="status">
                                        {t("skills.marketplace.selected", {
                                            name: selected.name,
                                            source: selected.source,
                                        })}
                                    </p>
                                )}

                            {!detailOpen && install.kind === "reviewed" && (
                                <div className={styles["review"]}>
                                    <p className={styles["reviewHeading"]}>
                                        {t("skills.marketplace.reviewHeading")}
                                    </p>
                                    <dl className={styles["facts"]}>
                                        <dt>
                                            {t("skills.install.factSource")}
                                        </dt>
                                        <dd>{install.preview.sourceUrl}</dd>
                                        <dt>
                                            {t("skills.install.factRevision")}
                                        </dt>
                                        <dd>{install.preview.revision}</dd>
                                    </dl>
                                    <pre className={styles["command"]}>
                                        <code>{install.preview.command}</code>
                                    </pre>
                                    <p className={styles["hint"]}>
                                        {t("skills.marketplace.warning")}
                                    </p>
                                </div>
                            )}

                            {!detailOpen && install.kind === "error" && (
                                <div className={styles["review"]}>
                                    <p className={styles["error"]} role="alert">
                                        {t(`skills.errors.${install.code}`)}
                                    </p>
                                    {install.uncertain && (
                                        <p className={styles["hint"]}>
                                            {t("skills.reloadUnknown")}
                                        </p>
                                    )}
                                </div>
                            )}

                            {!detailOpen && install.kind === "reviewing" && (
                                <p className={styles["hint"]} role="status">
                                    {t("skills.install.reviewing")}
                                </p>
                            )}
                            {!detailOpen && install.kind === "installing" && (
                                <p className={styles["hint"]} role="status">
                                    {t("skills.marketplace.installing")}
                                </p>
                            )}
                        </div>

                        <SecondaryPageActions>
                            <div className={styles["headerActions"]}>
                                {install.kind === "error" &&
                                    install.uncertain && (
                                        <button
                                            type="button"
                                            className={buttons["secondary"]}
                                            onClick={() => {
                                                // Re-read the current board so the
                                                // user can see whether the install landed.
                                                void loadBoard(board);
                                            }}
                                        >
                                            {t("skills.reload")}
                                        </button>
                                    )}
                                {!detailOpen &&
                                    (install.kind === "reviewed" ? (
                                        <button
                                            ref={confirmButton}
                                            type="button"
                                            className={buttons["primary"]}
                                            aria-disabled={busy}
                                            onClick={() => {
                                                void installSelected();
                                            }}
                                        >
                                            <Icon name="check" />
                                            {t("skills.marketplace.confirm")}
                                        </button>
                                    ) : (
                                        <button
                                            type="button"
                                            className={buttons["primary"]}
                                            aria-disabled={
                                                busy || selected === null
                                            }
                                            onClick={() => {
                                                void review();
                                            }}
                                        >
                                            {t("skills.marketplace.review")}
                                        </button>
                                    ))}
                            </div>
                        </SecondaryPageActions>
                    </>
                )}
                <Dialog.Root
                    open={detailOpen && selected !== null}
                    onOpenChange={(open) => {
                        if (!open) closeDetail();
                    }}
                >
                    <Dialog.Portal>
                        <Dialog.Backdrop className={dialog["backdrop"]} />
                        <Dialog.Popup
                            className={styles["detailPopup"]}
                            finalFocus={() => listPane.current}
                        >
                            {selected !== null && (
                                <>
                                    <Dialog.Title className={styles["detailTitle"]}>
                                        {t("skills.marketplace.detailTitle")}
                                    </Dialog.Title>
                                    {/*
                                      * Main body prefers Summary, else meta description.
                                      * Loading/error stay short; failure never invents text.
                                      */}
                                    {detailCopy.kind === "loading" ? (
                                        <Dialog.Description
                                            className={styles["hint"]}
                                        >
                                            {t(
                                                "skills.marketplace.detailLoading",
                                            )}
                                        </Dialog.Description>
                                    ) : detailCopy.kind === "preview" ? (
                                        <Dialog.Description
                                            className={styles["hint"]}
                                        >
                                            {t(
                                                "skills.marketplace.desktopRequired",
                                            )}
                                        </Dialog.Description>
                                    ) : detailCopy.kind === "error" ? (
                                        <Dialog.Description
                                            className={styles["error"]}
                                        >
                                            {t(
                                                `skills.errors.${detailCopy.code}`,
                                            )}
                                        </Dialog.Description>
                                    ) : detailCopy.kind === "ready" ? (
                                        <Dialog.Description
                                            className={styles["detailBody"]}
                                        >
                                            {detailCopy.text}
                                        </Dialog.Description>
                                    ) : null}
                                    <dl
                                        className={[
                                            styles["facts"],
                                            styles["detailFacts"],
                                        ].join(" ")}
                                    >
                                        <dt>{t("skills.factName")}</dt>
                                        <dd>{selected.name}</dd>
                                        <dt>{t("skills.factSource")}</dt>
                                        <dd>{selected.source}</dd>
                                        <dt>
                                            {t(
                                                "skills.marketplace.factSkillId",
                                            )}
                                        </dt>
                                        <dd>{selected.skillId}</dd>
                                        {selected.installs !== null ? (
                                            <>
                                                <dt>
                                                    {t(
                                                        "skills.marketplace.factInstalls",
                                                    )}
                                                </dt>
                                                <dd>
                                                    {t(
                                                        "skills.marketplace.installsCount",
                                                        {
                                                            count: selected.installs,
                                                        },
                                                    )}
                                                </dd>
                                            </>
                                        ) : null}
                                        {selected.catalogInstallCommand !==
                                        null ? (
                                            <>
                                                <dt>
                                                    {t(
                                                        "skills.marketplace.factCatalogInstall",
                                                    )}
                                                </dt>
                                                <dd>
                                                    <code>
                                                        {
                                                            selected.catalogInstallCommand
                                                        }
                                                    </code>
                                                    <p
                                                        className={
                                                            styles["hint"]
                                                        }
                                                    >
                                                        {t(
                                                            "skills.marketplace.catalogInstallHint",
                                                        )}
                                                    </p>
                                                </dd>
                                            </>
                                        ) : null}
                                    </dl>
                                    {install.kind === "reviewed" && (
                                        <div className={styles["review"]}>
                                            <p
                                                className={
                                                    styles["reviewHeading"]
                                                }
                                            >
                                                {t(
                                                    "skills.marketplace.reviewHeading",
                                                )}
                                            </p>
                                            <dl className={styles["facts"]}>
                                                <dt>
                                                    {t(
                                                        "skills.install.factSource",
                                                    )}
                                                </dt>
                                                <dd>
                                                    {install.preview.sourceUrl}
                                                </dd>
                                                <dt>
                                                    {t(
                                                        "skills.install.factRevision",
                                                    )}
                                                </dt>
                                                <dd>
                                                    {install.preview.revision}
                                                </dd>
                                            </dl>
                                            <pre className={styles["command"]}>
                                                <code>
                                                    {install.preview.command}
                                                </code>
                                            </pre>
                                            <p className={styles["hint"]}>
                                                {t(
                                                    "skills.marketplace.warning",
                                                )}
                                            </p>
                                        </div>
                                    )}
                                    {install.kind === "error" && (
                                        <div className={styles["review"]}>
                                            <p
                                                className={styles["error"]}
                                                role="alert"
                                            >
                                                {t(
                                                    `skills.errors.${install.code}`,
                                                )}
                                            </p>
                                            {install.uncertain && (
                                                <p className={styles["hint"]}>
                                                    {t("skills.reloadUnknown")}
                                                </p>
                                            )}
                                        </div>
                                    )}
                                    {install.kind === "reviewing" && (
                                        <p
                                            className={styles["hint"]}
                                            role="status"
                                        >
                                            {t("skills.install.reviewing")}
                                        </p>
                                    )}
                                    {install.kind === "installing" && (
                                        <p
                                            className={styles["hint"]}
                                            role="status"
                                        >
                                            {t("skills.marketplace.installing")}
                                        </p>
                                    )}
                                    <div className={styles["detailActions"]}>
                                        {install.kind === "error" &&
                                            install.uncertain && (
                                                <button
                                                    type="button"
                                                    className={
                                                        buttons["secondary"]
                                                    }
                                                    onClick={() => {
                                                        setDetailOpen(false);
                                                        void loadBoard(board);
                                                    }}
                                                >
                                                    {t("skills.reload")}
                                                </button>
                                            )}
                                        <button
                                            type="button"
                                            className={buttons["secondary"]}
                                            aria-disabled={
                                                mutationBusy ||
                                                (install.kind === "error" &&
                                                    install.uncertain)
                                            }
                                            onClick={() => {
                                                closeDetail();
                                            }}
                                        >
                                            {t("skills.marketplace.close")}
                                        </button>
                                        {install.kind === "reviewed" ? (
                                            <button
                                                type="button"
                                                className={buttons["primary"]}
                                                aria-disabled={busy}
                                                onClick={() => {
                                                    void installSelected();
                                                }}
                                            >
                                                <Icon name="check" />
                                                {t(
                                                    "skills.marketplace.confirm",
                                                )}
                                            </button>
                                        ) : (
                                            <button
                                                type="button"
                                                className={buttons["primary"]}
                                                aria-disabled={
                                                    busy ||
                                                    install.kind ===
                                                        "reviewing" ||
                                                    install.kind ===
                                                        "installing"
                                                }
                                                onClick={() => {
                                                    void review();
                                                }}
                                            >
                                                {t("skills.marketplace.review")}
                                            </button>
                                        )}
                                    </div>
                                </>
                            )}
                        </Dialog.Popup>
                    </Dialog.Portal>
                </Dialog.Root>
            </div>
        </ProviderPage>
    );
}

/** Result summary stays on the page until the user leaves with Done or Back. */
function InstallResult({
    report,
}: {
    report: SkillInstallReport;
}): JSX.Element {
    const { t } = useTranslation();
    return (
        <div className={styles["review"]}>
            <p className={styles["reviewHeading"]} role="status">
                {report.imported.length === 0
                    ? t("skills.install.resultNone")
                    : t("skills.install.resultImported", {
                          count: report.imported.length,
                      })}
            </p>
            {report.skipped.length > 0 && (
                <>
                    <p className={styles["hint"]}>
                        {t("skills.install.resultSkipped", {
                            count: report.skipped.length,
                        })}
                    </p>
                    <ul className={styles["skipped"]}>
                        {report.skipped.map((row, index) => (
                            <li key={`${row.folder}-${String(index)}`}>
                                {t("skills.reportItem", {
                                    folder: row.folder,
                                    reason: t(`skills.errors.${row.code}`),
                                })}
                            </li>
                        ))}
                    </ul>
                </>
            )}
            <dl className={styles["facts"]}>
                <dt>{t("skills.install.factSource")}</dt>
                <dd>{report.sourceUrl}</dd>
                <dt>{t("skills.install.factResolvedRevision")}</dt>
                <dd>{report.revision}</dd>
                <dt>{t("skills.install.factExit")}</dt>
                <dd>{report.exitStatus}</dd>
            </dl>
            <pre className={styles["command"]}>
                <code>{report.command}</code>
            </pre>
        </div>
    );
}
