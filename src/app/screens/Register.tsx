import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { CheckCircle, Delete, Minus, Plus, Scan, XClose } from "@untitledui/icons";
import { Badge } from "@/components/base/badges/badges";
import { Button } from "@/components/base/buttons/button";
import { Input } from "@/components/base/input/input";
import { Select } from "@/components/base/select/select";
import type { SelectItemType } from "@/components/base/select/select-shared";
import { Table, TableCard } from "@/components/application/table/table";
import { CartProvider, cartLineFromItem, cartLineTotal, useCart } from "@/app/cart";
import type { CartLine } from "@/app/cart";
import { decAdd, decCompare, decIsPositive, decStep, decSub } from "@/app/decimal";
import { formatMoney, formatQuantity, toDecimal } from "@/app/format";
import type { CheckoutInput, Decimal, PaymentLine, SaleView } from "@/app/ipc";
import { checkout, listCustomers, listItems, stockOnHand } from "@/app/ipc";

interface Tender {
    method: string;
    /** Kept as typed text so the exact digits reach the wire, not a parsed float. */
    amount: string;
}

const PAYMENT_METHODS: SelectItemType[] = [
    { id: "Cash", label: "Cash" },
    { id: "Card", label: "Card" },
    { id: "Qris", label: "QRIS" },
];

/**
 * A code search can match on name, alternative name or generic name too, so the
 * result set can be long and the exact code can be pushed out of the first page.
 * Ask for the widest page the backend allows and filter locally.
 */
const LOOKUP_PAGE = 500;

const SHORTCUTS: readonly (readonly [string, string])[] = [
    ["Enter", "Look up the code and add it"],
    ["/", "Jump to the barcode box"],
    ["↑ ↓", "Move the cart selection"],
    ["+ / -", "Change quantity by one"],
    ["Delete", "Remove the selected line"],
    ["F2", "Complete the sale"],
    ["Esc", "Clear the barcode box"],
];

/** Keys that still mean something while the barcode box has focus. */
const TYPING_SAFE = new Set(["ArrowUp", "ArrowDown", "+", "=", "-", "Escape"]);

type Lookup =
    | { kind: "idle" }
    | { kind: "searching"; code: string }
    | { kind: "unknown"; code: string }
    | { kind: "failed"; message: string };

type SaleState =
    | { kind: "idle" }
    | { kind: "saving" }
    | { kind: "done"; view: SaleView }
    | { kind: "failed"; message: string };

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error));

/**
 * Money is shown as `1.234,50` under `id-ID`, but `Decimal::from_str` understands
 * only a dot. Strip the thousands separator and read the comma as the decimal
 * point, so retyping a displayed total does not get the sale rejected.
 */
const normalizeMoney = (raw: string): string => raw.replace(/\./g, "").replace(",", ".");

