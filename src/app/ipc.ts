import { invoke } from "@tauri-apps/api/core";

/**
 * Typed wrapper around the Rust command boundary. Types here mirror the payloads
 * in `src-tauri/src/commands.rs` — check that file before changing anything below,
 * because a mismatch shows up as `undefined` at runtime, not a type error.
 *
 * Two wire conventions worth knowing:
 *
 * - Money and quantities are `Decimal` in Rust, which serde emits as a **string**
 *   to preserve scale. Use `parseFloat`/`toFixed` for display, never arithmetic on
 *   the raw value.
 * - Errors arrive as a bare string: `CmdError` serializes via `serialize_str`.
 *   `call` normalizes both that and a thrown non-string into an `IpcError`.
 */

/** Decimal on the wire: a string, e.g. `"1234.500"`. */
export type Decimal = string;

/** `chrono::DateTime<Utc>` via serde: RFC 3339, e.g. `"2026-10-03T09:15:00Z"`. */
export type Timestamp = string;

/** `del_status` values. Rows are soft-deleted, never removed. */
export type DelStatus = "Live" | "Deleted";

export interface PageQuery {
    /** 1-based; the Rust side clamps 0 to 1. */
    page?: number;
    /** Clamped server-side to 1..=500. */
    perPage?: number;
    /** Blank is treated as no filter. */
    search?: string;
}

export interface Page<T> {
    rows: T[];
    total: number;
    page: number;
    perPage: number;
}

export interface Unit {
    id: number;
    unitName: string;
    description: string | null;
    delStatus: DelStatus;
    createdAt: Timestamp;
    updatedAt: Timestamp;
}

export interface Brand {
    id: number;
    name: string;
    description: string | null;
    delStatus: DelStatus;
    createdAt: Timestamp;
    updatedAt: Timestamp;
}

export interface ItemCategory {
    id: number;
    name: string;
    description: string | null;
    sortId: number;
    delStatus: DelStatus;
    createdAt: Timestamp;
    updatedAt: Timestamp;
}

export interface Item {
    id: number;
    name: string;
    code: string;
    alternativeName: string | null;
    genericName: string | null;
    description: string | null;
    categoryId: number | null;
    brandId: number | null;
    purchaseUnitId: number | null;
    saleUnitId: number | null;
    /** How many purchase units make one sale unit. Defaults to 1 server-side. */
    conversionRate: Decimal;
    purchasePrice: Decimal;
    salePrice: Decimal;
    wholeSalePrice: Decimal | null;
    alertQuantity: Decimal | null;
    loyaltyPoint: Decimal;
    photo: string | null;
    delStatus: DelStatus;
    createdAt: Timestamp;
    updatedAt: Timestamp;
}

/** `Item` plus joined display names, so a list row needs no extra lookups. */
export interface ItemView {
    id: number;
    name: string;
    code: string;
    alternativeName: string | null;
    genericName: string | null;
    description: string | null;
    categoryId: number | null;
    categoryName: string | null;
    brandId: number | null;
    brandName: string | null;
    purchaseUnitId: number | null;
    purchaseUnitName: string | null;
    saleUnitId: number | null;
    saleUnitName: string | null;
    conversionRate: Decimal;
    purchasePrice: Decimal;
    salePrice: Decimal;
    wholeSalePrice: Decimal | null;
    alertQuantity: Decimal | null;
    loyaltyPoint: Decimal;
    photo: string | null;
}

export interface UnitInput {
    unitName: string;
    description?: string | null;
}

export interface BrandInput {
    name: string;
    description?: string | null;
}

export interface CategoryInput {
    name: string;
    description?: string | null;
    sortId?: number;
}

export interface ItemInput {
    name: string;
    code: string;
    alternativeName?: string | null;
    genericName?: string | null;
    description?: string | null;
    categoryId?: number | null;
    brandId?: number | null;
    purchaseUnitId?: number | null;
    saleUnitId?: number | null;
    conversionRate?: Decimal;
    purchasePrice?: Decimal;
    salePrice?: Decimal;
    wholeSalePrice?: Decimal | null;
    alertQuantity?: Decimal | null;
    loyaltyPoint?: Decimal;
}

