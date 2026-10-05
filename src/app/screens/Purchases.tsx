import { useCallback, useEffect, useState } from "react";
import { Plus, SearchLg } from "@untitledui/icons";
import { Table, TableCard } from "@/components/application/table/table";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { decIsNegative } from "@/app/decimal";
import { formatMoney, formatQuantity, formatTimestamp, toDecimal } from "@/app/format";
import { listPaymentMethods, listPurchases, getPurchase, recordPurchasePayment, createPurchaseReturn } from "@/app/ipc";
import type { PaymentMethod, PurchaseView } from "@/app/ipc";
import { PurchaseForm } from "./PurchaseForm";
import { PurchaseReturns } from "./PurchaseReturns";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

export const Purchases = () => {
    const [rows, setRows] = useState<PurchaseView[]>([]);
    const [total, setTotal] = useState(0);
    const [page, setPage] = useState(1);
    const [term, setTerm] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [reload, setReload] = useState(0);

    const [receiving, setReceiving] = useState(false);
    const [openId, setOpenId] = useState<number | null>(null);
    const [showReturns, setShowReturns] = useState(false);

    const perPage = 20;

    const refresh = useCallback(async () => {
        try {
            const result = await listPurchases({
                page,
                perPage,
                search: term.trim() || undefined,
            });
            setRows(result.rows);
            setTotal(result.total);
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, [page, term]);

    useEffect(() => {
        void refresh();
    }, [refresh, reload]);

    const pages = Math.max(1, Math.ceil(total / perPage));

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
                <div className="flex flex-col gap-1">
                    <h1 className="text-display-xs font-semibold text-primary">Purchases</h1>
                    <p className="text-md text-tertiary">
                        Goods received from suppliers. Each one puts stock on the shelf, and is
                        corrected by a return rather than an edit.
                    </p>
                </div>
                <div className="flex w-full flex-col gap-3 md:w-auto md:flex-row">
                    <Input
                        aria-label="Search purchases"
                        icon={SearchLg}
                        placeholder="Search reference, invoice or supplier"
                        value={term}
                        onChange={(value) => {
                            setPage(1);
                            setTerm(String(value));
                        }}
                        className="w-full md:w-72"
                    />
                    <Button size="md" color="secondary" onPress={() => setShowReturns(true)}>
                        Returns
                    </Button>
                    <Button size="md" iconLeading={Plus} onPress={() => setReceiving(true)}>
                        Receive goods
                    </Button>
                </div>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">
                    {term ? "No purchases match that search." : "No goods received yet."}
                </p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Purchases" description={`${total} total`} />
                    <Table aria-label="Purchases">
                        <Table.Header>
                            <Table.Head label="Reference" />
                            <Table.Head label="Supplier" />
                            <Table.Head label="Date" />
                            <Table.Head label="Total" className="text-right" />
                            <Table.Head label="Paid" className="text-right" />
                            <Table.Head label="Due" className="text-right" />
                            <Table.Head label="Status" />
                            <Table.Head label="" className="w-28" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium text-primary">
                                        {row.referenceNo}
                                    </Table.Cell>
                                    <Table.Cell>{row.supplierName}</Table.Cell>
                                    <Table.Cell className="text-tertiary">{row.purchasedAt}</Table.Cell>
                                    <Table.Cell className="text-right">{formatMoney(row.grandTotal)}</Table.Cell>
                                    <Table.Cell className="text-right">{formatMoney(row.paidTotal)}</Table.Cell>
                                    <Table.Cell
                                        className={`text-right ${decIsNegative(row.dueTotal) ? "text-error-primary" : ""}`}
                                    >
                                        {formatMoney(row.dueTotal)}
                                    </Table.Cell>
                                    <Table.Cell>{row.status}</Table.Cell>
                                    <Table.Cell>
                                        <Button size="sm" color="secondary" onPress={() => setOpenId(row.id)}>
                                            Open
                                        </Button>
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
                    <Button color="secondary" size="sm" isDisabled={page >= pages} onPress={() => setPage((n) => n + 1)}>
                        Next
                    </Button>
                </div>
            )}

            {receiving && (
                <PurchaseForm
                    onCancel={() => setReceiving(false)}
                    onSaved={() => {
                        setReceiving(false);
                        setPage(1);
                        setReload((n) => n + 1);
                    }}
                />
            )}

            {showReturns && (
                <PurchaseReturns onClose={() => setShowReturns(false)} />
            )}

            {openId !== null && (
                <PurchaseDetail
                    purchaseId={openId}
                    onClose={() => setOpenId(null)}
                    onChanged={() => setReload((n) => n + 1)}
                />
            )}
        </div>
    );
};

