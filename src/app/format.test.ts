import { describe, expect, test } from "bun:test";
import { formatTimestamp, toDecimal } from "./format";

describe("formatTimestamp", () => {
    // A zoneless string and its `Z` form denote the same instant, so they must render
    // identically. On a UTC machine this passes either way — it is here to catch the
    // regression on the machines that are not UTC.
    test("a zoneless timestamp and its Z form render the same", () => {
        expect(formatTimestamp("2026-10-03T09:12:00")).toBe(formatTimestamp("2026-10-03T09:12:00Z"));
    });

    test("an explicit offset is not overwritten", () => {
        expect(formatTimestamp("2026-10-03T09:12:00+07:00")).not.toBe(formatTimestamp("2026-10-03T09:12:00Z"));
    });

    test("an unparseable value renders as a dash rather than Invalid Date", () => {
        expect(formatTimestamp("not a date")).toBe("—");
        expect(formatTimestamp("")).toBe("—");
    });
});

describe("toDecimal", () => {
    test("a blank field becomes zero, which the Rust side can parse", () => {
        expect(toDecimal("")).toBe("0");
        expect(toDecimal("   ")).toBe("0");
    });

    test("digits typed are preserved exactly", () => {
        expect(toDecimal(" 1234.500 ")).toBe("1234.500");
    });
});
