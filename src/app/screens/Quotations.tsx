import { useCallback, useEffect, useState } from "react";
import { Plus, Trash01 } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { Table, TableCard } from "@/components/application/table/table";
import { formatMoney, formatTimestamp, toDecimal } from "@/app/format";
import { decAdd, decMul } from "@/app/decimal";
import type { Decimal, QuotationInput, QuotationLine, QuotationView } from "@/app/ipc";
import {
    createQuotation,
    deleteQuotation,
    getQuotation,
    listCustomers,
    listItems,
    listQuotations,
    updateQuotation,
} from "@/app/ipc";

type Loaded =
    | { kind: "loading" }
    | { kind: "ready"; rows: QuotationView[]; total: number }
    | { kind: "failed"; message: string };

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

const normalizeMoney = (raw: string): string => raw.replace(/\./g, "").replace(",", ".");

interface FormLine extends Omit<QuotationLine, "discount"> {
    key: number;
    name: string;
    discount: Decimal | null;
}

/**
 * Price offers to customers. Nothing here moves stock or money — that is what
 * separates a quotation from a draft, which is a sale waiting to happen.
 */
export const Quotations = () => {
    const [state, setState] = useState<Loaded>({ kind: "loading" });
    const [reload, setReload] = useState(0);
    const [formOpen, setFormOpen] = useState(false);
    const [editingId, setEditingId] = useState<number | null>(null);
    const [discarding, setDiscarding] = useState<QuotationView | null>(null);
    const [formError, setFormError] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);

    const [customerKey, setCustomerKey] = useState("");
    const [customers, setCustomers] = useState<SelectItemType[]>([]);
    const [quotedAt, setQuotedAt] = useState("");
    const [referenceNo, setReferenceNo] = useState("");
    const [discountTotal, setDiscountTotal] = useState("");
    const [note, setNote] = useState("");
    const [lines, setLines] = useState<FormLine[]>([]);
    const [itemTerm, setItemTerm] = useState("");
    const [itemOptions, setItemOptions] = useState<SelectItemType[]>([]);
    const [pickedItem, setPickedItem] = useState("");
    const [nextKey, setNextKey] = useState(1);

    const refresh = useCallback(() => setReload((n) => n + 1), []);

    useEffect(() => {
        let cancelled = false;
        setState({ kind: "loading" });

        listQuotations({ page: 1, perPage: 50 })
            .then((page) => {
                if (!cancelled) setState({ kind: "ready", rows: page.rows, total: page.total });
            })
            .catch((error: unknown) => {
                if (!cancelled) setState({ kind: "failed", message: messageOf(error) });
            });

        return () => {
            cancelled = true;
        };
    }, [reload]);

    // Customers are searched rather than listed: the form needs one, not all of them.
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
        setEditingId(null);
        setCustomerKey("");
        setQuotedAt("");
        setReferenceNo("");
        setDiscountTotal("");
        setNote("");
        setLines([]);
        setFormError(null);
        setFormOpen(true);
    }, []);

    const openEdit = useCallback(async (id: number) => {
        setFormError(null);
        try {
            const view = await getQuotation(id);
            setEditingId(id);
            setCustomerKey(String(view.customerId));
            setCustomers([{ id: String(view.customerId), label: view.customerName ?? String(view.customerId) }]);
            setQuotedAt(view.quotedAt.slice(0, 10));
            setReferenceNo(view.referenceNo ?? "");
            setDiscountTotal("");
            setNote(view.note ?? "");
                lines: view.lines.map((line, index) => ({
                    key: index + 1,
                    itemId: line.itemId,
                    name: line.itemName,
                    quantity: line.quantity,
                    unitPrice: line.unitPrice,
                    discount: line.discount === "0.000" ? null : (line.discount ?? null),
                })),
            setNextKey(view.lines.length + 1);
            setFormOpen(true);
        } catch (error) {
            setFormError(messageOf(error));
            setFormOpen(true);
        }
    }, []);

    // The order-level discount is intentionally blankable: the per-line discounts are
    // the common case and an empty box must not invent a discount.
    const totals = lines.reduce(
        (acc, line) => {
            const qty = line.quantity.trim() === "" ? "0" : toDecimal(normalizeMoney(line.quantity.trim()));
            const price = line.unitPrice.trim() === "" ? "0" : toDecimal(normalizeMoney(line.unitPrice.trim()));
            return { subtotal: decAdd(acc.subtotal, decMul(price, qty)), count: acc.count + 1 };
        },
        { subtotal: "0", count: 0 },
    );

    const addLine = useCallback(async () => {
        if (pickedItem === "") return;
        try {
            const found = await listItems({ page: 1, perPage: 20, search: undefined });
            const row = found.rows.find((r) => String(r.id) === pickedItem);
            if (!row) return;
            setLines((current) => [
                ...current,
                { key: nextKey, itemId: row.id, name: row.name, quantity: "1", unitPrice: row.salePrice, discount: null },
            ]);
            setNextKey((k) => k + 1);
            setPickedItem("");
            setItemTerm("");
        } catch (error) {
            setFormError(messageOf(error));
        }
    }, [pickedItem, nextKey]);

    const save = useCallback(async () => {
        if (customerKey === "" || lines.length === 0) return;
        setSaving(true);
        setFormError(null);
        try {
            const input: QuotationInput = {
                customerId: Number(customerKey),
                quotedAt: quotedAt.trim() === "" ? null : quotedAt.trim(),
                referenceNo: referenceNo.trim() === "" ? null : referenceNo.trim(),
                discountTotal: discountTotal.trim() === "" ? null : toDecimal(normalizeMoney(discountTotal.trim())),
                note: note.trim() === "" ? null : note.trim(),
                lines: lines.map((line) => ({
                    itemId: line.itemId,
                    quantity: toDecimal(normalizeMoney(line.quantity.trim() === "" ? "0" : line.quantity.trim())),
                    unitPrice: toDecimal(normalizeMoney(line.unitPrice.trim() === "" ? "0" : line.unitPrice.trim())),
                    discount:
                        line.discount === null || line.discount.trim() === ""
                            ? null
                            : toDecimal(normalizeMoney(line.discount.trim())),
                })),
            };
            if (editingId === null) {
                await createQuotation(input);
            } else {
                await updateQuotation(editingId, input);
            }
            setFormOpen(false);
            refresh();
        } catch (error) {
            setFormError(messageOf(error));
        } finally {
            setSaving(false);
        }
    }, [customerKey, quotedAt, referenceNo, discountTotal, note, lines, editingId, refresh]);

    const confirmDiscard = useCallback(async () => {
        if (discarding === null) return;
        const id = discarding.id;
        setDiscarding(null);
        try {
            await deleteQuotation(id);
            refresh();
        } catch (error) {
            setState({ kind: "failed", message: messageOf(error) });
        }
    }, [discarding, refresh]);

    return (
        <div className="flex flex-col gap-6 p-6 md:p-8">
            <TableCard.Root>
                <TableCard.Header
                    title="Quotations"
                    description={state.kind === "ready" ? `${state.total} offer${state.total === 1 ? "" : "s"}` : undefined}
                    contentTrailing={
                        <Button size="sm" iconLeading={Plus} onPress={openCreate}>
                            New quotation
                        </Button>
                    }
                />

                {state.kind === "failed" ? (
                    <p className="px-6 py-12 text-center text-md text-error-primary">{state.message}</p>
                ) : state.kind === "loading" ? (
                    <p className="px-6 py-12 text-center text-md text-tertiary">Loading…</p>
                ) : state.rows.length === 0 ? (
                    <p className="px-6 py-12 text-center text-md text-tertiary">No quotations yet.</p>
                ) : (
                    <Table>
                        <Table.Header>
                            <Table.Head label="Number" />
                            <Table.Head label="Customer" />
                            <Table.Head label="Quoted" />
                            <Table.Head label="Total" className="text-right" />
                            <Table.Head label="" />
                        </Table.Header>
                        <Table.Body>
                            {state.rows.map((row) => (
                                <Table.Row key={row.id} id={row.id}>
                                    <Table.Cell className="font-medium text-primary">{row.quotationNo}</Table.Cell>
                                    <Table.Cell>{row.customerName ?? "—"}</Table.Cell>
                                    <Table.Cell className="text-tertiary">{formatTimestamp(row.quotedAt)}</Table.Cell>
                                    <Table.Cell className="text-right font-medium text-primary">
                                        {formatMoney(row.grandTotal)}
                                    </Table.Cell>
                                    <Table.Cell>
                                        <div className="flex justify-end gap-1">
                                            <Button size="sm" color="secondary" onPress={() => void openEdit(row.id)}>
                                                Edit
                                            </Button>
                                            <Button
                                                size="sm"
                                                color="tertiary"
                                                iconLeading={Trash01}
                                                aria-label={`Delete ${row.quotationNo}`}
                                                onPress={() => setDiscarding(row)}
                                            />
                                        </div>
                                    </Table.Cell>
                                </Table.Row>
                            ))}
                        </Table.Body>
                    </Table>
                )}
            </TableCard.Root>

            {formOpen && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setFormOpen(false)}>
                    <Modal className="max-w-2xl">
                        <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">
                                    {editingId === null ? "New quotation" : "Edit quotation"}
                                </h2>

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

                                <div className="grid grid-cols-2 gap-3">
                                    <Input label="Quoted date" type="date" value={quotedAt} onChange={setQuotedAt} />
                                    <Input label="Reference" value={referenceNo} onChange={setReferenceNo} />
                                </div>

                                <div className="flex flex-col gap-2">
                                    <span className="text-sm font-medium text-primary">Lines</span>
                                    {lines.map((line) => (
                                        <div key={line.key} className="grid grid-cols-[1fr_5rem_7rem_6rem_auto] items-end gap-2">
                                            <span className="truncate text-sm text-primary">{line.name}</span>
                                            <Input
                                                label="Qty"
                                                aria-label={`Quantity for ${line.name}`}
                                                value={line.quantity}
                                                onChange={(value) =>
                                                    setLines((current) =>
                                                        current.map((l) =>
                                                            l.key === line.key ? { ...l, quantity: String(value) } : l,
                                                        ),
                                                    )
                                                }
                                            />
                                            <Input
                                                label="Price"
                                                aria-label={`Unit price for ${line.name}`}
                                                value={line.unitPrice}
                                                onChange={(value) =>
                                                    setLines((current) =>
                                                        current.map((l) =>
                                                            l.key === line.key ? { ...l, unitPrice: String(value) } : l,
                                                        ),
                                                    )
                                                }
                                            />
                                            <Input
                                                label="Discount"
                                                aria-label={`Discount for ${line.name}`}
                                                value={line.discount ?? ""}
                                                onChange={(value) =>
                                                    setLines((current) =>
                                                        current.map((l) =>
                                                            l.key === line.key ? { ...l, discount: String(value) } : l,
                                                        ),
                                                    )
                                                }
                                            />
                                            <Button
                                                size="sm"
                                                color="tertiary"
                                                iconLeading={Trash01}
                                                aria-label={`Remove ${line.name}`}
                                                onPress={() =>
                                                    setLines((current) => current.filter((l) => l.key !== line.key))
                                                }
                                            />
                                        </div>
                                    ))}
                                    <div className="flex items-end gap-2">
                                        <Input
                                            label="Add item"
                                            placeholder="Search the catalog"
                                            value={itemTerm}
                                            onChange={(value) => {
                                                setItemTerm(String(value));
                                                void findItems(String(value));
                                            }}
                                            onFocus={() => void findItems(itemTerm)}
                                            className="w-full"
                                        />
                                        <Select
                                            label="Match"
                                            aria-label="Matching item"
                                            items={itemOptions}
                                            selectedKey={pickedItem}
                                            onOpenChange={(open) => open && void findItems(itemTerm)}
                                            onSelectionChange={(key) => setPickedItem(String(key ?? ""))}
                                            className="w-56"
                                        >
                                            {(row) => (
                                                <Select.Item id={row.id} textValue={row.label}>
                                                    {row.label}
                                                </Select.Item>
                                            )}
                                        </Select>
                                        <Button size="sm" color="secondary" isDisabled={pickedItem === ""} onPress={() => void addLine()}>
                                            Add
                                        </Button>
                                    </div>
                                </div>

                                <div className="grid grid-cols-2 gap-3">
                                    <Input
                                        label="Order discount"
                                        placeholder="Blank means none"
                                        value={discountTotal}
                                        onChange={setDiscountTotal}
                                    />
                                    <Input label="Note" value={note} onChange={setNote} />
                                </div>

                                <p className="text-sm text-tertiary">
                                    Subtotal {formatMoney(totals.subtotal)} across {totals.count}{" "}
                                    {totals.count === 1 ? "line" : "lines"}. The server recomputes the
                                    grand total from these lines.
                                </p>

                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setFormOpen(false)}>
                                        Cancel
                                    </Button>
                                    <Button
                                        onPress={() => void save()}
                                        isLoading={saving}
                                        isDisabled={customerKey === "" || lines.length === 0}
                                    >
                                        {editingId === null ? "Create quotation" : "Save changes"}
                                    </Button>
                                </div>
                            </div>
                        </Dialog>
                    </Modal>
                </ModalOverlay>
            )}

            {discarding && (
                <ModalOverlay isOpen onOpenChange={(open) => !open && setDiscarding(null)}>
                    <Modal className="max-w-md">
                        <Dialog className="p-6">
                            <div className="flex flex-col gap-4">
                                <h2 className="text-display-xs font-semibold text-primary">
                                    Delete {discarding.quotationNo}?
                                </h2>
                                <p className="text-md text-secondary">
                                    An offer carries no money and moves no stock, so there is nothing
                                    to account for. It cannot be recovered afterwards.
                                </p>
                                <div className="flex justify-end gap-2">
                                    <Button color="secondary" onPress={() => setDiscarding(null)}>
                                        Cancel
                                    </Button>
                                    <Button color="primary-destructive" onPress={confirmDiscard}>
                                        Delete
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
