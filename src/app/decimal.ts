import type { Decimal } from "./ipc";

/**
 * Fixed-point helpers at the migration's `DECIMAL_SCALE`. Sums are done in integer
 * minor units over `BigInt` rather than in JS floats, because `0.1 + 0.2` must
 * never reach the till as `0.30000000000000004`. Rust's `Decimal` remains
 * authoritative at checkout — these values are for display and for sending a
 * candidate total back, not for owning the real value.
 */

/** Matches `DECIMAL_SCALE` in `src-tauri/src/migration.rs`. */
const SCALE = 3;
const MINOR = 10n ** BigInt(SCALE);

const NUMERIC = /^([+-]?)(\d*)(?:\.(\d*))?$/;

/** Half-up integer division. Ties go away from zero, matching `Decimal` rounding. */
const halfUp = (numerator: bigint, denominator: bigint): bigint => {
    if (denominator === 0n) return 0n;
    const divisor = denominator < 0n ? -denominator : denominator;
    const negative = numerator < 0n;
    const magnitude = negative ? -numerator : numerator;
    const rounded = (2n * magnitude + divisor) / (2n * divisor);
    return negative ? -rounded : rounded;
};

const toScaled = (value: Decimal): bigint => {
    const raw = value.trim();
    const match = NUMERIC.exec(raw);
    if (!match) {
        // Junk or blank. Falling back to zero keeps one bad field from turning
        // every total into NaN; checkout rejects the bad value server-side.
        const parsed = Number(raw);
        return Number.isFinite(parsed) ? BigInt(Math.round(parsed * 1e6)) / MINOR : 0n;
    }
    const sign = match[1];
    const whole = match[2] === "" || match[2] === undefined ? "0" : match[2];
    const fraction = match[3] ?? "";
    const digits = BigInt(whole + (fraction + "000").slice(0, SCALE));
    return sign === "-" ? -digits : digits;
};

/** Back to a plain string with trailing zeros dropped: `2000n` -> `"2"`, `500n` -> `"0.5"`. */
const fromScaled = (scaled: bigint): Decimal => {
    const negative = scaled < 0n;
    const magnitude = negative ? -scaled : scaled;
    const fraction = (magnitude % MINOR).toString().padStart(SCALE, "0").replace(/0+$/, "");
    return `${negative ? "-" : ""}${magnitude / MINOR}${fraction ? `.${fraction}` : ""}`;
};

export const decAdd = (a: Decimal, b: Decimal): Decimal => fromScaled(toScaled(a) + toScaled(b));
export const decSub = (a: Decimal, b: Decimal): Decimal => fromScaled(toScaled(a) - toScaled(b));

/** Re-scaled back to 3 dp, since a product of two 3 dp values needs 6. */
export const decMul = (a: Decimal, b: Decimal): Decimal => fromScaled(halfUp(toScaled(a) * toScaled(b), MINOR));

export const decSum = (values: readonly Decimal[]): Decimal => values.reduce((total, value) => decAdd(total, value), "0");

export const decIsPositive = (value: Decimal): boolean => toScaled(value) > 0n;

export const decIsNegative = (value: Decimal): boolean => toScaled(value) < 0n;

/** -1, 0 or 1. Compare via this rather than `Number(a) - Number(b)`. */
export const decCompare = (a: Decimal, b: Decimal): number => {
    const left = toScaled(a);
    const right = toScaled(b);
    return left < right ? -1 : left > right ? 1 : 0;
};

/** How many units a whole step is worth: `"2" + 1` -> `"3"`, `"2.5" + 1` -> `"3.5"`. */
export const decStep = (value: Decimal, delta: number): Decimal => fromScaled(toScaled(value) + toScaled(String(delta)));