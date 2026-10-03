import type { NavItemDividerType, NavItemType } from "@/components/application/app-navigation/config";
import {
    BankNote01,
    BarChart01,
    Building02,
    CreditCard01,
    Cube01,
    Home01,
    LayersTwo01,
    Receipt,
    Settings01,
    ShoppingCart01,
    Tag01,
    Truck01,
    User01,
} from "@untitledui/icons";

/** Module map of the product. `disabled` = no screen yet; hrefs are hash-prefixed. See AGENTS.md. */
type NavEntry = NavItemType | NavItemDividerType;

/** Screens that exist. Anything else in the tree is inert until built. */
export const enabledRoutes = new Set(["/", "/catalog/items", "/catalog/units", "/catalog/brands"]);

export const navItems: NavEntry[] = [
    { label: "Dashboard", href: "#/", icon: Home01 },
    { label: "POS Register", href: "#/pos", icon: ShoppingCart01, disabled: true },
    { divider: true },
    {
        label: "Catalog",
        icon: Cube01,
        items: [
            { label: "Items", href: "#/catalog/items", icon: Tag01 },
            { label: "Categories", href: "#/catalog/categories", icon: LayersTwo01, disabled: true },
            { label: "Brands", href: "#/catalog/brands" },
            { label: "Units", href: "#/catalog/units" },
        ],
    },
    {
        label: "Sales",
        icon: Receipt,
        items: [
            { label: "All Sales", href: "#/sales", disabled: true },
            { label: "Returns", href: "#/sales/returns", disabled: true },
            { label: "Holds", href: "#/sales/holds", disabled: true },
            { label: "Quotations", href: "#/sales/quotations", disabled: true },
            { label: "Registers", href: "#/sales/registers", disabled: true },
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
            { label: "Suppliers", href: "#/purchase/suppliers", disabled: true },
        ],
    },
    { label: "Customers", href: "#/customers", icon: User01, disabled: true },
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
    { label: "Settings", href: "#/settings", icon: Settings01, disabled: true },
];