import { useCallback, useEffect, useState } from "react";
import { Trash01 } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { Table, TableCard } from "@/components/application/table/table";
import { formatMoney, formatQuantity, formatTimestamp } from "@/app/format";
import { decSum } from "@/app/decimal";
import type { DraftSale, SaleView } from "@/app/ipc";
import { discardDraft, listDraftSales, promoteDraft } from "@/app/ipc";

type Loaded =
    | { kind: "loading" }
    | { kind: "ready"; drafts: DraftSale[] }
    | { kind: "failed"; message: string };

/** Kept separate from `Loaded`: a failed promote must not discard the list. */
type Pending = { promotingId: number | null; error: string | null; completed: SaleView | null };

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

/**
 * Interrupted sales, one per row. A draft is a cart the cashier walked away from, so
 * it is offered back rather than restored silently — which draft to keep is the
 * cashier's call, not an automatic merge of two carts.
 *
 * Deliberately NOT a "load into the cart" screen. The draft's lines already live in
 * the database and `promote_draft` re-reads them from there, so hydrating the client
 * cart would leave two copies to drift. This completes a draft, it does not adopt it.
 */
export const Drafts = () => {
    const [state, setState] = useState<Loaded>({ kind: "loading" });
    const [pending, setPending] = useState<Pending>({ promotingId: null, error: null, completed: null });
    const [reload, setReload] = useState(0);
    const [discarding, setDiscarding] = useState<DraftSale | null>(null);

    useEffect(() => {
        let cancelled = false;
        setState({ kind: "loading" });

        listDraftSales()
            .then((drafts) => {
                if (!cancelled) setState({ kind: "ready", drafts });
            })
            .catch((error: unknown) => {
                if (!cancelled) setState({ kind: "failed", message: messageOf(error) });
            });

        return () => {
            cancelled = true;
        };
    }, [reload]);

    const refresh = useCallback(() => setReload((n) => n + 1), []);

    const promote = useCallback(
        async (saleId: number) => {
            setPending({ promotingId: saleId, error: null, completed: null });
            try {
                // paidTotal omitted means paid in full, which is what resuming a
                // held sale means in practice. The cashier re-keys the payment on
                // the register if that is wrong — promoting is the point of no
                // return, so it is one click and no editable fields.
                const view = await promoteDraft(saleId);
                setPending({ promotingId: null, error: null, completed: view });
                refresh();
            } catch (error) {
                setPending({ promotingId: null, error: messageOf(error), completed: null });
            }
        },
        [refresh],
    );

    const confirmDiscard = useCallback(async () => {
        if (discarding === null) return;
        const id = discarding.sale.id;
        setDiscarding(null);
        try {
            await discardDraft(id);
            refresh();
        } catch (error) {
            setPending((current) => ({ ...current, error: messageOf(error) }));
        }
    }, [discarding, refresh]);

    const backToList = useCallback(() => setPending((current) => ({ ...current, completed: null })), []);

    if (pending.completed !== null) {
        const view = pending.completed;
        return (
            <div className="flex flex-col gap-6 p-6 md:p-8">
                <TableCard.Root>
                    <TableCard.Header title="Sale completed" />
                    <div className="flex flex-col gap-4 p-6">
                        <p className="text-md text-secondary">
                            <span className="font-semibold text-primary">{view.sale.invoiceNo}</span> was completed for{" "}
                            {formatMoney(view.sale.grandTotal)} by {view.sale.paymentMethod}.
                        </p>
                        <Table>
                            <Table.Header>
                                <Table.Head label="Item" />
                                <Table.Head label="Qty" />
                                <Table.Head label="Unit price" />
                                <Table.Head label="Total" className="text-right" />
                            </Table.Header>
                            <Table.Body>
                                {view.lines.map((line) => (
                                    <Table.Row key={line.id} id={line.id}>
                                        <Table.Cell className="font-medium text-primary">{line.itemName}</Table.Cell>
                                        <Table.Cell>{formatQuantity(line.quantity)}</Table.Cell>
                                        <Table.Cell>{formatMoney(line.unitPrice)}</Table.Cell>
                                        <Table.Cell className="text-right font-medium text-primary">{formatMoney(line.lineTotal)}</Table.Cell>
                                    </Table.Row>
                                ))}
                            </Table.Body>
                        </Table>
                        <div className="flex justify-end">
                            <Button onPress={backToList}>Back to drafts</Button>
                        </div>
                    </div>
                </TableCard.Root>
            </div>
        );
    }

    const busy = pending.promotingId !== null;

    return (
        <div className="flex flex-col gap-6 p-6 md:p-8">
            <TableCard.Root>
                <TableCard.Header
                    title="Interrupted sales"
                    description={state.kind === "ready" ? `${state.drafts.length} draft${state.drafts.length === 1 ? "" : "s"}` : undefined}
                />

                {pending.error !== null && (
                    <p className="mx-6 mt-4 rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{pending.error}</p>
                )}

                {state.kind === "failed" ? (
                    <p className="px-6 py-12 text-center text-md text-error-primary">{state.message}</p>
                ) : state.kind === "loading" ? (
                    <p className="px-6 py-12 text-center text-md text-tertiary">Loading…</p>
                ) : state.drafts.length === 0 ? (
                    <p className="px-6 py-12 text-center text-md text-tertiary">No interrupted sales.</p>
                ) : (
                    <Table>
                        <Table.Header>
                            <Table.Head label="Invoice" />
                            <Table.Head label="Lines" />
                            <Table.Head label="Total" className="text-right" />
                            <Table.Head label="Held" />
                            <Table.Head label="" />
                        </Table.Header>
                        <Table.Body>
                            {state.drafts.map((draft) => (
                                <Table.Row key={draft.sale.id} id={draft.sale.id}>
                                    <Table.Cell className="font-medium text-primary">{draft.sale.invoiceNo}</Table.Cell>
                                    <Table.Cell>
                                        {draft.lines.map((line) => `${line.itemName} × ${formatQuantity(line.quantity)}`).join(", ")}
                                    </Table.Cell>
                                    <Table.Cell className="text-right font-medium text-primary">
                                        {formatMoney(decSum(draft.lines.map((line) => line.lineTotal)))}
                                    </Table.Cell>
                                    <Table.Cell className="text-tertiary">{formatTimestamp(draft.sale.createdAt)}</Table.Cell>
                                    <Table.Cell>
                                        <div className="flex justify-end gap-1">
                                            <Button
                                                size="sm"
                                                isLoading={pending.promotingId === draft.sale.id}
                                                isDisabled={busy && pending.promotingId !== draft.sale.id}
                                                onPress={() => promote(draft.sale.id)}
                                            >
                                                Complete
                                            </Button>
                                            <Button
                                                size="sm"
                                                color="tertiary"
                                                iconLeading={Trash01}
                                                aria-label={`Discard ${draft.sale.invoiceNo}`}
                                                isDisabled={busy}
                                                onPress={() => setDiscarding(draft)}
                                            />
                                        </div>
                                    </Table.Cell>
                                </Table.Row>
                            ))}
                        </Table.Body>
                    </Table>
                )}
            </TableCard.Root>

            {discarding && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setDiscarding(null)}>
                    <Modal className="max-w-md">
                        <Dialog className="p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">Discard {discarding.sale.invoiceNo}?</h2>
                                <p className="text-md text-secondary">
                                    This sale was never completed and moved no stock, so there is nothing to account for. It cannot be
                                    recovered afterwards.
                                </p>
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setDiscarding(null)}>
                                        Cancel
                                    </Button>
                                    <Button color="primary-destructive" onPress={confirmDiscard}>
                                        Discard
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