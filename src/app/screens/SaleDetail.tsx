import { useEffect, useState } from "react";
import { ArrowLeft } from "@untitledui/icons";
import { useNavigate, useParams } from "react-router-dom";
import { Table, TableCard } from "@/components/application/table/table";
import { Button } from "@/components/base/buttons/button";
import { formatMoney, formatQuantity, formatTimestamp } from "@/app/format";
import { getSale } from "@/app/ipc";
import type { SaleView } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

export const SaleDetail = () => {
    const { id } = useParams();
    const navigate = useNavigate();
    const [view, setView] = useState<SaleView | null>(null);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        const saleId = Number(id);
        if (!Number.isFinite(saleId)) {
            setError("That is not a sale");
            return;
        }
        void getSale(saleId)
            .then(setView)
            .catch((cause) => setError(messageOf(cause)));
    }, [id]);

    return (
        <div className="flex flex-col gap-5 p-5">
            <Button color="secondary" iconLeading={ArrowLeft} className="w-fit" onPress={() => navigate("/sales")}>
                All sales
            </Button>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {!error && !view && <p className="py-10 text-center text-md text-tertiary">Loading…</p>}

            {view && (
                <>
                    <div className="flex flex-col gap-1">
                        <h1 className="text-display-xs font-semibold tabular-nums text-primary">
                            {view.sale.invoiceNo}
                        </h1>
                        <p className="text-md text-tertiary">
                            {formatTimestamp(view.sale.createdAt)} · {view.sale.paymentMethod} ·{" "}
                            {view.sale.status}
                        </p>
                        {view.sale.note && <p className="text-md text-secondary">{view.sale.note}</p>}
                    </div>

                    <TableCard.Root>
                        <TableCard.Header title="Lines" description={`${view.lines.length}`} />
                        <Table aria-label="Sale lines">
                            <Table.Header>
                                <Table.Head label="Item" />
                                <Table.Head label="Qty" />
                                <Table.Head label="Unit price" />
                                <Table.Head label="Discount" />
                                <Table.Head label="Total" />
                            </Table.Header>
                            <Table.Body>
                                {view.lines.map((line) => (
                                    <Table.Row key={line.id} id={line.id}>
                                        <Table.Cell className="font-medium text-primary">{line.itemName}</Table.Cell>
                                        <Table.Cell className="tabular-nums">
                                            {formatQuantity(line.quantity)}
                                        </Table.Cell>
                                        <Table.Cell className="tabular-nums">{formatMoney(line.unitPrice)}</Table.Cell>
                                        <Table.Cell className="tabular-nums">{formatMoney(line.discount)}</Table.Cell>
                                        <Table.Cell className="tabular-nums">{formatMoney(line.lineTotal)}</Table.Cell>
                                    </Table.Row>
                                ))}
                            </Table.Body>
                        </Table>
                    </TableCard.Root>

                    <div className="flex flex-col items-end gap-2">
                        <Row label="Subtotal" value={formatMoney(view.sale.subtotal)} />
                        <Row label="Discount" value={formatMoney(view.sale.discountTotal)} />
                        <Row label="Tax" value={formatMoney(view.sale.taxTotal)} />
                        <Row label="Paid" value={formatMoney(view.sale.paidTotal)} />
                        <div className="flex w-64 items-center justify-between border-t border-secondary pt-2">
                            <span className="font-semibold text-primary">Total</span>
                            <span className="text-lg font-semibold tabular-nums text-primary">
                                {formatMoney(view.sale.grandTotal)}
                            </span>
                        </div>
                        {view.payments.length > 1 && (
                            <div className="flex w-64 flex-col gap-1">
                                <span className="text-sm text-tertiary">Paid by</span>
                                {view.payments.map((tender) => (
                                    <div key={tender.id} className="flex items-center justify-between text-sm">
                                        <span className="text-secondary">{tender.method}</span>
                                        <span className="tabular-nums text-primary">{formatMoney(tender.amount)}</span>
                                    </div>
                                ))}
                            </div>
                        )}

                        {/* Below the total means the customer walked out owing money, which
                            is a credit sale rather than a completed one. */}
                        {view.sale.paidTotal !== view.sale.grandTotal && (
                            <p className="w-64 text-right text-sm text-tertiary">
                                On account {formatMoney(view.sale.grandTotal)} − paid{" "}
                                {formatMoney(view.sale.paidTotal)}
                            </p>
                        )}
                    </div>
                </>
            )}
        </div>
    );
};

const Row = ({ label, value }: { label: string; value: string }) => (
    <div className="flex w-64 items-center justify-between">
        <span className="text-tertiary">{label}</span>
        <span className="tabular-nums text-primary">{value}</span>
    </div>
);