/**
 * A sale header. No `delStatus`: financial history is voided or refunded, never
 * soft-deleted. `status` is typed `string` to mirror the column, but the only two
 * values written are `Draft` and `Completed`.
 */
export interface Sale {
    id: number;
    invoiceNo: string;
    status: string;
    /** Sum of `unitPrice * quantity` across lines, before any discount. */
    subtotal: Decimal;
    discountTotal: Decimal;
    taxTotal: Decimal;
    /** `subtotal - discountTotal + taxTotal`. */
    grandTotal: Decimal;
    /** Below `grandTotal` is a part-paid sale; above it is change given back. */
    paidTotal: Decimal;
    paymentMethod: string;
    customerId: number | null;
    note: string | null;
    createdAt: Timestamp;
    updatedAt: Timestamp;
}

/** One cart line as written. `itemName` and `unitPrice` are sale-time snapshots. */
export interface SaleDetail {
    id: number;
    saleId: number;
    itemId: number;
    itemName: string;
    unitPrice: Decimal;
    quantity: Decimal;
    discount: Decimal;
    /** `unitPrice * quantity - discount`. */
    lineTotal: Decimal;
    taxAmount: Decimal;
    createdAt: Timestamp;
}

/** One entry per distinct item touched by a sale. */
export interface ItemOnHand {
    itemId: number;
    quantity: Decimal;
}

export interface SaleView {
    sale: Sale;
    lines: SaleDetail[];
    /** Returned so the register can refresh without a second round-trip. */
    stockOnHand: ItemOnHand[];
    /** One per tender. Empty for a single-method sale recorded the pre-split way. */
    payments: SalePayment[];
}

export interface CheckoutLine {
    itemId: number;
    /** Must be greater than zero. Fractional is real (2.5 kg). */
    quantity: Decimal;
    /** The price *at sale time*, from the client. */
    unitPrice: Decimal;
    discount?: Decimal | null;
}

export interface CheckoutInput {
    lines: CheckoutLine[];
    /** Order-level discount, applied on top of the per-line discounts. */
    discountTotal?: Decimal | null;
    taxTotal?: Decimal | null;
    /** Omit to pay in full. A smaller figure is a part-paid / credit sale. */
    paidTotal?: Decimal | null;
    /** `"Cash" | "Card" | "Qris"` in the register; any non-blank string server-side. */
    paymentMethod?: string | null;
    note?: string | null;
    /** `false` leaves the sale as a `Draft` and writes no stock movements. Defaults `true`. */
    promote?: boolean | null;
    /** Who the sale is to. Omit for a walk-in, which is the common case. */
    customerId?: number | null;
    /** One entry per tender. When present these replace `paidTotal` and `paymentMethod`. */
    payments?: PaymentLine[] | null;
}

export interface PaymentLine {
    method: string;
    /** Must be greater than zero. */
    amount: Decimal;
    /** Gateway reference, receipt number, or whatever the tender produces. */
    reference?: string | null;
}

export interface SalePayment {
    id: number;
    saleId: number;
    method: string;
    amount: Decimal;
    reference: string | null;
    createdAt: Timestamp;
}

/** Closed vocabulary, stored and sent in PascalCase so raw SQL stays readable. */
export type MovementType = "Sale" | "SaleReturn" | "GoodsReceipt" | "Adjustment" | "TransferOut" | "TransferIn" | "OpeningBalance";

/**
 * One immutable ledger row. On-hand is derived as `SUM(quantity)`, so `items`
 * carries no quantity column. `quantity` is signed: negative leaves the shelf.
 */
