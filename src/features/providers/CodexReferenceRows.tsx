import type { JSX } from "react";
import { useTranslation } from "react-i18next";
import type { CodexReference } from "../../lib/desktop/providers";

/** Class names from the host list, so rows match its Agent reference rows. */
export interface CodexReferenceClasses {
    row?: string | undefined;
    name?: string | undefined;
    meta?: string | undefined;
    hint?: string | undefined;
}

/** Stable React key for one Codex reference row. */
export function codexReferenceKey(reference: CodexReference): string {
    return `codex:${reference.binding}:${reference.configRoot}:${reference.providerKey}:${reference.modelId ?? ""}`;
}

/**
 * List items for Codex rows that block deleting a Provider or model. They are
 * never clearable here: each row names the binding and points to Codex
 * settings, where it is revoked, cleared or recovered.
 */
export function CodexReferenceRows({
    references,
    classes,
}: {
    references: readonly CodexReference[];
    classes: CodexReferenceClasses;
}): JSX.Element {
    const { t } = useTranslation();
    return (
        <>
            {references.map((reference) => (
                <li className={classes.row} key={codexReferenceKey(reference)}>
                    <div>
                        <p className={classes.name}>
                            {t(
                                `providers.codexReferences.names.${reference.binding}`,
                                {
                                    key: reference.providerKey,
                                    model: reference.modelId ?? "",
                                },
                            )}
                        </p>
                        <p className={classes.meta}>
                            {reference.configRoot}
                            {" · "}
                            {t(
                                `providers.delete.referenceStates.${reference.state}`,
                            )}
                        </p>
                    </div>
                    <p className={classes.hint}>
                        {t(
                            `providers.codexReferences.hints.${reference.binding}`,
                        )}
                    </p>
                </li>
            ))}
        </>
    );
}
