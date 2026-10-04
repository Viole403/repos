import { useCallback, useEffect, useState } from "react";
import { ArrowLeft } from "@untitledui/icons";
import { useNavigate, useParams } from "react-router-dom";
import { Table, TableCard } from "@/components/application/table/table";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { decAdd, decSub } from "@/app/decimal";
import { formatMoney, formatQuantity, formatTimestamp, toDecimal } from "@/app/format";
import { createReturn, getSale, listReturns, RETURN_REASONS } from "@/app/ipc";
import type { Decimal, SaleView, SaleReturn } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

type State = { status: "loading" } | { status: "error"; message: string } | { status: "ready" };

export const Returns = () => {
    const { id } = useParams();
    const navigate = useNavigate();
    const saleId = Number(id);

    const [state, setState] = useState<State>({ status: "loading" });
    const [sale, setSale] = useState<SaleView | null>(null);
    const [rows, setRows] = useState<SaleReturn[]>([]);
    const [picked, setPicked] = useState<Record<number, string>>({});
    const [reason, setReason] = useState<string>(RETURN_REASONS[0]);
    const [returning, setReturning] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);
    const [reload, setReload] = useState(0);

    const refresh = useCallback(async () => {
        if (!Number.isFinite(saleId)) {
            setState({ status: "error", message: "That is not a sale" });
            return;
        }
        try {
            const [header, returns] = await Promise.all([getSale(saleId), listReturns(saleId)]);
            setSale(header);
            setRows(returns);
            setState({ status: "ready" });
            setError(null);
        } catch (cause) {
            setState({ status: "error", message: messageOf(cause) });
        }
    }, [saleId]);

    useEffect(() => {
        void refresh();
    }, [refresh, reload]);

    if (state.status === "loading") return <p className="p-6 text-md text-tertiary">Loading…</p>;
    if (state.status === "error") {
        return (
            <div className="p-6">
                <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{state.message}</p>
            </div>
        );
    }
    if (!sale) return null;

    // What is still returnable: sold minus what has already come back. Derived here so
    // the cashier cannot be offered a quantity the server will refuse. Decimal
    // arithmetic, not `Number` — 0.1 + 0.2 must not reach a return.
    const returnedByLine = new Map<number, Decimal>();
    for (const row of rows) {
        for (const line of row.lines) {
            const so_far = returnedByLine.get(line.saleDetailId) ?? "0";
            returnedByLine.set(line.saleDetailId, decAdd(so_far, line.quantity));
        }
    }

    const submit = async () => {
        setBusy(true);
        setError(null);
        try {
            await createReturn({
                saleId,
                reason,
                note: null,
                lines: Object.entries(picked)
                    .filter(([, qty]) => qty.trim() !== "")
                    .map(([detailId, qty]) => ({
                        saleDetailId: Number(detailId),
                        quantity: toDecimal(qty),
                    })),
            });
            setReturning(false);
            setPicked({});
            setReload((n) => n + 1);
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    };

    const reasonOptions: SelectItemType[] = RETURN_REASONS.map((r) => ({ id: r, label: r }));

    return (
        <div className="flex flex-col gap-5 p-5">
            <Button
                color="secondary"
                iconLeading={ArrowLeft}
                className="w-fit"
                onPress={() => navigate(`/sales/${saleId}`)}
            >
                Back to {sale.sale.invoiceNo}
            </Button>

            <div className="flex flex-col gap-1">
                <h1 className="text-display-xs font-semibold text-primary">Returns</h1>
                <p className="text-md text-tertiary">
                    Against {sale.sale.invoiceNo} — {formatMoney(sale.sale.grandTotal)} on{" "}
                    {formatTimestamp(sale.sale.createdAt)}.
                </p>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {sale.sale.status !== "Completed" && (
                <p className="rounded-lg bg-warning-secondary px-3 py-2 text-sm text-warning-primary">
                    This sale is {sale.sale.status}. Only a completed sale can be returned against.
                </p>
            )}

            <TableCard.Root>
                <TableCard.Header
                    title="Returnable"
                    description={`${sale.lines.length} lines`}
                    contentTrailing={
                        <Button size="sm" onPress={() => setReturning(true)} isDisabled={sale.sale.status !== "Completed"}>
                            Record a return
                        </Button>
                    }
                />
                <Table aria-label="Returnable lines">
                    <Table.Header>
                        <Table.Head label="Item" />
                        <Table.Head label="Sold" />
                        <Table.Head label="Back" />
                        <Table.Head label="Price" />
                    </Table.Header>
                    <Table.Body>
                        {sale.lines.map((line) => {
                            const back = returnedByLine.get(line.id) ?? "0";
                            const left = decSub(line.quantity, back);
                            return (
                                <Table.Row key={line.id} id={line.id}>
                                    <Table.Cell className="font-medium text-primary">{line.itemName}</Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatQuantity(line.quantity)}</Table.Cell>
                                    <Table.Cell className="tabular-nums text-tertiary">
                                        {formatQuantity(back)} / {formatQuantity(line.quantity)}
                                    </Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatQuantity(left)}</Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatMoney(line.unitPrice)}</Table.Cell>
                                </Table.Row>
                            );
                        })}
                    </Table.Body>
                </Table>
            </TableCard.Root>

            <TableCard.Root>
                <TableCard.Header title="Already returned" description={`${rows.length}`} />
                {rows.length === 0 ? (
                    <p className="px-5 pb-5 text-md text-tertiary">Nothing has come back against this sale.</p>
                ) : (
                    <Table aria-label="Returns">
                        <Table.Header>
                            <Table.Head label="Return" />
                            <Table.Head label="When" />
                            <Table.Head label="Reason" />
                            <Table.Head label="Refunded" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium tabular-nums text-primary">
                                        {row.returnNo}
                                    </Table.Cell>
                                    <Table.Cell className="text-tertiary">{formatTimestamp(row.createdAt)}</Table.Cell>
                                    <Table.Cell>{row.reason}</Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatMoney(row.refundedTotal)}</Table.Cell>
                                </Table.Row>
                            ))}
                        </Table.Body>
                    </Table>
                )}
            </TableCard.Root>

            {returning && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setReturning(false)}>
                    <Modal className="max-w-xl">
                        <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">Record a return</h2>

                                <Select
                                    label="Reason"
                                    items={reasonOptions}
                                    selectedKey={reason}
                                    onSelectionChange={(key) => setReason(String(key ?? RETURN_REASONS[0]))}
                                >
                                    {(row) => (
                                        <Select.Item id={row.id} textValue={row.label}>
                                            {row.label}
                                        </Select.Item>
                                    )}
                                </Select>

                                {sale.lines.map((line) => (
                                    <Input
                                        key={line.id}
                                        label={`${line.itemName} — sold ${formatQuantity(line.quantity)}`}
                                        placeholder="0"
                                        value={picked[line.id] ?? ""}
                                        onChange={(value) =>
                                            setPicked((current) => ({ ...current, [line.id]: String(value) }))
                                        }
                                    />
                                ))}

                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setReturning(false)}>
                                        Cancel
                                    </Button>
                                    <Button onPress={() => void submit()} isLoading={busy}>
                                        Record return
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