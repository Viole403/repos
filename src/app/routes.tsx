import type { ReactNode } from "react";
import type { RouteObject } from "react-router-dom";
import { Navigate, Outlet } from "react-router-dom";
import { AppShell } from "./AppShell";
import { HOME_PATH, LOGIN_PATH, useAuth } from "./auth";
import { Dashboard } from "./screens/Dashboard";
import { ItemsList } from "./screens/ItemsList";
import { Brands } from "./screens/Brands";
import { ItemCategories } from "./screens/ItemCategories";
import { Login, SessionLoading } from "./screens/Login";
import { Setup } from "./screens/Setup";
import { Accounts } from "./screens/Accounts";
import { Roles } from "./screens/Roles";
import { Customers } from "./screens/Customers";
import { Suppliers } from "./screens/Suppliers";
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
    "/customers": <Customers />,
    "/purchase/suppliers": <Suppliers />,
    "/settings/accounts": <Accounts />,
    "/settings/roles": <Roles />,
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

/**
 * Gate for everything behind the login. `replace` so the guarded URL does not
 * sit in history — signing back in would otherwise bounce through it again.
 *
 * This is navigation only. Nothing here is a security boundary: the backend
 * session cell decides who is signed in, not which URL the user reached.
 */
const RequireAuth = () => {
    const { status } = useAuth();

    if (status === "restoring") return <SessionLoading />;
    if (status === "anonymous") return <Navigate to={LOGIN_PATH} replace />;
    return <Outlet />;
};

/** The login route itself: a signed-in operator has no reason to see it. */
const OnlyAnonymous = () => {
    const { status, needsSetup } = useAuth();

    if (status === "restoring") return <SessionLoading />;
    if (status === "authenticated") return <Navigate to={HOME_PATH} replace />;
    return needsSetup ? <Setup /> : <Login />;
};

export const routes: RouteObject[] = [
    // Not in `navItems`: the login screen is full-screen by design, so a sidebar
    // entry for it would be a link that leaves the sidebar.
    { path: LOGIN_PATH, element: <OnlyAnonymous /> },
    {
        element: <RequireAuth />,
        children: [
            {
                element: <AppShell />,
                children,
            },
        ],
    },
];