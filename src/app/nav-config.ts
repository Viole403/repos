import type { NavItemDividerType, NavItemType } from "@/components/application/app-navigation/config";
import {
    BankNote01,
    BarChart01,
    Building02,
    CreditCard01,
    Cube01,
    Home01,
    Key01,
    LayersTwo01,
    Receipt,
    Settings01,
    ShoppingCart01,
    Tag01,
    Truck01,
    User01,
} from "@untitledui/icons";

/**
 * Module map of the product. `disabled` = no screen yet; hrefs are hash-prefixed. See
 * AGENTS.md.
 *
 * `permission` hides an entry from an operator who cannot use it. Only the
 * administrative screens declare one: the trading screens are what a cashier opens,
 * and hiding the catalog from someone who can only sell would leave them with a
 * register and nothing to ring up.
 */
interface NavEntryItem extends Omit<NavItemType, "items"> {
    href: string;
    permission?: string;
}

interface NavEntryGroup extends Omit<NavItemType, "items" | "href"> {
    items: NavEntryItem[];
}

type NavEntry = NavEntryItem | NavEntryGroup | NavItemDividerType;

const isDivider = (entry: NavEntry): entry is NavItemDividerType =>
    "divider" in entry && Boolean(entry.divider);

const isGroup = (entry: NavEntry): entry is NavEntryGroup =>
    "items" in entry && Array.isArray(entry.items);

const allowed = (permissions: string[], permission?: string): boolean =>
    !permission || permissions.length === 0 || permissions.includes(permission);

/** Screens that exist. Anything else in the tree is inert until built. */
export const enabledRoutes = new Set(["/", "/pos", "/sales/holds", "/sales/registers", "/sales/quotations", "/sales/bookings", "/sales/promotions", "/sales/installments", "/sales/warranties", "/sales/servicings", "/sales/gift-cards", "/catalog/items", "/catalog/units", "/catalog/brands", "/catalog/categories", "/catalog/combos", "/settings/accounts", "/settings/roles", "/customers", "/purchase/suppliers", "/sales"]);

export const navItems: NavEntry[] = [
    { label: "Dashboard", href: "#/", icon: Home01 },
    { label: "POS Register", href: "#/pos", icon: ShoppingCart01 },
    { divider: true },
    {
        label: "Catalog",
        icon: Cube01,
        items: [
            { label: "Items", href: "#/catalog/items", icon: Tag01 },
            { label: "Categories", href: "#/catalog/categories", icon: LayersTwo01 },
            { label: "Brands", href: "#/catalog/brands" },
            { label: "Units", href: "#/catalog/units" },
            { label: "Combos", href: "#/catalog/combos" },
        ],
    },
    {
        label: "Sales",
        icon: Receipt,
        items: [
            { label: "All Sales", href: "#/sales" },
            { label: "Returns", href: "#/sales/returns", disabled: true },
            { label: "Holds", href: "#/sales/holds" },
            { label: "Quotations", href: "#/sales/quotations" },
            { label: "Bookings", href: "#/sales/bookings" },
            { label: "Promotions", href: "#/sales/promotions" },
            { label: "Installments", href: "#/sales/installments" },
            { label: "Warranties", href: "#/sales/warranties" },
            { label: "Servicing", href: "#/sales/servicings" },
            { label: "Gift Cards", href: "#/sales/gift-cards" },
            { label: "Registers", href: "#/sales/registers" },
        ],
    },
    {
        label: "Stock",
        icon: Truck01,
        items: [
            { label: "Stock on Hand", href: "#/stock", disabled: true },
            { label: "Stock Counts", href: "#/stock/counts", disabled: true },
            { label: "Transfers", href: "#/stock/transfers", disabled: true },
            { label: "Barcode Settings", href: "#/stock/barcodes", disabled: true },
        ],
    },
    {
        label: "Purchase",
        icon: Building02,
        items: [
            { label: "Purchases", href: "#/purchase", disabled: true },
            { label: "Suppliers", href: "#/purchase/suppliers", icon: Truck01 },
        ],
    },
    { label: "Customers", href: "#/customers", icon: User01 },
    {
        label: "Accounting",
        icon: BankNote01,
        items: [
            { label: "Incomes", href: "#/accounting/incomes", disabled: true },
            { label: "Expenses", href: "#/accounting/expenses", disabled: true },
        ],
    },
    { label: "Payments", href: "#/payments", icon: CreditCard01, disabled: true },
    { label: "Reports", href: "#/reports", icon: BarChart01, disabled: true },
    { divider: true },
    {
        label: "Settings",
        icon: Settings01,
        items: [
            { label: "Accounts", href: "#/settings/accounts", icon: User01, permission: "user-list" },
            { label: "Roles", href: "#/settings/roles", icon: Key01, permission: "role-list" },
        ],
    },
];
/**
 * The nav tree with entries the operator cannot use removed. A group whose children are
 * all hidden is dropped rather than left as a heading that goes nowhere.
 *
 * `permissions` empty means "unknown" — before the round trip, or if it failed — and
 * then nothing is hidden. Showing a screen that will reject is better than hiding the
 * app from a signed-in operator, and the backend guard is the boundary either way.
 */
export const visibleNavItems = (permissions: string[]): (NavItemType | NavItemDividerType)[] => {
    // `permission` is ours, not the vendor's, so it is stripped on the way out.
    const strip = ({ permission: _permission, ...rest }: NavEntryItem): NavItemType => rest;

    return navItems
        .map((entry) =>
            isGroup(entry)
                ? {
                      ...entry,
                      items: entry.items
                          .filter((child) => allowed(permissions, child.permission))
                          .map(strip),
                  }
                : isDivider(entry)
                  ? entry
                  : allowed(permissions, entry.permission)
                    ? strip(entry)
                    : null,
        )
        .filter((entry): entry is NavItemType | NavItemDividerType => entry !== null)
        .filter((entry) => entry.items === undefined || entry.items.length > 0);
};