export interface StockMovement {
    id: number;
    itemId: number;
    /** Null once the causing sale is hard-deleted; the movement outlives it. */
    saleId: number | null;
    movementType: MovementType;
    quantity: Decimal;
    /** Receipt number, adjustment reason code, transfer note. */
    reference: string | null;
    /** On-hand immediately after this row landed. */
    balanceAfter: Decimal;
    createdAt: Timestamp;
}

/** A draft and its lines: everything needed to put the cashier back where they were. */
export interface DraftSale {
    sale: Sale;
    lines: SaleDetail[];
}

/** `users::UserView` — the safe projection, so `passwordHash` never crosses the wire. */
export interface UserView {
    id: number;
    name: string;
    email: string;
    phone: string | null;
    role: string | null;
    /** URL or data URI; null falls back to the avatar's own initials. */
    photo: string | null;
}

export interface LoginInput {
    email: string;
    password: string;
}

export class IpcError extends Error {
    constructor(message: string) {
        super(message);
        this.name = "IpcError";
    }
}

/**
 * Tauri rejects with whatever the command returned, which for `CmdError` is a
 * bare string. A missing command also rejects with a plain string rather than an
 * Error, so both are normalized here instead of at every call site.
 */
const call = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
    try {
        return await invoke<T>(command, args);
    } catch (raw) {
        throw new IpcError(typeof raw === "string" ? raw : raw instanceof Error ? raw.message : String(raw));
    }
};

export const healthCheck = () => call<number>("health_check");

// ---------------------------------------------------------------- auth
//
// Permission checks belong on the Rust side — a client-side guard is UI, not a
// boundary. Add a wrapper here only when a command exists to call.

export interface InstallStatus {
    /** No live account exists, so nobody can sign in and the wizard is the only way in. */
    needsSetup: boolean;
    accountCount: number;
}

export interface RoleView {
    id: number;
    name: string;
    /** "Master" bypasses the permission pivot, so `permissions` lists everything. */
    roleType: string;
    permissions: string[];
}

export interface UserInput {
    name: string;
    email: string;
    password: string;
    phone?: string | null;
    role?: string | null;
}

export const installStatus = () => call<InstallStatus>("install_status");
export const login = (input: LoginInput) => call<UserView>("login", { input });
export const logout = () => call<void>("logout");
export const listUsers = () => call<UserView[]>("list_users");
export const createUser = (input: UserInput) => call<UserView>("create_user", { input });
export const deleteUser = (id: number) => call<void>("delete_user", { id });
export const myPermissions = () => call<string[]>("my_permissions");
export const listRoles = () => call<RoleView[]>("list_roles");
export const setUserRole = (userId: number, roleId: number) =>
    call<void>("set_user_role", { userId, roleId });
export const createRole = (name: string) => call<RoleView>("create_role", { input: { name } });
export const setRolePermissions = (roleId: number, permissions: string[]) =>
    call<void>("set_role_permissions", { roleId, permissions });
export const deleteRole = (id: number) => call<void>("delete_role", { id });
/**
 * The signed-in user. The session lives in a process-wide cell, so this is the
 * restore — but note it *rejects* with `NotFound("session")` when nobody is
 * signed in rather than returning null, so callers must treat a throw as "no
 * session". There is no way to tell that apart from any other rejection, since
 * `CmdError` arrives as a bare string.
 */
export const currentUser = () => call<UserView>("current_user");

export interface SaleSummary {
    id: number;
    invoiceNo: string;
    status: string;
    grandTotal: Decimal;
    paidTotal: Decimal;
    paymentMethod: string;
    customerId: number | null;
    /** Null for a walk-in sale, which is not a missing value — it is the common case. */
    customerName: string | null;
    note: string | null;
    createdAt: string;
}

export interface SaleFilter {
    status?: string | null;
    customerId?: number | null;
    /** `YYYY-MM-DD`, inclusive. An unparseable value narrows nothing. */
    from?: string | null;
    to?: string | null;
}

export const listSales = (filter: SaleFilter = {}, query: PageQuery = {}) =>
    call<Page<SaleSummary>>("list_sales", { filter, query });
