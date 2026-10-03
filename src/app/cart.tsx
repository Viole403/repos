import { createContext, useCallback, useContext, useMemo, useState } from "react";
import type { ReactNode } from "react";
import type { Decimal, ItemView } from "./ipc";
import { decAdd, decIsPositive, decMul, decSub, decSum } from "./decimal";

/**
 * Cart state for the register. Client-side only — the backend learns about a sale
 * once, at checkout. Every value here is a candidate: `checkout` recomputes the
 * totals and rejects the sale if it disagrees.
 */
export interface CartLine {
    itemId: number;
    code: string;
    name: string;
    unitName: string | null;
    /** Price when the line was added; `sale_details` snapshots it so a later catalog edit cannot move a receipt. */
    unitPrice: Decimal;
    quantity: Decimal;
    discount: Decimal;
}

/** What `add` takes. Quantity and discount default to one unit and nothing off. */
export type NewCartLine = Omit<CartLine, "quantity" | "discount"> & { quantity?: Decimal; discount?: Decimal };

export interface CartTotals {
    lines: readonly CartLine[];
    /** Sum of `unitPrice * quantity` across lines, before discounts — mirrors `sales.subtotal`. */
    subtotal: Decimal;
    /** Per-line discounts summed. An order-level discount is typed at checkout, not stored per line. */
    discountTotal: Decimal;
    /** Always zero until the tax settings land; nothing configures a rate yet. */
    taxTotal: Decimal;
    /** `subtotal - discountTotal + taxTotal`, matching the backend's arithmetic. */
    grandTotal: Decimal;
    /** Distinct lines. Unit counts are fractional (2.5 kg), so this stays an integer. */
    itemCount: number;
    /** False when empty or a line has a non-positive quantity, which the backend rejects. */
    canCheckout: boolean;
}

interface CartValue extends CartTotals {
    add: (line: NewCartLine) => void;
    remove: (itemId: number) => void;
    setQuantity: (itemId: number, quantity: Decimal) => void;
    clear: () => void;
}

const CartContext = createContext<CartValue | null>(null);

/** Catalog row -> cart line, priced at the sale price the till will charge. */
export const cartLineFromItem = (item: ItemView): NewCartLine => ({
    itemId: item.id,
    code: item.code,
    name: item.name,
    unitName: item.saleUnitName,
    unitPrice: item.salePrice,
});

export const CartProvider = ({ children }: { children: ReactNode }) => {
    const [lines, setLines] = useState<CartLine[]>([]);

    const add = useCallback((incoming: NewCartLine) => {
        const quantity = incoming.quantity ?? "1";
        const discount = incoming.discount ?? "0";
        setLines((current) => {
            const at = current.findIndex((line) => line.itemId === incoming.itemId);
            if (at === -1) return [...current, { ...incoming, quantity, discount }];

            // One item yields one line. The register always scans at the current
            // catalog price, so a second price means the catalog moved and the
            // line re-prices instead of duplicating — which is also what keeps
            // `remove`/`setQuantity` unambiguous when keyed on item id alone.
            return current.map((line, i) =>
                i === at
                    ? { ...line, unitPrice: incoming.unitPrice, quantity: decAdd(line.quantity, quantity), discount }
                    : line,
            );
        });
    }, []);

    const remove = useCallback((itemId: number) => {
        setLines((current) => current.filter((l) => l.itemId !== itemId));
    }, []);

    const setQuantity = useCallback((itemId: number, quantity: Decimal) => {
        setLines((current) => current.map((l) => (l.itemId === itemId ? { ...l, quantity } : l)));
    }, []);

    const clear = useCallback(() => setLines([]), []);

    const totals = useMemo<CartTotals>(() => {
        const gross = lines.map((line) => decMul(line.unitPrice, line.quantity));
        const subtotal = decSum(gross);
        const discountTotal = decSum(lines.map((line) => line.discount));
        const taxTotal = "0";

        return {
            lines,
            subtotal,
            discountTotal,
            taxTotal,
            // Line discounts land on the grand total, not just the header: the
            // backend subtracts them before adding tax, so mirroring that keeps
            // the displayed total equal to the one written.
            grandTotal: decAdd(decSub(subtotal, discountTotal), taxTotal),
            itemCount: lines.length,
            canCheckout: lines.length > 0 && lines.every((line) => decIsPositive(line.quantity)),
        };
    }, [lines]);

    const value = useMemo<CartValue>(() => ({ ...totals, add, remove, setQuantity, clear }), [totals, add, remove, setQuantity, clear]);

    return <CartContext.Provider value={value}>{children}</CartContext.Provider>;
};

export const useCart = (): CartValue => {
    const value = useContext(CartContext);
    if (!value) throw new Error("useCart must be called inside <CartProvider>.");
    return value;
};

/** `unitPrice * quantity - discount`, per line. Mirrors `sale_details.line_total`. */
export const cartLineTotal = (line: CartLine): Decimal => decSub(decMul(line.unitPrice, line.quantity), line.discount);