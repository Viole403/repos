import { useEffect, useMemo, useState } from "react";
import { Plus, Trash01 } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import { Dialog, Modal, ModalOverlay } from "@/components/application/modals/modal";
import { decMul, decSum } from "@/app/decimal";
import { formatMoney, toDecimal } from "@/app/format";
import { createPurchase, listItems, listPaymentMethods, listSuppliers } from "@/app/ipc";
import type { ItemView, PaymentMethod, PurchaseInput, PurchaseLineInput, PurchasePaymentInput, SupplierView } from "@/app/ipc";

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

/** Blank text becomes null rather than an empty string the server has to re-trim. */
const optionalText = (raw: string): string | null => (raw.trim() === "" ? null : raw.trim());

/** One editable line. Quantities are not comma-normalised on purpose: stripping a
    dot from `1.234` would silently inflate the quantity received. */
interface DraftLine {
    key: number;
    itemId: string;
    quantity: string;
    unitPrice: string;
    batchNo: string;
    expiryDate: string;
}

const draftLine = (key: number): DraftLine => ({
    key,
    itemId: "",
    quantity: "1",
    unitPrice: "",
    batchNo: "",
    expiryDate: "",
});

const today = () => new Date().toISOString().slice(0, 10);

export const PurchaseForm = ({ onSaved, onCancel }: { onSaved: () => void; onCancel: () => void }) => {
    const [suppliers, setSuppliers] = useState<SupplierView[]>([]);
    const [items, setItems] = useState<ItemView[]>([]);
    const [methods, setMethods] = useState<PaymentMethod[]>([]);

    const [supplierId, setSupplierId] = useState("");
    const [purchasedAt, setPurchasedAt] = useState(today());
    const [invoiceNo, setInvoiceNo] = useState("");
    const [discount, setDiscount] = useState("");
    const [note, setNote] = useState("");
    const [lines, setLines] = useState<DraftLine[]>([draftLine(1)]);

    const [itemSearch, setItemSearch] = useState("");
    const [tenderId, setTenderId] = useState("");
    const [tendered, setTendered] = useState("");

    const [error, setError] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);
    const [nextKey, setNextKey] = useState(2);

    useEffect(() => {
        const load = async () => {
            try {
                const [supplierPage, itemPage, methodList] = await Promise.all([
                    listSuppliers({ page: 1, perPage: 500 }),
                    listItems({ page: 1, perPage: 500, search: itemSearch.trim() || undefined }),
                    listPaymentMethods(),
                ]);
                setSuppliers(supplierPage.rows);
                setItems(itemPage.rows);
                setMethods(methodList);
                setTenderId((current) => current || (methodList[0] ? String(methodList[0].id) : ""));
            } catch (cause) {
                setError(messageOf(cause));
            }
        };
        void load();
    }, [itemSearch]);

    /** Defaults to the item's own purchase price — the last figure the catalog
        agreed with its supplier — which the operator overwrites as needed. */
    const pickItem = (key: number, itemId: string) => {
        const picked = items.find((item) => String(item.id) === itemId);
        setLines((current) =>
            current.map((line) =>
                line.key === key
                    ? { ...line, itemId, unitPrice: picked ? String(picked.purchasePrice) : line.unitPrice }
                    : line,
            ),
        );
    };

    const setLine = <K extends keyof DraftLine>(key: number, field: K, value: DraftLine[K]) =>
        setLines((current) =>
            current.map((line) => (line.key === key ? { ...line, [field]: value } : line)),
        );

    const lineTotal = (line: DraftLine) =>
        decMul(toDecimal(line.quantity || "0"), toDecimal(line.unitPrice || "0"));

    // Display only. The server recomputes subtotal, discount and grand total from
    // the lines; these are here so the operator sees the effect before submitting.
    const subtotal = useMemo(
        () =>
            decSum(
                lines.filter((line) => line.itemId !== "").map(lineTotal),
            ),
        [lines],
    );

    const save = async () => {
        setBusy(true);
        setError(null);
        try {
            const payload: PurchaseLineInput[] = lines
                .filter((line) => line.itemId !== "")
                .map((line) => ({
                    itemId: Number(line.itemId),
                    quantity: toDecimal(line.quantity),
                    unitPrice: toDecimal(line.unitPrice),
                    batchNo: optionalText(line.batchNo),
                    expiryDate: optionalText(line.expiryDate),
                }));

            const payments: PurchasePaymentInput[] =
                tendered.trim() !== "" && tenderId !== ""
                    ? [{ paymentMethodId: Number(tenderId), amount: toDecimal(tendered) }]
                    : [];

            const input: PurchaseInput = {
                supplierId: Number(supplierId),
                supplierInvoiceNo: optionalText(invoiceNo),
                purchasedAt,
                lines: payload,
                discount: optionalText(discount),
                note: optionalText(note),
                payments,
            };
            await createPurchase(input);
            onSaved();
        } catch (cause) {
            setError(messageOf(cause));
        } finally {
            setBusy(false);
        }
    };

    const ready = supplierId !== "" && lines.some((line) => line.itemId !== "");

    return (
        <ModalOverlay isOpen onOpenChange={(open) => !open && onCancel()}>
            <Modal className="max-w-4xl">
                <Dialog className="max-h-[85dvh] overflow-y-auto p-6">
                    <form
                        className="flex flex-col gap-4"
                        onSubmit={(event) => {
                            event.preventDefault();
                            void save();
                        }}
                    >
                        <h2 className="text-display-xs font-semibold text-primary">Receive goods</h2>

                        <div className="grid grid-cols-3 gap-3">
                            <Select
                                label="Supplier"
                                selectedKey={supplierId}
                                onSelectionChange={(key) => setSupplierId(String(key))}
                            >
                                {suppliers.map((supplier) => (
                                    <Select.Item key={String(supplier.id)} id={String(supplier.id)}>
                                        {supplier.name}
                                    </Select.Item>
                                ))}
                            </Select>
                            <Input label="Date" type="date" value={purchasedAt} onChange={setPurchasedAt} />
                            <Input label="Supplier invoice no" value={invoiceNo} onChange={setInvoiceNo} />
                        </div>

                        <Input
                            label="Find an item"
                            placeholder="Search name or code"
                            value={itemSearch}
                            onChange={setItemSearch}
                        />

                        <div className="flex flex-col gap-2">
                            {lines.map((line, index) => (
                                <div key={line.key} className="grid grid-cols-12 items-end gap-2">
                                    <div className="col-span-5">
                                        <Select
                                            label={index === 0 ? "Item" : undefined}
                                            selectedKey={line.itemId}
                                            onSelectionChange={(key) => pickItem(line.key, String(key))}
                                        >
                                            {items.map((item) => (
                                                <Select.Item key={String(item.id)} id={String(item.id)}>
                                                    {item.name} ({item.code})
                                                </Select.Item>
                                            ))}
                                        </Select>
                                    </div>
                                    <div className="col-span-2">
                                        <Input
                                            label={index === 0 ? "Quantity" : undefined}
                                            value={line.quantity}
                                            onChange={(value) => setLine(line.key, "quantity", String(value))}
                                        />
                                    </div>
                                    <div className="col-span-2">
                                        <Input
                                            label={index === 0 ? "Unit price" : undefined}
                                            value={line.unitPrice}
                                            onChange={(value) => setLine(line.key, "unitPrice", String(value))}
                                        />
                                    </div>
                                    <div className="col-span-2">
                                        <Input
                                            label={index === 0 ? "Batch / expiry" : undefined}
                                            placeholder="optional"
                                            value={line.batchNo}
                                            onChange={(value) => setLine(line.key, "batchNo", String(value))}
                                        />
                                    </div>
                                    <div className="col-span-1 flex justify-end">
                                        <Button
                                            size="sm"
                                            color="tertiary"
                                            iconLeading={Trash01}
                                            aria-label="Remove line"
                                            onPress={() => setLines((current) => current.filter((l) => l.key !== line.key))}
                                        />
                                    </div>
                                </div>
                            ))}
                            <div>
                                <Button
                                    size="sm"
                                    color="secondary"
                                    iconLeading={Plus}
                                    onPress={() => {
                                        setLines((current) => [...current, draftLine(nextKey)]);
                                        setNextKey((n) => n + 1);
                                    }}
                                >
                                    Add line
                                </Button>
                            </div>
                        </div>

                        <div className="grid grid-cols-2 gap-3">
                            <Input
                                label="Discount (amount or %, e.g. 10%)"
                                value={discount}
                                onChange={setDiscount}
                            />
                            <Input label="Note" value={note} onChange={setNote} />
                        </div>

                        <div className="grid grid-cols-2 gap-3">
                            <Select
                                label="Paid now (optional)"
                                selectedKey={tenderId}
                                onSelectionChange={(key) => setTenderId(String(key))}
                            >
                                {methods.map((method) => (
                                    <Select.Item key={String(method.id)} id={String(method.id)}>
                                        {method.name}
                                    </Select.Item>
                                ))}
                            </Select>
                            <Input label="Amount" value={tendered} onChange={setTendered} />
                        </div>

                        {error && (
                            <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                                {error}
                            </p>
                        )}

                        <div className="flex items-center justify-between">
                            <span className="text-sm text-tertiary">
                                Subtotal {formatMoney(subtotal)}
                            </span>
                            <div className="flex gap-2">
                                <Button color="secondary" onPress={onCancel}>
                                    Cancel
                                </Button>
                                <Button type="submit" isLoading={busy} isDisabled={!ready}>
                                    Receive
                                </Button>
                            </div>
                        </div>
                    </form>
                </Dialog>
            </Modal>
        </ModalOverlay>
    );
};