export const getSale = (id: number) => call<SaleView>("get_sale", { id });
export const listSalePayments = (saleId: number) => call<SalePayment[]>("list_sale_payments", { saleId });

export interface QuotationLine {
    itemId: number;
    quantity: Decimal;
    unitPrice: Decimal;
    discount?: Decimal | null;
}

export interface QuotationInput {
    customerId: number;
    /** `YYYY-MM-DD`. Defaults to today server-side. */
    quotedAt?: string | null;
    referenceNo?: string | null;
    discountTotal?: Decimal | null;
    note?: string | null;
    lines: QuotationLine[];
}

export interface QuotationDetail {
    id: number;
    quotationId: number;
    itemId: number;
    itemName: string;
    quantity: Decimal;
    unitPrice: Decimal;
    discount: Decimal;
    lineTotal: Decimal;
}

export interface QuotationView {
    id: number;
    quotationNo: string;
    customerId: number;
    customerName: string | null;
    quotedAt: Timestamp;
    referenceNo: string | null;
    subtotal: Decimal;
    discountTotal: Decimal;
    grandTotal: Decimal;
    note: string | null;
    createdAt: Timestamp;
    lines: QuotationDetail[];
}

export const listQuotations = (query: PageQuery = {}) => call<Page<QuotationView>>("list_quotations", { query });
export const getQuotation = (id: number) => call<QuotationView>("get_quotation", { id });
export const createQuotation = (input: QuotationInput) => call<QuotationView>("create_quotation", { input });
export const updateQuotation = (id: number, input: QuotationInput) =>
    call<QuotationView>("update_quotation", { id, input });
export const deleteQuotation = (id: number) => call<void>("delete_quotation", { id });

export interface BookingInput {
    customerId: number;
    serviceSellerId?: number | null;
    status?: string | null;
    /** `YYYY-MM-DDTHH:MM`, as a datetime-local field sends it. */
    startAt: string;
    endAt: string;
    note?: string | null;
}

export interface BookingFilter {
    status?: string | null;
    customerId?: number | null;
    /** `YYYY-MM-DD`, inclusive. Unparseable narrows nothing. */
    from?: string | null;
    to?: string | null;
}

export interface BookingView {
    id: number;
    customerId: number;
    customerName: string | null;
    serviceSellerId: number | null;
    serviceSellerName: string | null;
    status: string;
    startAt: Timestamp;
    endAt: Timestamp;
    note: string | null;
    createdAt: Timestamp;
}

/** Closed vocabulary, mirrored from the server. Anything else is refused. */
export const BOOKING_STATUSES = ["Booked", "Waiting", "Completed", "Cancelled"] as const;

export const listBookings = (filter: BookingFilter = {}, query: PageQuery = {}) =>
    call<Page<BookingView>>("list_bookings", { filter, query });
export const getBooking = (id: number) => call<BookingView>("get_booking", { id });
export const createBooking = (input: BookingInput) => call<BookingView>("create_booking", { input });
export const updateBooking = (id: number, input: BookingInput) =>
    call<BookingView>("update_booking", { id, input });
export const deleteBooking = (id: number) => call<void>("delete_booking", { id });

export interface ReturnLine {
    /** Which line of the original sale this reverses. */
    saleDetailId: number;
    quantity: Decimal;
}

export interface ReturnInput {
    saleId: number;
    /** Must be one of the closed `RETURN_REASONS` list. */
    reason: string;
    note?: string | null;
    lines: ReturnLine[];
}

export interface SaleReturnLine {
    id: number;
    saleReturnId: number;
    saleDetailId: number;
    itemId: number;
    /** Snapshotted from the sale line, so a rename since does not rewrite it. */
    itemName: string;
    quantity: Decimal;
    unitPrice: Decimal;
    amount: Decimal;
}

