import { describe, expect, test } from "bun:test";
import {
    decAdd,
    decCompare,
    decIsNegative,
    decMul,
    decStep,
    decSub,
    decSum,
} from "./decimal";

/**
 * Money reaches the till as a string, so these helpers are the only thing standing
 * between a float and a wrong total. The cases below are the ones plain arithmetic
 * gets wrong.
 */
describe("decAdd", () => {
    test("adds without float noise", () => {
        // 0.1 + 0.2 is 0.30000000000000004 in plain JS.
        expect(decAdd("0.1", "0.2")).toBe("0.3");
        expect(decAdd("1", "1")).toBe("2");
        expect(decAdd("0.5", "0.5")).toBe("1");
    });

    test("normalizes trailing zeros", () => {
        expect(decAdd("1.000", "0")).toBe("1");
        expect(decAdd("1234.500", "0")).toBe("1234.5");
    });

    test("handles negatives", () => {
        expect(decAdd("-1", "-0.5")).toBe("-1.5");
        expect(decAdd("1", "-1")).toBe("0");
    });

    test("treats blank and junk as zero rather than NaN", () => {
        expect(decAdd("", "0")).toBe("0");
        expect(decAdd("abc", "0")).toBe("0");
        expect(decAdd("  7  ", "0")).toBe("7");
    });

    test("drops precision finer than the 3 dp column", () => {
        // Display only: the register sends the raw typed string, and the server
        // owns the real value, so nothing below the column's scale is lost.
        expect(decAdd("0.0005", "0")).toBe("0");
        expect(decAdd("1.2345", "0")).toBe("1.234");
    });
});

describe("decSub", () => {
    test("subtracts exactly", () => {
        expect(decSub("10", "2.5")).toBe("7.5");
        expect(decSub("100", "0.1")).toBe("99.9");
    });
});

describe("decMul", () => {
    test("multiplies without float noise", () => {
        // 1.2 * 0.5 is 0.6000000000000001 in plain JS.
        expect(decMul("1.2", "0.5")).toBe("0.6");
        expect(decMul("0.1", "0.1")).toBe("0.01");
        expect(decMul("3", "2.5")).toBe("7.5");
    });

    test("rounds a 6 dp product half-up back to 3 dp", () => {
        expect(decMul("1.005", "2")).toBe("2.01"); // 2.01 exactly
        expect(decMul("2.5", "1.005")).toBe("2.513"); // 2.5125 -> half-up at 3 dp
    });
});

describe("decSum", () => {
    test("sums a cart", () => {
        expect(decSum(["1", "2", "3"])).toBe("6");
        expect(decSum(["0.1", "0.2", "0.3"])).toBe("0.6");
    });

    test("is zero for an empty cart", () => {
        expect(decSum([])).toBe("0");
    });
});

describe("decStep", () => {
    test("steps fractional quantities", () => {
        expect(decStep("2.5", 1)).toBe("3.5");
        expect(decStep("2.5", -1)).toBe("1.5");
        expect(decStep("0.5", -1)).toBe("-0.5");
    });
});

describe("decCompare", () => {
    test("compares by value, not lexically", () => {
        // "10" < "2" as strings, which is the bug this replaces.
        expect(decCompare("2", "10")).toBe(-1);
        expect(decCompare("10", "2")).toBe(1);
        expect(decCompare("2", "2.000")).toBe(0);
    });
});
describe("decIsNegative", () => {
    test("reads the sign from the digits, not the string order", () => {
        // "-50.000" < "0" happens to hold for ASCII, but "9.999" < "0" does not —
        // a string comparison would call this negative.
        expect(decIsNegative("-50.000")).toBe(true);
        expect(decIsNegative("9.999")).toBe(false);
        expect(decIsNegative("0")).toBe(false);
        expect(decIsNegative("0.001")).toBe(false);
    });
});
