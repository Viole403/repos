import { useCallback, useEffect, useState } from "react";
import { ArrowRight } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { Table, TableCard } from "@/components/application/table/table";
import { formatQuantity, formatTimestamp } from "@/app/format";
import type { BatchRow, StockMovement, StockRow } from "@/app/ipc";
import { createItemBatch, listItemBatches, listStock, listStockMovements } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

/**
 * Lots for a perishable. `is_expired` is computed server-side against today's date
 * rather than stored, so no flag has to be rewritten at midnight to stay honest — and
 * a lot expiring *today* is still sellable, which is why the badge reads "expired"
 * rather than the row being hidden.
 */
const BatchPanel = ({ itemId, batches, onChanged }: { itemId: number; batches: BatchRow[]; onChanged: () => void }) => {
    const [adding, setAdding] = useState(false);
    const [batchNo, setBatchNo] = useState("");
    const [expiry, setExpiry] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);

    const submit = async (e: React.FormEvent) => {
        e.preventDefault();
        setBusy(true);
        setError(null);
        try {
            await createItemBatch({ itemId, batchNo: batchNo.trim(), expiryDate: expiry.trim() || null });
            setBatchNo("");
            setExpiry("");
            setAdding(false);
            onChanged();
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    };

    return (
        <div className="flex flex-col gap-3">
            <div className="flex items-center justify-between">
                <h3 className="text-title-sm font-semibold text-primary">Lots</h3>
                <Button size="sm" color="secondary" onPress={() => setAdding((open) => !open)}>
                    {adding ? "Cancel" : "Add lot"}
                </Button>
            </div>

            {adding && (
                <form onSubmit={submit} className="flex flex-col gap-3 rounded-lg bg-secondary px-4 py-4">
                    {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}
                    <div className="grid gap-3 md:grid-cols-2">
                        <Input label="Batch number" value={batchNo} onChange={setBatchNo} isRequired />
                        <Input
                            label="Expiry date"
                            type="date"
                            value={expiry}
                            onChange={setExpiry}
                            hint="Blank for a lot that does not expire"
                        />
                    </div>
                    <div className="flex justify-end">
                        <Button type="submit" size="sm" isLoading={busy} isDisabled={batchNo.trim() === ""}>
                            Save lot
                        </Button>
                    </div>
                </form>
            )}

            {batches.length > 0 && (
                <TableCard.Root>
                    <Table aria-label="Lots">
                        <Table.Header>
                            <Table.Head label="Batch" />
                            <Table.Head label="Expires" />
                            <Table.Head label="On hand" />
                            <Table.Head label="Status" />
                        </Table.Header>
                        <Table.Body>
                            {batches.map((batch) => (
                                <Table.Row key={batch.batchId} id={batch.batchId}>
                                    <Table.Cell className="font-medium text-primary">{batch.batchNo}</Table.Cell>
                                    <Table.Cell className="text-tertiary">{batch.expiryDate ?? "—"}</Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatQuantity(batch.onHand)}</Table.Cell>
                                    <Table.Cell>
                                        {batch.isExpired ? (
                                            <span className="rounded-md bg-error-secondary px-2 py-0.5 text-sm font-medium text-error-primary">
                                                Expired
                                            </span>
                                        ) : (
                                            <span className="text-tertiary">OK</span>
                                        )}
                                    </Table.Cell>
                                </Table.Row>
                            ))}
                        </Table.Body>
                    </Table>
                </TableCard.Root>
            )}
        </div>
    );
};

/**
 * What is on the shelf, derived from the ledger — never stored, never stale.
 * The alert quantity is the item's own threshold; without one the row carries
 * no low-stock opinion.
 */