export interface SaleReturn {
    id: number;
    saleId: number;
    returnNo: string;
    reason: string;
    refundedTotal: Decimal;
    /** The account that authorised it. */
    returnedBy: number | null;
    note: string | null;
    createdAt: Timestamp;
    lines: SaleReturnLine[];
    /** Returned by the write path so the till can refresh without a round trip. */
    stockOnHand: ItemOnHand[];
}

/** Closed vocabulary, mirrored from the server. The server refuses anything else. */
export const RETURN_REASONS = [
    "Damaged",
    "Wrong item",
    "Customer changed mind",
    "Not as described",
    "Expired",
    "Other",
] as const;

export const createReturn = (input: ReturnInput) => call<SaleReturn>("create_return", { input });
export const listReturns = (saleId?: number | null) => call<SaleReturn[]>("list_returns", { saleId: saleId ?? null });

// ---------------------------------------------------------------- trade
export interface CustomerView {
    id: number;
    name: string;
    code: string | null;
    email: string | null;
    phone: string | null;
    address: string | null;
    city: string | null;
    country: string | null;
    zip: string | null;
    taxNumber: string | null;
    creditLimit: Decimal;
    loyaltyPoints: Decimal;
    note: string | null;
    photo: string | null;
    createdAt: string;
    /** Completed sales less receipts. Derived on the server, never stored. */
    balance: Decimal;
    /** What they may still take on credit. Negative once they are over the limit. */
    creditAvailable: Decimal;
}

export interface CustomerInput {
    name: string;
    code?: string | null;
    email?: string | null;
    phone?: string | null;
    address?: string | null;
    city?: string | null;
    country?: string | null;
    zip?: string | null;
    taxNumber?: string | null;
    creditLimit: Decimal;
    loyaltyPoints: Decimal;
    note?: string | null;
}

export interface SupplierView {
    id: number;
    name: string;
    code: string | null;
    email: string | null;
    phone: string | null;
    address: string | null;
    city: string | null;
    country: string | null;
    zip: string | null;
    taxNumber: string | null;
    openingBalance: Decimal;
    note: string | null;
    photo: string | null;
    createdAt: string;
    /** What the shop owes them. Positive means the shop is in debt. */
    balance: Decimal;
}

export interface SupplierInput {
    name: string;
    code?: string | null;
    email?: string | null;
    phone?: string | null;
    address?: string | null;
    city?: string | null;
    country?: string | null;
    zip?: string | null;
    taxNumber?: string | null;
    openingBalance: Decimal;
    note?: string | null;
}

/** Money in, with the date it arrived. The backend defaults `paidAt` to now. */
export interface PaymentInput {
    amount: Decimal;
    reference?: string | null;
    paidAt?: string | null;
}

export interface CustomerReceipt {
    id: number;
    customerId: number;
    amount: Decimal;
    reference: string | null;
    paidAt: string;
    createdAt: string;
}

export interface SupplierPayment {
    id: number;
    supplierId: number;
    amount: Decimal;
    reference: string | null;
    paidAt: string;
    createdAt: string;
}

export const listCustomers = (query: PageQuery = {}) => call<Page<CustomerView>>("list_customers", { query });
export const createCustomer = (input: CustomerInput) => call<CustomerView>("create_customer", { input });
export const updateCustomer = (id: number, input: CustomerInput) =>
    call<CustomerView>("update_customer", { id, input });
export const deleteCustomer = (id: number) => call<void>("delete_customer", { id });
export const customerBalance = (id: number) => call<Decimal>("customer_balance", { id });
export const recordCustomerReceipt = (customerId: number, input: PaymentInput) =>
    call<CustomerReceipt>("record_customer_receipt", { customerId, input });
export const listCustomerReceipts = (customerId: number) =>
    call<CustomerReceipt[]>("list_customer_receipts", { customerId });

export const listSuppliers = (query: PageQuery = {}) => call<Page<SupplierView>>("list_suppliers", { query });
export const createSupplier = (input: SupplierInput) => call<SupplierView>("create_supplier", { input });
export const updateSupplier = (id: number, input: SupplierInput) =>
    call<SupplierView>("update_supplier", { id, input });
