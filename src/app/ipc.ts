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