export const Stock = () => {
    const [rows, setRows] = useState<StockRow[]>([]);
    const [total, setTotal] = useState(0);
    const [page, setPage] = useState(1);
    const [term, setTerm] = useState("");
    const [lowOnly, setLowOnly] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const [detail, setDetail] = useState<StockRow | null>(null);
    const [movements, setMovements] = useState<StockMovement[]>([]);
    const [batches, setBatches] = useState<BatchRow[]>([]);
    const [detailError, setDetailError] = useState<string | null>(null);

    const perPage = 20;

    const refresh = useCallback(async () => {
        try {
            const found = await listStock(lowOnly || null, {
                page,
                perPage,
                search: term.trim() || undefined,
            });
            setRows(found.rows);
            setTotal(found.total);
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, [page, term, lowOnly]);

    useEffect(() => {
        void refresh();
    }, [refresh]);

    const openDetail = useCallback(async (row: StockRow) => {
        setDetail(row);
        setDetailError(null);
        try {
            // Batches come back empty for the overwhelming majority of items, so a
            // failure here must not hide the movement history underneath.
            const lots = await listItemBatches(row.itemId);
            setBatches(lots.rows);
        } catch {
            setBatches([]);
        }
        try {
            const found = await listStockMovements(row.itemId, { page: 1, perPage: 20 });
            setMovements(found.rows);
        } catch (cause) {
            setDetailError(messageOf(cause));
        }
    }, []);

    const pages = Math.max(1, Math.ceil(total / perPage));

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-1">
                <h1 className="text-display-xs font-semibold text-primary">Stock on hand</h1>
                <p className="text-md text-tertiary">Derived from the ledger — what moved, and what is left.</p>
            </div>

            <div className="flex flex-col gap-3 md:flex-row md:items-end">
                <Input
                    label="Search"
                    placeholder="Name or code"
                    value={term}
                    onChange={(value) => {
                        setPage(1);
                        setTerm(String(value));
                    }}
                    className="w-full md:w-64"
                />
                <Button
                    color={lowOnly ? "primary" : "secondary"}
                    aria-pressed={lowOnly}
                    onPress={() => {
                        setPage(1);
                        setLowOnly((current) => !current);
                    }}
                    className="w-fit"
                >
                    {lowOnly ? "Low stock only" : "Show low stock only"}
                </Button>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">
                    {lowOnly ? "Nothing is low. The shelf is fine." : "No items yet."}
                </p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Stock" description={`${total} total`} />
                    <Table aria-label="Stock">
                        <Table.Header>
                            <Table.Head label="Item" />
                            <Table.Head label="Code" />
                            <Table.Head label="On hand" />
                            <Table.Head label="Alert at" />
                            <Table.Head label="Status" />
                            <Table.Head label="" className="w-12" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.itemId} id={row.itemId}>
                                    <Table.Cell className="font-medium text-primary">{row.itemName}</Table.Cell>
                                    <Table.Cell className="text-tertiary">{row.itemCode}</Table.Cell>
                                    <Table.Cell className="tabular-nums">
                                        {formatQuantity(row.onHand)}
                                    </Table.Cell>
                                    <Table.Cell className="tabular-nums text-tertiary">
                                        {row.alertQuantity ?? "—"}
                                    </Table.Cell>
                                    <Table.Cell>
                                        {row.isLow ? (
                                            <span className="rounded-md bg-error-secondary px-2 py-0.5 text-sm font-medium text-error-primary">
                                                Low
                                            </span>
                                        ) : (
                                            <span className="text-tertiary">OK</span>
                                        )}
                                    </Table.Cell>
                                    <Table.Cell>
                                        <Button
                                            size="sm"
                                            color="tertiary"
                                            iconLeading={ArrowRight}
                                            aria-label={`History of ${row.itemName}`}
                                            onPress={() => void openDetail(row)}
                                        />
                                    </Table.Cell>
                                </Table.Row>
                            ))}
                        </Table.Body>
                    </Table>
                </TableCard.Root>
            )}

            {pages > 1 && (
                <div className="flex items-center justify-end gap-3">
                    <Button color="secondary" size="sm" isDisabled={page <= 1} onPress={() => setPage((n) => n - 1)}>
                        Previous
                    </Button>
                    <span className="text-sm text-tertiary">
                        {page} / {pages}
                    </span>
                    <Button
                        color="secondary"
                        size="sm"
                        isDisabled={page >= pages}
                        onPress={() => setPage((n) => n + 1)}
                    >
                        Next
                    </Button>
                </div>
            )}

            {detail !== null && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setDetail(null)}>
                    <Modal className="max-w-2xl">
                        <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                            <div className="flex flex-col gap-4">
                                <div>
                                    <h2 className="text-display-xs font-semibold text-primary">
                                        {detail.itemName}
                                    </h2>
                                    <p className="text-md tabular-nums text-tertiary">
                                        On hand {formatQuantity(detail.onHand)}
                                        {detail.alertQuantity !== null &&
                                            ` · alerts at ${formatQuantity(detail.alertQuantity)}`}
                                    </p>
                                </div>

                                {detailError !== null && (
                                    <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                        {detailError}
                                    </p>
                                )}

                                <BatchPanel
                                    itemId={detail.itemId}
                                    batches={batches}
                                    onChanged={() => void openDetail(detail)}
                                />

                                <TableCard.Root>
                                    <TableCard.Header title="Movements" description="newest first" />
                                    <Table aria-label="Movements">
                                        <Table.Header>
                                            <Table.Head label="When" />
                                            <Table.Head label="What" />
                                            <Table.Head label="Qty" />
                                            <Table.Head label="Balance" />
                                        </Table.Header>
                                        <Table.Body>
                                            {movements.map((m) => (
                                                <Table.Row key={m.id} id={m.id}>
                                                    <Table.Cell className="text-tertiary">
                                                        {formatTimestamp(m.createdAt)}
                                                    </Table.Cell>
                                                    <Table.Cell>
                                                        {m.movementType}
                                                        {m.reference && (
                                                            <span className="text-tertiary"> · {m.reference}</span>
                                                        )}
                                                    </Table.Cell>
                                                    <Table.Cell className="tabular-nums">
                                                        {formatQuantity(m.quantity)}
                                                    </Table.Cell>
                                                    <Table.Cell className="tabular-nums">
                                                        {formatQuantity(m.balanceAfter)}
                                                    </Table.Cell>
                                                </Table.Row>
                                            ))}
                                        </Table.Body>
                                    </Table>
                                </TableCard.Root>

                                <div className="flex justify-end">
                                    <Button color="secondary" onPress={() => setDetail(null)}>
                                        Close
                                    </Button>
                                </div>
                            </div>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}
        </div>
    );
};
