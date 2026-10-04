import { emit } from "@tauri-apps/api/event";

/** One cart row as the customer sees it: name, quantity, line total. */
export interface DisplayLine {
    name: string;
    quantity: string;
    total: string;
}

/**
 * Everything the mirror shows. Pushed from the Register on every cart change —
 * the display never pulls, so it cannot show a stale basket.
 *
 * `payment` and `done` carry the figures the till committed; `done` additionally
 * names the sale, so a rating tap lands on the right row. The QRIS QR slot lives
 * in the `payment` state and stays empty until Stage 14 generates dynamic QRs.
 */
export interface DisplayPayload {
    state: "idle" | "cart" | "payment" | "done";
    lines: DisplayLine[];
    subtotal: string;
    discountTotal: string;
    taxTotal: string;
    grandTotal: string;
    /** Saved vs. list price — the mirror's "Anda hemat". */
    savedTotal: string;
    paymentMethod: string | null;
    paidTotal: string | null;
    changeDue: string | null;
    saleId: number | null;
    loyaltyEarned: number;
    /** The mirror only offers rating taps when the till says it may. */
    touch: boolean;
}

export const DISPLAY_EVENT = "display-update";

/** Fire-and-forget: under `bun run dev` there is no bridge, and the till must
 *  keep working with no second window open. */
export const emitDisplay = (payload: DisplayPayload): void => {
    void emit(DISPLAY_EVENT, payload).catch(() => {});
};

const TOUCH_KEY = "repos.display.touch";

export const loadTouch = (): boolean => {
    try {
        return localStorage.getItem(TOUCH_KEY) === "1";
    } catch {
        return false;
    }
};

export const saveTouch = (on: boolean): void => {
    try {
        localStorage.setItem(TOUCH_KEY, on ? "1" : "0");
    } catch {
        // A display preference is not worth failing a sale over.
    }
};