const RegisterScreen = () => {
    const cart = useCart();
    const { lines, subtotal, discountTotal, taxTotal, grandTotal, itemCount, canCheckout } = cart;
    const { add: addToCart, clear: clearCart, remove: removeLine, setQuantity } = cart;

    const [code, setCode] = useState("");
    const [lookup, setLookup] = useState<Lookup>({ kind: "idle" });
    const [sale, setSale] = useState<SaleState>({ kind: "idle" });
    const [paymentMethod, setPaymentMethod] = useState("Cash");
    // Split is opt-in. The single-tender path is one field and one keypress, and
    // should not become two rows to fill in for the common case.
    const [split, setSplit] = useState(false);
    const [tenders, setTenders] = useState<Tender[]>([
        { method: "Cash", amount: "" },
        { method: "Card", amount: "" },
    ]);
    // "" is a walk-in, which is what most of a counter's sales are.
    const [customerKey, setCustomerKey] = useState("");
    const [customers, setCustomers] = useState<SelectItemType[]>([]);

    // Searched rather than listed: the till needs a handful of regulars, not every
    // customer in the database, and the list is fetched on focus.
    const findCustomers = useCallback(async (term: string) => {
        try {
            const found = await listCustomers({ page: 1, perPage: 20, search: term || undefined });
            setCustomers([
                { id: "", label: "Walk-in" },
                ...found.rows.map((row) => ({ id: String(row.id), label: row.name })),
            ]);
        } catch {
            // A till must still take cash with no customer list. The backend rejects a
            // sale that names an unknown customer either way.
            setCustomers([{ id: "", label: "Walk-in" }]);
        }
    }, []);
    const [paid, setPaid] = useState("");
    const [note, setNote] = useState("");
    const [onHand, setOnHand] = useState<Record<number, Decimal>>({});
    const [selectedItemId, setSelectedItemId] = useState<number | null>(null);

    const codeRef = useRef<HTMLInputElement>(null);
    /** Ids already asked about, so the effect does not refetch on every render. */
    const requested = useRef(new Set<number>());
    /** Guards against two quick scans resolving out of order. */
    const lookupSeq = useRef(0);

    const focusCode = useCallback(() => {
        codeRef.current?.focus();
        codeRef.current?.select();
    }, []);

    useEffect(() => focusCode(), [focusCode]);

    const runLookup = useCallback(
        async (raw: string) => {
            const term = raw.trim();
            if (term === "") return;

            const seq = ++lookupSeq.current;
            setLookup({ kind: "searching", code: term });

            try {
                const result = await listItems({ search: term, perPage: LOOKUP_PAGE });
                if (seq !== lookupSeq.current) return;

                // `search` is a substring match, so a fuzzy hit would put the wrong
                // product on the shelf. Only the whole code may sell. Compared
                // case-insensitively because alphanumeric symbologies (Code 39/128)
                // encode shift state into the barcode itself.
                const match = result.rows.find((row) => row.code.trim().toLowerCase() === term.toLowerCase());
                if (!match) {
                    setLookup({ kind: "unknown", code: term });
                    focusCode();
                    return;
                }

                addToCart(cartLineFromItem(match));
                setSelectedItemId(match.id);
                setCode("");
                setLookup({ kind: "idle" });
                // A banner from the previous sale is stale the moment a new one starts.
                setSale({ kind: "idle" });
                focusCode();
            } catch (error) {
                if (seq !== lookupSeq.current) return;
                setLookup({ kind: "failed", message: messageOf(error) });
                focusCode();
            }
        },
        [addToCart, focusCode],
    );

    // Fetch availability for newly scanned items. Advisory: checkout re-checks
    // inside the transaction, so a failed read must not stand in the way of a sale.
    useEffect(() => {
        const wanted = lines.map((line) => line.itemId).filter((id) => !requested.current.has(id));
        if (wanted.length === 0) return;
        wanted.forEach((id) => requested.current.add(id));

        let cancelled = false;
        void Promise.all(
            wanted.map(async (id): Promise<[number, Decimal | null]> => {
                try {
                    return [id, await stockOnHand(id)];
                } catch {
                    return [id, null];
                }
            }),
        ).then((entries) => {
            if (cancelled) return;
            setOnHand((current) => {
                const next = { ...current };
                for (const [id, quantity] of entries) if (quantity !== null) next[id] = quantity;
                return next;
            });
        });

        return () => {
            cancelled = true;
        };
    }, [lines]);

    const selected = lines.find((line) => line.itemId === selectedItemId) ?? null;

    const moveSelection = useCallback(
        (delta: number) => {
            if (lines.length === 0) return;
            const at = lines.findIndex((line) => line.itemId === selectedItemId);
            const next = at === -1 ? (delta > 0 ? 0 : lines.length - 1) : Math.min(lines.length - 1, Math.max(0, at + delta));
            setSelectedItemId(lines[next]?.itemId ?? null);
        },
        [lines, selectedItemId],
    );

    const changeQuantity = useCallback(
        (line: CartLine, delta: number) => {
            const next = decStep(line.quantity, delta);
            // Never step down to zero: the backend rejects a non-positive quantity,
            // and removing the line is what Delete is for.
            if (decIsPositive(next)) setQuantity(line.itemId, next);
        },
        [setQuantity],
    );

    const stepQuantity = useCallback(
        (delta: number) => {
            if (selected) changeQuantity(selected, delta);
        },
        [selected, changeQuantity],
    );

    const removeSelected = useCallback(() => {
        if (!selected) return;
        removeLine(selected.itemId);
        setSelectedItemId(null);
    }, [selected, removeLine]);

    const paidDigits = paid.trim() === "" ? "" : normalizeMoney(paid.trim());
    const paidInvalid = paidDigits !== "" && (!Number.isFinite(Number(paidDigits)) || Number(paidDigits) < 0);

    const tenderLines = (): PaymentLine[] =>
        tenders
            .filter((t) => t.amount.trim() !== "")
            .map((t) => ({ method: t.method, amount: toDecimal(t.amount), reference: null }));

    const tenderSum = tenderLines().reduce((total, t) => decAdd(total, t.amount), "0");
    // Same rule the server applies, so the cashier sees it before the sale is refused.
    const splitOver = split && decCompare(tenderSum, grandTotal) > 0;

    const submitSale = useCallback(async () => {
        if (!canCheckout || paidInvalid || splitOver || sale.kind === "saving") return;

        const input: CheckoutInput = {
            lines: lines.map((line) => ({
                itemId: line.itemId,
                quantity: line.quantity,
                unitPrice: line.unitPrice,
                discount: line.discount,
            })),
            // Order-level discount and tax stay zero: nothing configures them yet.
            discountTotal: "0",
            taxTotal,
            // Blank means paid in full; a smaller figure books a credit sale.
            paidTotal: paidDigits === "" ? null : toDecimal(paidDigits),
            paymentMethod,
            note: note.trim() === "" ? null : note.trim(),
            // Always promote. A draft that moves no stock is a separate flow.
            promote: true,
            customerId: customerKey === "" ? null : Number(customerKey),
        };

        setSale({ kind: "saving" });
        try {
            const view = await checkout(input);
            setSale({ kind: "done", view });
            setPaid("");
            setNote("");
            // The backend returns the balances it committed, so take those over
            // what was cached, then forget them so the next scan refetches.
            for (const entry of view.stockOnHand) requested.current.delete(entry.itemId);
            setOnHand((current) => {
                const next = { ...current };
                for (const entry of view.stockOnHand) next[entry.itemId] = entry.quantity;
                return next;
            });
            clearCart();
            setSelectedItemId(null);
            focusCode();
        } catch (error) {
            setSale({ kind: "failed", message: messageOf(error) });
            focusCode();
        }
    }, [canCheckout, paidInvalid, paidDigits, sale.kind, lines, taxTotal, paymentMethod, customerKey, split, tenderSum, grandTotal, note, clearCart, focusCode]);

    useEffect(() => {
        const onKeyDown = (event: KeyboardEvent) => {
            // Never swallow a chord the browser or OS owns.
            if (event.ctrlKey || event.metaKey || event.altKey) return;

            const target = event.target as HTMLElement | null;
            const editable = target !== null && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable);
            const inBarcode = target === codeRef.current;
            // Typing in any field beats the shortcuts, so no keystroke is stolen
            // mid-word. F2 is exempt: no text field uses it, and blocking it would
            // strand a cashier who just keyed a paid amount.
            if (event.key !== "F2" && editable && !(inBarcode && TYPING_SAFE.has(event.key))) return;

            switch (event.key) {
                case "/":
                    event.preventDefault();
                    focusCode();
                    break;
                case "Escape":
                    if (code.trim() !== "") {
                        event.preventDefault();
                        setCode("");
                        setLookup({ kind: "idle" });
                        focusCode();
                    }
                    break;
                case "ArrowUp":
                case "ArrowDown":
                    // Leave the scroll alone when there is no line to move to.
                    if (lines.length === 0) break;
                    event.preventDefault();
                    moveSelection(event.key === "ArrowDown" ? 1 : -1);
                    break;
                case "+":
                case "=":
                    event.preventDefault();
                    stepQuantity(1);
                    break;
                case "-":
                    event.preventDefault();
                    stepQuantity(-1);
                    break;
                case "Delete":
                    event.preventDefault();
                    removeSelected();
                    break;
                case "F2":
                    event.preventDefault();
                    void submitSale();
                    break;
                default:
                    break;
            }
        };

        window.addEventListener("keydown", onKeyDown);
        return () => window.removeEventListener("keydown", onKeyDown);
    }, [focusCode, moveSelection, stepQuantity, removeSelected, submitSale, code, lines.length]);

    const available = (line: CartLine): Decimal | null => (line.itemId in onHand ? onHand[line.itemId] : null);
    const change = decSub(paidDigits === "" ? grandTotal : paidDigits, grandTotal);

    const rowTotal = useMemo(() => new Map(lines.map((line) => [line.itemId, cartLineTotal(line)])), [lines]);

    return (
        <div className="flex flex-col gap-4 p-6 md:p-8">
            <form
                className="flex items-end gap-2"
                onSubmit={(event) => {
                    event.preventDefault();
                    void runLookup(code);
                }}
                // Enter is intercepted here rather than left to the native submit:
                // React Aria's text field can swallow it. The button keeps the
                // submit path for mouse users.
                onKeyDown={(event: ReactKeyboardEvent<HTMLFormElement>) => {
                    if (event.key !== "Enter") return;
                    event.preventDefault();
                    void runLookup(code);
                }}
            >
                <Input
                    ref={codeRef}
                    size="lg"
                    icon={Scan}
                    label="Barcode or item code"
                    placeholder="Scan a barcode or type a code"
                    shortcut="/"
                    value={code}
                    onChange={(value) => {
                        setCode(value);
                        // A stale "unknown code" banner next to edited text reads as a
                        // verdict on a code the cashier no longer typed.
                        if (lookup.kind !== "idle") setLookup({ kind: "idle" });
                    }}
                    isDisabled={sale.kind === "saving"}
                    className="w-full"
                />
                <Button type="submit" isDisabled={code.trim() === "" || sale.kind === "saving"}>
                    Add
                </Button>
            </form>

            {lookup.kind === "searching" && (
                <p className="rounded-lg bg-secondary px-3 py-2 text-sm text-tertiary">Looking up “{lookup.code}”…</p>
            )}
            {lookup.kind === "unknown" && (
                <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                    <span className="font-semibold">Unknown code “{lookup.code}”.</span> Nothing in the catalog has that exact
                    code. Check the label, or add the item to the catalog first.
                </p>
            )}
            {lookup.kind === "failed" && (
                <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">{lookup.message}</p>
            )}

            {sale.kind === "done" && (
                <p className="rounded-lg bg-success-secondary px-3 py-2 text-sm text-success-primary">
                    <span className="font-semibold">{sale.view.sale.invoiceNo}</span> recorded —{" "}
                    {formatMoney(sale.view.sale.grandTotal)} by {sale.view.sale.paymentMethod}.
                </p>
            )}
            {sale.kind === "failed" && (
                <p className="rounded-lg bg-error-secondary px-3 py-2 text-sm text-error-primary">
                    <span className="font-semibold">Sale rejected.</span> {sale.message}
                </p>
            )}

            <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
                <TableCard.Root>
                    <TableCard.Header
                        title="Cart"
                        description={itemCount === 0 ? "Nothing scanned yet" : `${itemCount} ${itemCount === 1 ? "line" : "lines"}`}
                    />

                    {lines.length === 0 ? (
                        <p className="px-6 py-16 text-center text-md text-tertiary">
                            Cart is empty. Scan a barcode or type a code above, then press Enter.
                        </p>
                    ) : (
                        <Table>
                            <Table.Header>
                                <Table.Head label="Item" />
                                <Table.Head label="Price" className="text-right" />
                                <Table.Head label="Qty" className="w-24" />
                                <Table.Head label="On hand" className="w-28" />
                                <Table.Head label="Total" className="text-right" />
                                <Table.Head label="" className="w-14" />
                            </Table.Header>
                            <Table.Body>
                                {lines.map((line) => {
                                    const inStock = available(line);
                                    const oversold = inStock !== null && decCompare(line.quantity, inStock) > 0;

                                    return (
                                        <Table.Row
                                            key={line.itemId}
                                            id={line.itemId}
                                            className={line.itemId === selectedItemId ? "bg-secondary" : undefined}
                                        >
                                            <Table.Cell>
                                                <span className="block font-medium text-primary">{line.name}</span>
                                                <span className="text-sm text-tertiary">
                                                    {line.code}
                                                    {line.unitName ? ` · ${line.unitName}` : ""}
                                                </span>
                                            </Table.Cell>
                                            <Table.Cell className="text-right text-tertiary">{formatMoney(line.unitPrice)}</Table.Cell>
                                            <Table.Cell>
                                                <div className="flex items-center gap-1">
                                                    <Button
                                                        size="xs"
                                                        color="tertiary"
                                                        iconLeading={Minus}
                                                        aria-label={`Decrease quantity of ${line.name}`}
                                                        onPress={() => changeQuantity(line, -1)}
                                                    />
                                                    <Input
                                                        size="sm"
                                                        aria-label={`Quantity for ${line.name}`}
                                                        className="w-16"
                                                        // Held as typed: clearing the box must not
                                                        // rewrite it to "0" mid-edit. An empty or
                                                        // non-positive value simply blocks checkout.
                                                        value={line.quantity}
                                                        onChange={(value) => setQuantity(line.itemId, value)}
                                                    />
                                                    <Button
                                                        size="xs"
                                                        color="tertiary"
                                                        iconLeading={Plus}
                                                        aria-label={`Increase quantity of ${line.name}`}
                                                        onPress={() => changeQuantity(line, 1)}
                                                    />
                                                </div>
                                            </Table.Cell>
                                            <Table.Cell>
                                                {inStock === null ? (
                                                    <span className="text-tertiary">…</span>
                                                ) : oversold ? (
                                                    <Badge size="sm" color="error">
                                                        {formatQuantity(inStock)} only
                                                    </Badge>
                                                ) : (
                                                    <span className="text-tertiary">{formatQuantity(inStock)}</span>
                                                )}
                                            </Table.Cell>
                                            <Table.Cell className="text-right font-medium text-primary">
                                                {formatMoney(rowTotal.get(line.itemId) ?? "0")}
                                            </Table.Cell>
                                            <Table.Cell>
                                                <Button
                                                    size="sm"
                                                    color="tertiary"
                                                    iconLeading={Delete}
                                                    aria-label={`Remove ${line.name}`}
                                                    onPress={() => {
                                                        removeLine(line.itemId);
                                                        if (selectedItemId === line.itemId) setSelectedItemId(null);
                                                    }}
                                                />
                                            </Table.Cell>
                                        </Table.Row>
                                    );
                                })}
                            </Table.Body>
                        </Table>
                    )}
                </TableCard.Root>

                <div className="flex flex-col gap-4">
                    <div className="flex flex-col gap-3 rounded-xl bg-primary p-4 ring-1 ring-primary">
                        <div className="flex items-center justify-between text-sm">
                            <span className="text-tertiary">Subtotal</span>
                            <span className="font-medium tabular-nums text-primary">{formatMoney(subtotal)}</span>
                        </div>
                        {discountTotal !== "0" && (
                            <div className="flex items-center justify-between text-sm">
                                <span className="text-tertiary">Discount</span>
                                <span className="font-medium tabular-nums text-primary">−{formatMoney(discountTotal)}</span>
                            </div>
                        )}
                        <div className="flex items-center justify-between text-sm">
                            <span className="text-tertiary">Tax</span>
                            <span className="font-medium tabular-nums text-primary">{formatMoney(taxTotal)}</span>
                        </div>
                        <div className="flex items-center justify-between border-t border-secondary pt-3">
                            <span className="font-semibold text-primary">Total</span>
                            <span className="text-lg font-semibold tabular-nums text-primary">{formatMoney(grandTotal)}</span>
                        </div>
                    </div>

                    {split ? (
                        <div className="flex flex-col gap-2">
                            <span className="text-sm font-medium text-primary">Split payment</span>
                            {tenders.map((tender, index) => (
                                <div key={index} className="flex items-end gap-2">
                                    <Select
                                        label="Method"
                                        aria-label={`Method for tender ${index + 1}`}
                                        items={PAYMENT_METHODS}
                                        selectedKey={tender.method}
                                        onSelectionChange={(key) =>
                                            setTenders((current) =>
                                                current.map((t, i) =>
                                                    i === index ? { ...t, method: String(key ?? "Cash") } : t,
                                                ),
                                            )
                                        }
                                        className="w-32"
                                    >
                                        {(row) => (
                                            <Select.Item id={row.id} textValue={row.label}>
                                                {row.label}
                                            </Select.Item>
                                        )}
                                    </Select>
                                    <Input
                                        label="Amount"
                                        aria-label={`Amount for tender ${index + 1}`}
                                        value={tender.amount}
                                        onChange={(value) =>
                                            setTenders((current) =>
                                                current.map((t, i) =>
                                                    i === index ? { ...t, amount: String(value) } : t,
                                                ),
                                            )
                                        }
                                    />
                                </div>
                            ))}
                            {splitOver && (
                                <p className="text-sm text-error-primary">
                                    The tenders add up to more than the total — the difference is change, not
                                    a payment.
                                </p>
                            )}
                            <p className="text-sm text-tertiary">Tendered {formatMoney(tenderSum)}</p>
                        </div>
                    ) : (
                        <>
                            <Select
                                label="Payment method"
                                items={PAYMENT_METHODS}
                                selectedKey={paymentMethod}
                                onSelectionChange={(key) => setPaymentMethod(String(key ?? "Cash"))}
                            >
                                {(row) => (
                                    <Select.Item id={row.id} textValue={row.label}>
                                        {row.label}
                                    </Select.Item>
                                )}
                            </Select>

                            <Input
                                label="Paid"
                                placeholder={formatMoney(grandTotal)}
                                hint={paidInvalid ? "Paid must be a positive number" : "Blank means paid in full"}
                                isInvalid={paidInvalid}
                                value={paid}
                                onChange={setPaid}
                            />
                        </>
                    )}

                    <Button color="secondary" size="sm" className="w-fit" onPress={() => setSplit(!split)}>
                        {split ? "Use one payment" : "Split across methods"}
                    </Button>

                    {!split && paidDigits !== "" && !paidInvalid && (
                        <p className="text-sm text-tertiary">
                            {decCompare(change, "0") >= 0
                                ? `Change ${formatMoney(change)}`
                                : `On account ${formatMoney(decSub("0", change))}`}
                        </p>
                    )}

                    <Select
                        label="Customer"
                        items={customers}
                        selectedKey={customerKey}
                        onOpenChange={(open) => open && void findCustomers("")}
                        onSelectionChange={(key) => setCustomerKey(String(key ?? ""))}
                    >
                        {(row) => (
                            <Select.Item id={row.id} textValue={row.label}>
                                {row.label}
                            </Select.Item>
                        )}
                    </Select>

                    <Input label="Note" value={note} onChange={setNote} />

                    <Button
                        size="lg"
                        iconLeading={CheckCircle}
                        isLoading={sale.kind === "saving"}
                        isDisabled={!canCheckout || paidInvalid || splitOver}
                        onPress={() => void submitSale()}
                    >
                        Complete sale
                    </Button>

                    <Button
                        color="secondary-destructive"
                        iconLeading={XClose}
                        isDisabled={lines.length === 0 || sale.kind === "saving"}
                        onPress={() => {
                            clearCart();
                            setSelectedItemId(null);
                            setSale({ kind: "idle" });
                            focusCode();
                        }}
                    >
                        Clear cart
                    </Button>

                    <div className="flex flex-col gap-2 rounded-xl bg-secondary p-4">
                        <p className="text-xs font-semibold text-tertiary">Keyboard</p>
                        {SHORTCUTS.map(([keys, description]) => (
                            <div key={keys} className="flex items-baseline justify-between gap-3 text-xs">
                                <kbd className="rounded border border-secondary bg-primary px-1.5 py-0.5 font-medium text-secondary">{keys}</kbd>
                                <span className="text-right text-tertiary">{description}</span>
                            </div>
                        ))}
                        <p className="border-t border-secondary pt-2 text-xs text-tertiary">
                            Shortcuts pause while you type, except F2. Nothing here overrides a Ctrl or Cmd chord.
                        </p>
                    </div>
                </div>
            </div>
        </div>
    );
};

/** Owns the provider so a route can render `<Register />` with nothing else. */
export const Register = () => (
    <CartProvider>
        <RegisterScreen />
    </CartProvider>
);