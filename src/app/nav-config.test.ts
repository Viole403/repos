import { describe, expect, test } from "bun:test";
import { enabledRoutes, visibleNavItems } from "./nav-config";

const top = (permissions: string[]) =>
    visibleNavItems(permissions)
        .map((e) => ("items" in e && e.items ? e.label : "divider" in e ? "" : e.label))
        .filter(Boolean);

const children = (permissions: string[], label: string) => {
    const group = visibleNavItems(permissions).find((e) => "items" in e && e.items && e.label === label);
    return "items" in group! && group!.items ? (group!.items as { label: string }[]).map((i) => i.label) : [];
};

describe("visibleNavItems", () => {
    test("hides an entry the operator cannot use", () => {
        expect(children(["role-list"], "Settings")).toEqual(["Roles"]);
    });

    test("hides the whole group when every child is hidden", () => {
        expect(top(["item-list", "sale-list"])).not.toContain("Settings");
    });

    test("an unknown permission set hides nothing", () => {
        // Before the round trip, or if `my_permissions` failed. Hiding the app from a
        // signed-in operator is worse than showing a screen that will reject.
        expect(children([], "Settings")).toEqual(["Accounts", "Roles"]);
        expect(children(["user-list", "role-list"], "Settings")).toEqual(["Accounts", "Roles"]);
    });

    test("trading screens stay visible to a cashier", () => {
        // Someone who can only sell still needs the register and the catalog.
        const tree = top(["sale-pos"]);
        expect(tree).toContain("POS Register");
        expect(tree).toContain("Catalog");
        expect(tree).not.toContain("Settings");
    });

    test("an enabled navigation entry always has a route", () => {
        // The inverse is deliberately not required: a `disabled` entry has no route yet.
        for (const entry of visibleNavItems(["item-list"])) {
            for (const child of "items" in entry && entry.items ? entry.items : [entry]) {
                if (!child.href || "divider" in child || child.disabled) continue;
                expect(enabledRoutes.has(child.href.replace(/^#/, ""))).toBe(true);
            }
        }
    });
});