const PurchaseDetail = ({
    purchaseId,
    onClose,
    onChanged,
}: {
    purchaseId: number;
    onClose: () => void;
    onChanged: () => void;
}) => {
    const [purchase, setPurchase] = useState<PurchaseView | null>(null);
    const [methods, setMethods] = useState<PaymentMethod[]>([]);
    const [tenderId, setTenderId] = useState("");
    const [amount, setAmount] = useState("");
    const [returning, setReturning] = useState<Record<number, string>>({});
    const [error, setError] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);

    const load = useCallback(async () => {
        try {
            const [detail, methodList] = await Promise.all([getPurchase(purchaseId), listPaymentMethods()]);
            setPurchase(detail);
            setMethods(methodList);
            setTenderId((current) => current || (methodList[0] ? String(methodList[0].id) : ""));
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, [purchaseId]);

    useEffect(() => {
        void load();
    }, [load]);

    const pay = async () => {
        setBusy(true);
        setError(null);
        try {
            setPurchase(
                await recordPurchasePayment(purchaseId, {
                    paymentMethodId: Number(tenderId),
                    amount: toDecimal(amount),
                }),
            );
            setAmount("");
            onChanged();
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    };

    const sendBack = async () => {
        if (!purchase) return;
        setBusy(true);
        setError(null);
        try {
            await createPurchaseReturn({
                purchaseId: purchase.id,
                returnedAt: new Date().toISOString().slice(0, 10),
                lines: purchase.lines
                    .filter((line) => (returning[line.id] ?? "").trim() !== "")
                    .map((line) => ({
                        purchaseDetailId: line.id,
                        quantity: toDecimal(returning[line.id]),
                    })),
            });
            setReturning({});
            await load();
            onChanged();
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    };

    const returnable = purchase?.lines.some((line) => (returning[line.id] ?? "").trim() !== "") ?? false;

    return (
        <ModalOverlay isOpen onOpenChange={(open) => !open && onClose()}>
            <Modal className="max-w-3xl">
                <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                    <div className="flex flex-col gap-4">
                        <h2 className="text-display-xs font-semibold text-primary">
                            {purchase ? purchase.referenceNo : "Purchase"}
                        </h2>

                        {error && (
                            <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                {error}
                            </p>
                        )}

                        {purchase && (
                            <>
                                <div className="grid grid-cols-2 gap-2 text-sm">
                                    <span className="text-tertiary">Supplier</span>
                                    <span>{purchase.supplierName}</span>
                                    <span className="text-tertiary">Date</span>
                                    <span>{purchase.purchasedAt}</span>
                                    <span className="text-tertiary">Supplier invoice</span>
                                    <span>{purchase.supplierInvoiceNo ?? "—"}</span>
                                    <span className="text-tertiary">Subtotal</span>
                                    <span>{formatMoney(purchase.subtotal)}</span>
                                    <span className="text-tertiary">Discount</span>
                                    <span>{formatMoney(purchase.discount)}</span>
                                    <span className="text-tertiary">Total</span>
                                    <span>{formatMoney(purchase.grandTotal)}</span>
                                    <span className="text-tertiary">Paid</span>
                                    <span>{formatMoney(purchase.paidTotal)}</span>
                                    <span className="text-tertiary">Due</span>
                                    <span className={decIsNegative(purchase.dueTotal) ? "text-error-primary" : ""}>
                                        {formatMoney(purchase.dueTotal)}
                                    </span>
                                </div>

                                <Table aria-label="Lines">
                                    <Table.Header>
                                        <Table.Head label="Item" />
                                        <Table.Head label="Batch" />
                                        <Table.Head label="Quantity" className="text-right" />
                                        <Table.Head label="Unit price" className="text-right" />
                                        <Table.Head label="Total" className="text-right" />
                                        <Table.Head label="Returned" className="text-right" />
                                        <Table.Head label="Return" />
                                    </Table.Header>
                                    <Table.Body>
                                        {purchase.lines.map((line) => (
                                            <Table.Row key={line.id} id={line.id}>
                                                <Table.Cell>
                                                    {line.itemName} ({line.itemCode})
                                                </Table.Cell>
                                                <Table.Cell className="text-tertiary">
                                                    {line.batchNo ?? "—"}
                                                </Table.Cell>
                                                <Table.Cell className="text-right">
                                                    {formatQuantity(line.quantity)}
                                                </Table.Cell>
                                                <Table.Cell className="text-right">
                                                    {formatMoney(line.unitPrice)}
                                                </Table.Cell>
                                                <Table.Cell className="text-right">{formatMoney(line.total)}</Table.Cell>
                                                <Table.Cell className="text-right">
                                                    {formatQuantity(line.returnedQuantity)}
                                                </Table.Cell>
                                                <Table.Cell>
                                                    <Input
                                                        aria-label={`Quantity of ${line.itemName} to return`}
                                                        placeholder="0"
                                                        value={returning[line.id] ?? ""}
                                                        onChange={(value) =>
                                                            setReturning((current) => ({
                                                                ...current,
                                                                [line.id]: String(value),
                                                            }))
                                                        }
                                                    />
                                                </Table.Cell>
                                            </Table.Row>
                                        ))}
                                    </Table.Body>
                                </Table>

                                <Button size="sm" color="secondary" isDisabled={!returnable || busy} onPress={() => void sendBack()}>
                                    Send selected back to supplier
                                </Button>

                                {purchase.payments.length > 0 && (
                                    <div className="flex flex-col gap-1 text-sm">
                                        <span className="text-tertiary">Payments</span>
                                        {purchase.payments.map((payment) => (
                                            <span key={payment.id}>
                                                {formatTimestamp(payment.paidAt)} —{" "}
                                                {payment.paymentMethodName ?? "—"} {formatMoney(payment.amount)}
                                            </span>
                                        ))}
                                    </div>
                                )}

                                <div className="grid grid-cols-3 items-end gap-3">
                                    <Select
                                        label="Pay with"
                                        selectedKey={tenderId}
                                        onSelectionChange={(key) => setTenderId(String(key))}
                                    >
                                        {methods.map((method) => (
                                            <Select.Item key={String(method.id)} id={String(method.id)}>
                                                {method.name}
                                            </Select.Item>
                                        ))}
                                    </Select>
                                    <Input label="Amount" value={amount} onChange={setAmount} />
                                    <Button isLoading={busy} isDisabled={amount.trim() === ""} onPress={() => void pay()}>
                                        Record payment
                                    </Button>
                                </div>
                            </>
                        )}

                        <div className="flex justify-end">
                            <Button color="secondary" onPress={onClose}>
                                Close
                            </Button>
                        </div>
                    </div>
                </Dialog>
            </Modal>
        </ModalOverlay>
    );
};