import { useCallback, useEffect, useState } from "react";
import { ArrowRight, Plus } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { Table, TableCard } from "@/components/application/table/table";
import { formatMoney, formatTimestamp, toDecimal } from "@/app/format";
import type { InstallmentSaleView, InstallmentSummary } from "@/app/ipc";
import {
    collectInstallmentPayment,
    createInstallmentSale,
    getInstallmentSale,
    listCustomers,
    listInstallments,
    listItems,
} from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

/** Display `1.234,50` back into the `1234.500` the wire wants. */
const normalizeMoney = (raw: string): string => raw.replace(/\./g, "").replace(",", ".");

const PAYMENT_METHODS: SelectItemType[] = [
    { id: "Cash", label: "Cash" },
    { id: "Card", label: "Card" },
    { id: "Qris", label: "QRIS" },
];

/**
 * Credit sales: the goods leave now, the balance arrives over dated dues.
 * Paid and due are derived server-side — this screen displays, never computes.
 */
export const Installments = () => {
    const [rows, setRows] = useState<InstallmentSummary[]>([]);
    const [total, setTotal] = useState(0);
    const [page, setPage] = useState(1);
    const [error, setError] = useState<string | null>(null);

    const [formOpen, setFormOpen] = useState(false);
    const [formError, setFormError] = useState<string | null>(null);
    const [customers, setCustomers] = useState<SelectItemType[]>([]);
    const [customerKey, setCustomerKey] = useState("");
    const [itemOptions, setItemOptions] = useState<SelectItemType[]>([]);
    const [itemKey, setItemKey] = useState("");
    const [quantity, setQuantity] = useState("1");
    const [unitPrice, setUnitPrice] = useState("");
    const [discount, setDiscount] = useState("");
    const [interest, setInterest] = useState("");
    const [otherCharges, setOtherCharges] = useState("");
    const [downPayment, setDownPayment] = useState("");
    const [downMethod, setDownMethod] = useState("Cash");
    const [dueCount, setDueCount] = useState("3");
    const [intervalDays, setIntervalDays] = useState("30");
    const [note, setNote] = useState("");

    const [detail, setDetail] = useState<InstallmentSaleView | null>(null);
    const [detailError, setDetailError] = useState<string | null>(null);
    const [collectAmounts, setCollectAmounts] = useState<Record<number, string>>({});
    const [collectMethods, setCollectMethods] = useState<Record<number, string>>({});

    const perPage = 20;

    const refresh = useCallback(async () => {
        try {
            const found = await listInstallments(null, { page, perPage });
            setRows(found.rows);
            setTotal(found.total);
            setError(null);
        } catch (cause) {
            setError(messageOf(cause));
        }
    }, [page]);

    useEffect(() => {
        void refresh();
    }, [refresh]);

    const findCustomers = useCallback(async (term: string) => {
        try {
            const found = await listCustomers({ page: 1, perPage: 20, search: term || undefined });
            setCustomers(found.rows.map((row) => ({ id: String(row.id), label: row.name })));
        } catch {
            setCustomers([]);
        }
    }, []);

    const findItems = useCallback(async (term: string) => {
        try {
            const found = await listItems({ page: 1, perPage: 20, search: term || undefined });
            setItemOptions(
                found.rows.map((row) => ({ id: String(row.id), label: `${row.name} · ${formatMoney(row.salePrice)}` })),
            );
        } catch {
            setItemOptions([]);
        }
    }, []);

    const openCreate = useCallback(() => {
        setCustomerKey("");
        setItemKey("");
        setQuantity("1");
        setUnitPrice("");
        setDiscount("");
        setInterest("");
        setOtherCharges("");
        setDownPayment("");
        setDownMethod("Cash");
        setDueCount("3");
        setIntervalDays("30");
        setNote("");
        setFormError(null);
        setFormOpen(true);
    }, []);

    const moneyOrNull = (raw: string): string | null => {
        const trimmed = raw.trim();
        return trimmed === "" ? null : toDecimal(normalizeMoney(trimmed));
    };

    const submit = useCallback(async () => {
        setFormError(null);
        try {
            const view = await createInstallmentSale({
                customerId: Number(customerKey),
                itemId: Number(itemKey),
                quantity: toDecimal(normalizeMoney(quantity.trim() === "" ? "0" : quantity)),
                unitPrice: toDecimal(normalizeMoney(unitPrice.trim() === "" ? "0" : unitPrice)),
                discountAmount: moneyOrNull(discount),
                interestPercent: moneyOrNull(interest),
                otherCharges: moneyOrNull(otherCharges),
                downPayment: moneyOrNull(downPayment),
                downPaymentMethod: downPayment.trim() === "" ? null : downMethod,
                numberOfInstallments: Number(dueCount),
                intervalDays: intervalDays.trim() === "" ? null : Number(intervalDays),
                note: note.trim() === "" ? null : note.trim(),
            });
            setFormOpen(false);
            setDetail(view);
            setDetailError(null);
            setCollectAmounts({});
            setCollectMethods({});
            void refresh();
        } catch (cause) {
            setFormError(messageOf(cause));
        }
    }, [customerKey, itemKey, quantity, unitPrice, discount, interest, otherCharges, downPayment, downMethod, dueCount, intervalDays, note, refresh]);

    const openDetail = useCallback(async (id: number) => {
        setDetailError(null);
        try {
            const view = await getInstallmentSale(id);
            setDetail(view);
            setCollectAmounts({});
            setCollectMethods({});
        } catch (cause) {
            setDetailError(messageOf(cause));
        }
    }, []);

    const collect = useCallback(
        async (dueId: number, outstanding: string) => {
            setDetailError(null);
            try {
                const raw = (collectAmounts[dueId] ?? "").trim();
                const view = await collectInstallmentPayment(
                    dueId,
                    toDecimal(normalizeMoney(raw === "" ? outstanding : raw)),
                    collectMethods[dueId] ?? "Cash",
                );
                setDetail(view);
                setCollectAmounts((current) => ({ ...current, [dueId]: "" }));
                void refresh();
            } catch (cause) {
                setDetailError(messageOf(cause));
            }
        },
        [collectAmounts, collectMethods, refresh],
    );

    const pages = Math.max(1, Math.ceil(total / perPage));

    return (
        <div className="flex flex-col gap-5 p-5">
            <div className="flex flex-col gap-1">
                <h1 className="text-display-xs font-semibold text-primary">Installment sales</h1>
                <p className="text-md text-tertiary">Goods out now, the balance arriving over dated dues.</p>
            </div>

            <div>
                <Button color="primary" iconLeading={Plus} onPress={openCreate}>
                    New installment sale
                </Button>
            </div>

            {error && <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{error}</p>}

            {rows.length === 0 ? (
                <p className="py-10 text-center text-md text-tertiary">No installment sales yet.</p>
            ) : (
                <TableCard.Root>
                    <TableCard.Header title="Installment sales" description={`${total} total`} />
                    <Table aria-label="Installment sales">
                        <Table.Header>
                            <Table.Head label="Reference" />
                            <Table.Head label="Customer" />
                            <Table.Head label="Item" />
                            <Table.Head label="Total" />
                            <Table.Head label="Paid" />
                            <Table.Head label="Due" />
                            <Table.Head label="Status" />
                            <Table.Head label="" className="w-12" />
                        </Table.Header>
                        <Table.Body>
                            {rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium tabular-nums text-primary">
                                        {row.referenceNo}
                                    </Table.Cell>
                                    <Table.Cell>{row.customerName ?? "—"}</Table.Cell>
                                    <Table.Cell>{row.itemName}</Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatMoney(row.total)}</Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatMoney(row.paidTotal)}</Table.Cell>
                                    <Table.Cell className="tabular-nums">{formatMoney(row.dueTotal)}</Table.Cell>
                                    <Table.Cell className="text-tertiary">{row.status}</Table.Cell>
                                    <Table.Cell>
                                        <Button
                                            size="sm"
                                            color="tertiary"
                                            iconLeading={ArrowRight}
                                            aria-label={`Open ${row.referenceNo}`}
                                            onPress={() => void openDetail(row.id)}
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

            {formOpen && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setFormOpen(false)}>
                    <Modal className="max-w-2xl">
                        <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">New installment sale</h2>

                                {formError !== null && (
                                    <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                        {formError}
                                    </p>
                                )}

                                <Select
                                    label="Customer"
                                    items={customers}
                                    selectedKey={customerKey}
                                    onOpenChange={(open) => open && void findCustomers("")}
                                    onSelectionChange={(key) => setCustomerKey(String(key ?? ""))}
                                    isRequired
                                >
                                    {(row) => (
                                        <Select.Item id={row.id} textValue={row.label}>
                                            {row.label}
                                        </Select.Item>
                                    )}
                                </Select>

                                <Select
                                    label="Item"
                                    items={itemOptions}
                                    selectedKey={itemKey}
                                    onOpenChange={(open) => open && void findItems("")}
                                    onSelectionChange={(key) => setItemKey(String(key ?? ""))}
                                    isRequired
                                >
                                    {(row) => (
                                        <Select.Item id={row.id} textValue={row.label}>
                                            {row.label}
                                        </Select.Item>
                                    )}
                                </Select>

                                <div className="grid grid-cols-2 gap-3">
                                    <Input label="Quantity" value={quantity} onChange={setQuantity} />
                                    <Input label="Unit price" value={unitPrice} onChange={setUnitPrice} />
                                    <Input label="Discount" hint="Fixed amount off" value={discount} onChange={setDiscount} />
                                    <Input label="Interest %" hint="On the discounted price" value={interest} onChange={setInterest} />
                                    <Input label="Other charges" value={otherCharges} onChange={setOtherCharges} />
                                    <Input label="Dues" hint="How many payments" value={dueCount} onChange={setDueCount} />
                                    <Input
                                        label="Down payment"
                                        value={downPayment}
                                        onChange={setDownPayment}
                                    />
                                    <Select
                                        label="Down payment method"
                                        items={PAYMENT_METHODS}
                                        selectedKey={downMethod}
                                        onSelectionChange={(key) => setDownMethod(String(key ?? "Cash"))}
                                    >
                                        {(row) => (
                                            <Select.Item id={row.id} textValue={row.label}>
                                                {row.label}
                                            </Select.Item>
                                        )}
                                    </Select>
                                </div>

                                <div className="grid grid-cols-2 gap-3">
                                    <Input
                                        label="Days between dues"
                                        hint="Blank means 30"
                                        value={intervalDays}
                                        onChange={setIntervalDays}
                                    />
                                    <Input label="Note" value={note} onChange={setNote} />
                                </div>

                                <p className="text-sm text-tertiary">
                                    The total, the interest and the dues are derived server-side — the
                                    schedule splits the balance across {dueCount.trim() === "" ? "…" : dueCount} dues.
                                </p>

                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setFormOpen(false)}>
                                        Cancel
                                    </Button>
                                    <Button
                                        color="primary"
                                        isDisabled={customerKey === "" || itemKey === ""}
                                        onPress={() => void submit()}
                                    >
                                        Create installment sale
                                    </Button>
                                </div>
                            </div>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}

            {detail !== null && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setDetail(null)}>
                    <Modal className="max-w-2xl">
                        <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                            <div className="flex flex-col gap-4">
                                <div>
                                    <h2 className="text-display-xs font-semibold text-primary">
                                        {detail.sale.referenceNo}
                                    </h2>
                                    <p className="text-md text-tertiary">
                                        {detail.customerName ?? "—"} · {detail.itemName} · {detail.status}
                                    </p>
                                    <p className="text-md tabular-nums text-tertiary">
                                        Total {formatMoney(detail.sale.total)} · Paid {formatMoney(detail.paidTotal)} ·
                                        Due {formatMoney(detail.dueTotal)}
                                    </p>
                                </div>

                                {detailError !== null && (
                                    <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                        {detailError}
                                    </p>
                                )}

                                <TableCard.Root>
                                    <TableCard.Header title="Dues" description={`${detail.details.length}`} />
                                    <Table aria-label="Dues">
                                        <Table.Header>
                                            <Table.Head label="Due" />
                                            <Table.Head label="Amount" />
                                            <Table.Head label="Paid" />
                                            <Table.Head label="Status" />
                                            <Table.Head label="Collect" />
                                        </Table.Header>
                                        <Table.Body>
                                            {detail.details.map((due) => (
                                                <Table.Row key={due.id} id={due.id}>
                                                    <Table.Cell className="text-tertiary">
                                                        {formatTimestamp(due.dueDate)}
                                                    </Table.Cell>
                                                    <Table.Cell className="tabular-nums">
                                                        {formatMoney(due.amount)}
                                                    </Table.Cell>
                                                    <Table.Cell className="tabular-nums">
                                                        {formatMoney(due.paidAmount)}
                                                    </Table.Cell>
                                                    <Table.Cell className="text-tertiary">{due.paidStatus}</Table.Cell>
                                                    <Table.Cell>
                                                        {due.paidStatus === "Paid" ? (
                                                            <span className="text-sm text-tertiary">—</span>
                                                        ) : (
                                                            <div className="flex items-end gap-2">
                                                                <Input
                                                                    aria-label={`Collect amount for due ${formatTimestamp(due.dueDate)}`}
                                                                    placeholder={due.remainingAmount}
                                                                    value={collectAmounts[due.id] ?? ""}
                                                                    onChange={(value) =>
                                                                        setCollectAmounts((current) => ({
                                                                            ...current,
                                                                            [due.id]: String(value),
                                                                        }))
                                                                    }
                                                                    className="w-28"
                                                                />
                                                                <Select
                                                                    aria-label={`Method for due ${formatTimestamp(due.dueDate)}`}
                                                                    items={PAYMENT_METHODS}
                                                                    selectedKey={collectMethods[due.id] ?? "Cash"}
                                                                    onSelectionChange={(key) =>
                                                                        setCollectMethods((current) => ({
                                                                            ...current,
                                                                            [due.id]: String(key ?? "Cash"),
                                                                        }))
                                                                    }
                                                                    className="w-28"
                                                                >
                                                                    {(row) => (
                                                                        <Select.Item id={row.id} textValue={row.label}>
                                                                            {row.label}
                                                                        </Select.Item>
                                                                    )}
                                                                </Select>
                                                                <Button
                                                                    size="sm"
                                                                    color="primary"
                                                                    onPress={() => void collect(due.id, due.remainingAmount)}
                                                                >
                                                                    Collect
                                                                </Button>
                                                            </div>
                                                        )}
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
