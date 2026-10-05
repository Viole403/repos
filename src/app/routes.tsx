import type { ReactNode } from "react";
import type { RouteObject } from "react-router-dom";
import { Navigate, Outlet } from "react-router-dom";
import { AppShell } from "./AppShell";
import { HOME_PATH, LOGIN_PATH, useAuth } from "./auth";
import { Dashboard } from "./screens/Dashboard";
import { ItemsList } from "./screens/ItemsList";
import { Brands } from "./screens/Brands";
import { ItemCategories } from "./screens/ItemCategories";
import { ItemSubCategories } from "./screens/ItemSubCategories";
import { Login, SessionLoading } from "./screens/Login";
import { Setup } from "./screens/Setup";
import { DatabaseSetup } from "./screens/DatabaseSetup";
import { Accounts } from "./screens/Accounts";
import { Roles } from "./screens/Roles";
import { Sales } from "./screens/Sales";
import { SaleDetail } from "./screens/SaleDetail";
import { Returns } from "./screens/Returns";
import { Customers } from "./screens/Customers";
import { Suppliers } from "./screens/Suppliers";
import { Incomes } from "./screens/Incomes";
import { Expenses } from "./screens/Expenses";
import { Deposits } from "./screens/Deposits";
import { CashAccounts } from "./screens/CashAccounts";
import { IncomeCategories } from "./screens/IncomeCategories";
import { ExpenseCategories } from "./screens/ExpenseCategories";
import { RecurringExpenses } from "./screens/RecurringExpenses";
import { Reports } from "./screens/Reports";
import { Purchases } from "./screens/Purchases";
import { Register } from "./screens/Register";
import { Drafts } from "./screens/Drafts";
import { Registers } from "./screens/Registers";
import { Quotations } from "./screens/Quotations";
import { Bookings } from "./screens/Bookings";
import { Promotions } from "./screens/Promotions";
import { Installments } from "./screens/Installments";
import { Warranties } from "./screens/Warranties";
import { Servicings } from "./screens/Servicings";
import { GiftCards } from "./screens/GiftCards";
import { CreditNotes } from "./screens/CreditNotes";
import { Stock } from "./screens/Stock";
import { StockOps } from "./screens/StockOps";
import { FixedAssets } from "./screens/FixedAssets";
import { Display } from "./screens/Display";
import { Combos } from "./screens/Combos";
import { Units } from "./screens/Units";
import { enabledRoutes, navItems } from "./nav-config";

/** `#/catalog/items` -> `/catalog/items`. Nav hrefs are hash-prefixed; routes are not. */
const toPath = (href: string) => href.replace(/^#/, "") || "/";

/** Every route the app renders, keyed by path. An enabled entry missing here throws — see AGENTS.md. */
const screens: Record<string, ReactNode> = {
    "/": <Dashboard />,
    "/pos": <Register />,
    "/sales/holds": <Drafts />,
    "/sales/registers": <Registers />,
    "/sales/quotations": <Quotations />,
    "/sales/bookings": <Bookings />,
    "/sales/promotions": <Promotions />,
    "/sales/installments": <Installments />,
    "/sales/warranties": <Warranties />,
    "/sales/servicings": <Servicings />,
    "/sales/gift-cards": <GiftCards />,
    "/sales/credit-notes": <CreditNotes />,
    "/stock": <Stock />,
    "/stock/operations": <StockOps />,
    "/stock/assets": <FixedAssets />,
    "/catalog/combos": <Combos />,
    "/catalog/items": <ItemsList />,
    "/catalog/units": <Units />,
    "/catalog/brands": <Brands />,
    "/catalog/categories": <ItemCategories />,
    "/catalog/sub-categories": <ItemSubCategories />,
    "/sales": <Sales />,
    "/sales/:id": <SaleDetail />,
    "/sales/:id/returns": <Returns />,
    "/customers": <Customers />,
    "/purchase": <Purchases />,
    "/purchase/suppliers": <Suppliers />,
    "/accounting/incomes": <Incomes />,
    "/accounting/expenses": <Expenses />,
    "/accounting/deposits": <Deposits />,
    "/accounting/accounts": <CashAccounts />,
    "/accounting/income-categories": <IncomeCategories />,
    "/accounting/expense-categories": <ExpenseCategories />,
    "/accounting/recurring": <RecurringExpenses />,
    "/accounting/reports": <Reports />,
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
 * Routes with no sidebar entry, because they are reached from a list rather than
 * from the sidebar. Still listed through `screens`, so the "enabled entry has no
 * screen" check below covers them too.
 */
const detailRoutes: string[] = ["/sales/:id", "/sales/:id/returns"];

const withoutNavEntry: RouteObject[] = detailRoutes.map((path) => {
    const screen = screens[path];
    if (!screen) throw new Error(`No screen registered for detail route "${path}".`);
    return { path, element: screen };
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
    // No database means no session and no data, so every screen would be broken. The
    // wizard's first step is the only thing worth showing.
    if (status === "unconfigured") return <Navigate to={LOGIN_PATH} replace />;
    if (status === "anonymous") return <Navigate to={LOGIN_PATH} replace />;
    return <Outlet />;
};

/**
 * The login route itself: a signed-in operator has no reason to see it.
 *
 * This is also where the wizard starts. The database step comes first and only ever
 * yields to one: the account step cannot run without a database to hold the account.
 */
const OnlyAnonymous = () => {
    const { status, needsSetup, reload } = useAuth();

    if (status === "restoring") return <SessionLoading />;
    if (status === "authenticated") return <Navigate to={HOME_PATH} replace />;
    if (status === "unconfigured") return <DatabaseSetup onConfigured={reload} />;
    return needsSetup ? <Setup /> : <Login />;
};

export const routes: RouteObject[] = [
    // Not in `navItems`: the login screen is full-screen by design, so a sidebar
    // entry for it would be a link that leaves the sidebar.
    { path: LOGIN_PATH, element: <OnlyAnonymous /> },
    // The customer mirror: no sidebar, no nav entry — it is opened from the
    // register onto a second monitor, never navigated to by the operator.
    {
        element: <RequireAuth />,
        children: [{ path: "/display", element: <Display /> }],
    },
    {
        element: <RequireAuth />,
        children: [
            {
                element: <AppShell />,
                children: [...children, ...withoutNavEntry],
            },
        ],
    },
];