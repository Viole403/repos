import type { Decimal } from "./ipc";

/**
 * Money arrives as a `Decimal` string so scale survives the wire. Parse only for
 * display — never accumulate in `number`, and never send a parsed value back.
 */

const LOCALE = "id-ID";

const group = (min: number, max: number) => new Intl.NumberFormat(LOCALE, { minimumFractionDigits: min, maximumFractionDigits: max });

const twoDp = group(2, 2);
const upToTwoDp = group(0, 2);

/** `"1234.500"` -> `"1.234,50"`. Blank/invalid input renders as an em dash. */
export const formatMoney = (value: Decimal | null | undefined): string => {
    if (value === null || value === undefined || value === "") return "—";
    const parsed = Number(value);
    return Number.isFinite(parsed) ? twoDp.format(parsed) : "—";
};

/** Quantity: `"2.500"` -> `"2,5"`, but `"3.000"` -> `"3"`. Trailing zeros dropped. */
export const formatQuantity = (value: Decimal | null | undefined): string => {
    if (value === null || value === undefined || value === "") return "—";
    const parsed = Number(value);
    return Number.isFinite(parsed) ? upToTwoDp.format(parsed) : "—";
};

const dateTime = new Intl.DateTimeFormat(LOCALE, { dateStyle: "medium", timeStyle: "short" });

export const formatTimestamp = (value: string): string => {
    const parsed = new Date(value);
    return Number.isNaN(parsed.getTime()) ? "—" : dateTime.format(parsed);
};

/**
 * `Decimal` string back to what the command expects. A blank field becomes `"0"`
 * rather than `""`, which the Rust side would reject as unparseable.
 */
export const toDecimal = (raw: string): Decimal => {
    const trimmed = raw.trim();
    return trimmed === "" ? "0" : trimmed;
};