export const deleteSupplier = (id: number) => call<void>("delete_supplier", { id });
export const supplierBalance = (id: number) => call<Decimal>("supplier_balance", { id });
export const recordSupplierPayment = (supplierId: number, input: PaymentInput) =>
    call<SupplierPayment>("record_supplier_payment", { supplierId, input });
export const listSupplierPayments = (supplierId: number) =>
    call<SupplierPayment[]>("list_supplier_payments", { supplierId });

export const listUnits = (query: PageQuery = {}) => call<Page<Unit>>("list_units", { query });
export const createUnit = (input: UnitInput) => call<Unit>("create_unit", { input });
export const deleteUnit = (id: number) => call<void>("delete_unit", { id });

export const listBrands = (query: PageQuery = {}) => call<Page<Brand>>("list_brands", { query });
export const createBrand = (input: BrandInput) => call<Brand>("create_brand", { input });
export const deleteBrand = (id: number) => call<void>("delete_brand", { id });

export const listItemCategories = (query: PageQuery = {}) => call<Page<ItemCategory>>("list_item_categories", { query });
export const createItemCategory = (input: CategoryInput) => call<ItemCategory>("create_item_category", { input });
export const deleteItemCategory = (id: number) => call<void>("delete_item_category", { id });

export const listItems = (query: PageQuery = {}) => call<Page<ItemView>>("list_items", { query });
export const createItem = (input: ItemInput) => call<Item>("create_item", { input });
export const updateItem = (id: number, input: ItemInput) => call<Item>("update_item", { id, input });
export const deleteItem = (id: number) => call<void>("delete_item", { id });

export const listDraftSales = () => call<DraftSale[]>("list_draft_sales");

/** Completes a draft. Omitting `paidTotal` means paid in full. */
export const promoteDraft = (saleId: number, paidTotal?: Decimal, paymentMethod?: string) =>
    call<SaleView>("promote_draft", { saleId, paidTotal: paidTotal ?? null, paymentMethod: paymentMethod ?? null });

export const discardDraft = (saleId: number) => call<void>("discard_draft", { saleId });

export interface MethodTotal {
    method: string;
    amount: Decimal;
}

export interface RegisterView {
    id: number;
    status: string;
    openedAt: Timestamp;
    closedAt: Timestamp | null;
    openingBalance: Decimal;
    openingDetails: MethodTotal[] | null;
    closingBalance: Decimal | null;
    expectedBalance: Decimal | null;
    /** Counted minus expected. Negative means the drawer is short. */
    variance: Decimal | null;
    note: string | null;
}

export interface RegisterSummary {
    salesCount: number;
    salesTotal: Decimal;
    collectedTotal: Decimal;
    cashTotal: Decimal;
    otherTotal: Decimal;
    refundedTotal: Decimal;
    receiptsTotal: Decimal;
    expectedBalance: Decimal;
    methods: MethodTotal[];
}

export const openRegister = (input: { openingBalance: Decimal; openingDetails?: MethodTotal[] | null; note?: string | null }) =>
    call<RegisterView>("open_register", { input });

export const currentRegister = () => call<RegisterView | null>("current_register");

export const listRegisters = () => call<RegisterView[]>("list_registers");

export const registerSummary = () => call<RegisterSummary | null>("register_summary");

export const closeRegister = (input: { closingBalance: Decimal; note?: string | null }) =>
    call<RegisterView>("close_register", { input });

export const listStockMovements = (itemId: number, query: PageQuery = {}) => call<Page<StockMovement>>("list_stock_movements", { itemId, query });
export const stockOnHand = (itemId: number) => call<Decimal>("stock_on_hand", { itemId });

/**
 * Writes the sale, its lines and the stock movements as one transaction. The
 * server recomputes every total and rejects an oversell or a non-positive
 * quantity, so this is the authority — client totals are for display only.
 */
export const checkout = (input: CheckoutInput) => call<SaleView>("checkout", { input });