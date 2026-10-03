import type { ReactNode } from "react";
import type { RouteObject } from "react-router-dom";
import { AppShell } from "./AppShell";
import { Dashboard } from "./screens/Dashboard";
import { ItemsList } from "./screens/ItemsList";
import { Brands } from "./screens/Brands";
import { ItemCategories } from "./screens/ItemCategories";
import { Register } from "./screens/Register";
import { Drafts } from "./screens/Drafts";
import { Units } from "./screens/Units";
import { enabledRoutes, navItems } from "./nav-config";

/** `#/catalog/items` -> `/catalog/items`. Nav hrefs are hash-prefixed; routes are not. */
const toPath = (href: string) => href.replace(/^#/, "") || "/";

/** Every route the app renders, keyed by path. An enabled entry missing here throws — see AGENTS.md. */
const screens: Record<string, ReactNode> = {
    "/": <Dashboard />,
    "/pos": <Register />,
    "/sales/holds": <Drafts />,
    "/catalog/items": <ItemsList />,
    "/catalog/units": <Units />,
    "/catalog/brands": <Brands />,
    "/catalog/categories": <ItemCategories />,
};

/** Derived from the sidebar, so no nav entry can point at a route that does not exist. */
const children: RouteObject[] = navItems.flatMap((item) => {
    if ("divider" in item && item.divider) return [];

    const hrefs = [
        ...("href" in item && item.href ? [item.href] : []),
        ...("items" in item ? (item.items ?? []).filter((child) => !child.disabled).map((child) => child.href) : []),
    ];

    return hrefs.filter((href) => enabledRoutes.has(toPath(href))).map((href) => {
        const path = toPath(href);
        const screen = screens[path];
        if (!screen) throw new Error(`No screen registered for enabled route "${path}". Add it to screens in src/app/routes.tsx.`);
        return path === "/" ? { index: true, element: screen } : { path, element: screen };
    });
});

export const routes: RouteObject[] = [
    {
        element: <AppShell />,
        children,
    },
];