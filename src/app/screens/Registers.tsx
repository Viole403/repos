import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Table, TableCard } from "@/components/application/table/table";
import { formatMoney, formatTimestamp, toDecimal } from "@/app/format";
import type { RegisterSummary, RegisterView } from "@/app/ipc";
import { closeRegister, currentRegister, listRegisters, openRegister, registerSummary } from "@/app/ipc";

type Loaded =
    | { kind: "loading" }
    | { kind: "ready"; current: RegisterView | null; summary: RegisterSummary | null; history: RegisterView[] }
    | { kind: "failed"; message: string };

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const normalizeMoney = (raw: string): string => raw.replace(/\./g, "").replace(",", ".");

const isMoney = (raw: string): boolean => {
    const trimmed = raw.trim();
    return trimmed !== "" && Number.isFinite(Number(normalizeMoney(trimmed))) && Number(normalizeMoney(trimmed)) >= 0;
};

/**
 * One cashier shift. Opening records the float, closing counts the drawer against
 * a snapshot the server derives — the screen never computes expected cash itself,
 * so the number the cashier is judged against is the same one the report shows.
 */
export const Registers = () => {
    const [state, setState] = useState<Loaded>({ kind: "loading" });
    const [reload, setReload] = useState(0);
    const [opening, setOpening] = useState("");
    const [openNote, setOpenNote] = useState("");
    const [counted, setCounted] = useState("");
    const [closeNote, setCloseNote] = useState("");
    const [actionError, setActionError] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);

    useEffect(() => {
        let cancelled = false;
        setState({ kind: "loading" });

        Promise.all([currentRegister(), registerSummary(), listRegisters()])
            .then(([current, summary, history]) => {
                if (!cancelled) setState({ kind: "ready", current, summary, history });
            })
            .catch((error: unknown) => {
                if (!cancelled) setState({ kind: "failed", message: messageOf(error) });
            });

        return () => {
            cancelled = true;
        };
    }, [reload]);

    const refresh = useCallback(() => setReload((n) => n + 1), []);

    const open = useCallback(async () => {
        setBusy(true);
        setActionError(null);
        try {
            await openRegister({
                openingBalance: toDecimal(normalizeMoney(opening.trim())),
                note: openNote.trim() === "" ? null : openNote.trim(),
            });
            setOpening("");
            setOpenNote("");
            refresh();
        } catch (error) {
            setActionError(messageOf(error));
        } finally {
            setBusy(false);
        }
    }, [opening, openNote, refresh]);

    const close = useCallback(async () => {
        setBusy(true);
        setActionError(null);
        try {
            await closeRegister({
                closingBalance: toDecimal(normalizeMoney(counted.trim())),
                note: closeNote.trim() === "" ? null : closeNote.trim(),
            });
            setCounted("");
            setCloseNote("");
            refresh();
        } catch (error) {
            setActionError(messageOf(error));
        } finally {
            setBusy(false);
        }
    }, [counted, closeNote, refresh]);

    return (
        <div className="flex flex-col gap-6 p-6 md:p-8">
            {actionError !== null && (
                <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{actionError}</p>
            )}

            {state.kind === "failed" ? (
                <p className="px-6 py-12 text-center text-md text-error-primary">{state.message}</p>
            ) : state.kind === "loading" ? (
                <p className="px-6 py-12 text-center text-md text-tertiary">Loading…</p>
            ) : state.current !== null ? (
                <TableCard.Root>
                    <TableCard.Header
                        title="Register open"
                        description={`Opened ${formatTimestamp(state.current.openedAt)} · float ${formatMoney(state.current.openingBalance)}`}
                    />
                    <div className="flex flex-col gap-4 p-6">
                        {state.summary !== null && (
                            <dl className="grid grid-cols-2 gap-3 md:grid-cols-4">
                                <div>
                                    <dt className="text-sm text-tertiary">Sales</dt>
                                    <dd className="font-medium tabular-nums text-primary">{state.summary.salesCount}</dd>
                                </div>
                                <div>
                                    <dt className="text-sm text-tertiary">Sales total</dt>
                                    <dd className="font-medium tabular-nums text-primary">{formatMoney(state.summary.salesTotal)}</dd>
                                </div>
                                <div>
                                    <dt className="text-sm text-tertiary">Cash in drawer</dt>
                                    <dd className="font-medium tabular-nums text-primary">{formatMoney(state.summary.cashTotal)}</dd>
                                </div>
                                <div>
                                    <dt className="text-sm text-tertiary">Other methods</dt>
                                    <dd className="font-medium tabular-nums text-primary">{formatMoney(state.summary.otherTotal)}</dd>
                                </div>
                                <div>
                                    <dt className="text-sm text-tertiary">Refunded</dt>
                                    <dd className="font-medium tabular-nums text-primary">{formatMoney(state.summary.refundedTotal)}</dd>
                                </div>
                                <div>
                                    <dt className="text-sm text-tertiary">Debt collected</dt>
                                    <dd className="font-medium tabular-nums text-primary">{formatMoney(state.summary.receiptsTotal)}</dd>
                                </div>
                                <div>
                                    <dt className="text-sm text-tertiary">Expected in drawer</dt>
                                    <dd className="text-lg font-semibold tabular-nums text-primary">
                                        {formatMoney(state.summary.expectedBalance)}
                                    </dd>
                                </div>
                            </dl>
                        )}
                        <div className="flex flex-col gap-3 md:flex-row md:items-end">
                            <Input
                                label="Counted cash"
                                placeholder="What you counted in the drawer"
                                value={counted}
                                onChange={setCounted}
                                isInvalid={counted.trim() !== "" && !isMoney(counted)}
                                className="w-full md:w-72"
                            />
                            <Input
                                label="Note"
                                placeholder="Optional"
                                value={closeNote}
                                onChange={setCloseNote}
                                className="w-full md:w-72"
                            />
                            <Button
                                isLoading={busy}
                                isDisabled={!isMoney(counted)}
                                onPress={() => void close()}
                            >
                                Close register
                            </Button>
                        </div>
                    </div>
                </TableCard.Root>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="No open register" description="Count the float, then open." />
                    <div className="flex flex-col gap-3 p-6 md:flex-row md:items-end">
                        <Input
                            label="Opening float"
                            placeholder="Cash in the drawer at open"
                            value={opening}
                            onChange={setOpening}
                            isInvalid={opening.trim() !== "" && !isMoney(opening)}
                            className="w-full md:w-72"
                        />
                        <Input
                            label="Note"
                            placeholder="Optional"
                            value={openNote}
                            onChange={setOpenNote}
                            className="w-full md:w-72"
                        />
                        <Button isLoading={busy} isDisabled={!isMoney(opening)} onPress={() => void open()}>
                            Open register
                        </Button>
                    </div>
                </TableCard.Root>
            )}

            {state.kind === "ready" && (
                <TableCard.Root>
                    <TableCard.Header title="Past shifts" description={`${state.history.length} register${state.history.length === 1 ? "" : "s"}`} />
                    {state.history.length === 0 ? (
                        <p className="px-6 py-12 text-center text-md text-tertiary">No shifts yet.</p>
                    ) : (
                        <Table>
                            <Table.Header>
                                <Table.Head label="Opened" />
                                <Table.Head label="Closed" />
                                <Table.Head label="Float" className="text-right" />
                                <Table.Head label="Expected" className="text-right" />
                                <Table.Head label="Counted" className="text-right" />
                                <Table.Head label="Variance" className="text-right" />
                            </Table.Header>
                            <Table.Body>
                                {state.history.map((row) => (
                                    <Table.Row key={row.id} id={row.id}>
                                        <Table.Cell className="text-tertiary">{formatTimestamp(row.openedAt)}</Table.Cell>
                                        <Table.Cell className="text-tertiary">
                                            {row.closedAt === null ? "Open" : formatTimestamp(row.closedAt)}
                                        </Table.Cell>
                                        <Table.Cell className="text-right tabular-nums">{formatMoney(row.openingBalance)}</Table.Cell>
                                        <Table.Cell className="text-right tabular-nums">
                                            {row.expectedBalance === null ? "—" : formatMoney(row.expectedBalance)}
                                        </Table.Cell>
                                        <Table.Cell className="text-right tabular-nums">
                                            {row.closingBalance === null ? "—" : formatMoney(row.closingBalance)}
                                        </Table.Cell>
                                        <Table.Cell className="text-right font-medium tabular-nums text-primary">
                                            {row.variance === null ? "—" : formatMoney(row.variance)}
                                        </Table.Cell>
                                    </Table.Row>
                                ))}
                            </Table.Body>
                        </Table>
                    )}
                </TableCard.Root>
            )}
        </div>
    );
};