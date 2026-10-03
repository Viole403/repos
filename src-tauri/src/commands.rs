//! Tauri commands. Every DB-touching command lives here.
//!
//! Conventions:
//! - Payload structs are camelCase on the wire (`rename_all = "camelCase"`), matching
//!   what the TypeScript side sends.
//! - Soft-deleted rows are filtered out of list queries via `del_status = 'Live'`;
//!   deletes set `del_status = 'Deleted'` rather than removing rows.
//! - Money and quantities are `Decimal`, never `f64`.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::atomic::{AtomicU64, Ordering};

use sea_orm::prelude::Decimal;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DbErr, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, TransactionSession, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use crate::db::db;
use crate::entities::catalog::{brand, item, item_category, unit};
use crate::entities::catalog::item::ItemView;
use crate::entities::sales::stock_movement::MovementType;
use crate::entities::sales::{sale, sale_detail, stock_movement};

const LIVE: &str = "Live";
const DELETED: &str = "Deleted";

/// Money and quantities are stored at scale 3 (`migration::DECIMAL_SCALE`). SQLite
/// has no decimal type, so sea-orm round-trips those columns through `f64`; a
/// `SUM()` can therefore come back as `0.30000000000000004` rather than `0.300`.
/// Re-rounding to the declared scale undoes that and is a no-op on Postgres, whose
/// `NUMERIC` sum is already exact.
const MONEY_SCALE: u32 = 3;


// ---------------------------------------------------------------------------
// Shared error type
// ---------------------------------------------------------------------------

/// Errors crossing the IPC boundary. `Serialize` as a plain string so the frontend
/// can render `message` without knowing the variant.
#[derive(Debug, thiserror::Error)]
pub enum CmdError {
    #[error("database error: {0}")]
    Db(#[from] DbErr),
    #[error("{0} not found")]
    NotFound(String),
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    Conflict(String),
}

impl Serialize for CmdError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type CmdResult<T> = Result<T, CmdError>;

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageQuery {
    #[serde(default)]
    pub page: u64,
    /// Clamped to 1..=500 so a client can't ask for the whole table by accident.
    #[serde(default)]
    pub per_page: u64,
    #[serde(default)]
    pub search: Option<String>,
}

impl PageQuery {
    /// Page numbers are 1-based from the UI; guard against 0.
    fn page(&self) -> u64 {
        self.page.max(1)
    }

    fn per_page(&self) -> u64 {
        self.per_page.clamp(1, 500)
    }

    fn offset(&self) -> u64 {
        (self.page() - 1) * self.per_page()
    }

    /// Lowercased search term, or `None` when blank.
    fn term(&self) -> Option<String> {
        self.search
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_lowercase)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub rows: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub per_page: u64,
}

impl<T> Page<T> {
    fn new(rows: Vec<T>, total: u64, q: &PageQuery) -> Self {
        Self {
            rows,
            total,
            page: q.page(),
            per_page: q.per_page(),
        }
    }
}

// ---------------------------------------------------------------------------
// Units
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitInput {
    pub unit_name: String,
    pub description: Option<String>,
}

#[tauri::command]
pub async fn list_units(query: PageQuery) -> CmdResult<Page<unit::Model>> {
    let db = db();
    let mut q = unit::Entity::find().filter(unit::Column::DelStatus.eq(LIVE));

    if let Some(term) = query.term() {
        q = q.filter(unit::Column::UnitName.contains(like_term(&term)));
    }

    let total = q.clone().count(db).await?;
    let rows = q
        .order_by_asc(unit::Column::UnitName)
        .offset(query.offset())
        .limit(query.per_page())
        .all(db)
        .await?;

    Ok(Page::new(rows, total, &query))
}

#[tauri::command]
pub async fn create_unit(input: UnitInput) -> CmdResult<unit::Model> {
    let name = required(&input.unit_name, "unit name")?;
    let db = db();

    let existing = unit::Entity::find()
        .filter(unit::Column::UnitName.eq(&name))
        .filter(unit::Column::DelStatus.eq(LIVE))
        .one(db)
        .await?;
    if existing.is_some() {
        return Err(CmdError::Conflict(format!("unit '{name}' already exists")));
    }

    let now = chrono::Utc::now();
    Ok(unit::ActiveModel {
        unit_name: Set(name),
        description: Set(input.description),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

// ---------------------------------------------------------------------------
// Brands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrandInput {
    pub name: String,
    pub description: Option<String>,
}

#[tauri::command]
pub async fn delete_unit(id: i32) -> CmdResult<()> {
    let db = db();
    let Some(found) = unit::Entity::find_by_id(id).one(db).await? else {
        return Err(CmdError::NotFound("unit".into()));
    };
    let mut model: unit::ActiveModel = found.into();
    model.del_status = Set(DELETED.to_owned());
    model.updated_at = Set(chrono::Utc::now());
    model.update(db).await?;
    Ok(())
}

#[tauri::command]
pub async fn list_brands(query: PageQuery) -> CmdResult<Page<brand::Model>> {
    let db = db();
    let mut q = brand::Entity::find().filter(brand::Column::DelStatus.eq(LIVE));

    if let Some(term) = query.term() {
        q = q.filter(brand::Column::Name.contains(like_term(&term)));
    }

    let total = q.clone().count(db).await?;
    let rows = q
        .order_by_asc(brand::Column::Name)
        .offset(query.offset())
        .limit(query.per_page())
        .all(db)
        .await?;

    Ok(Page::new(rows, total, &query))
}

#[tauri::command]
pub async fn create_brand(input: BrandInput) -> CmdResult<brand::Model> {
    let name = required(&input.name, "brand name")?;
    let db = db();

    let existing = brand::Entity::find()
        .filter(brand::Column::Name.eq(&name))
        .filter(brand::Column::DelStatus.eq(LIVE))
        .one(db)
        .await?;
    if existing.is_some() {
        return Err(CmdError::Conflict(format!("brand '{name}' already exists")));
    }

    let now = chrono::Utc::now();
    Ok(brand::ActiveModel {
        name: Set(name),
        description: Set(input.description),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

// ---------------------------------------------------------------------------
// Item categories
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryInput {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub sort_id: i32,
}

#[tauri::command]
pub async fn delete_brand(id: i32) -> CmdResult<()> {
    let db = db();
    let Some(found) = brand::Entity::find_by_id(id).one(db).await? else {
        return Err(CmdError::NotFound("brand".into()));
    };
    let mut model: brand::ActiveModel = found.into();
    model.del_status = Set(DELETED.to_owned());
    model.updated_at = Set(chrono::Utc::now());
    model.update(db).await?;
    Ok(())
}

#[tauri::command]
pub async fn list_item_categories(query: PageQuery) -> CmdResult<Page<item_category::Model>> {
    let db = db();
    let mut q = item_category::Entity::find().filter(item_category::Column::DelStatus.eq(LIVE));

    if let Some(term) = query.term() {
        q = q.filter(item_category::Column::Name.contains(like_term(&term)));
    }

    let total = q.clone().count(db).await?;
    let rows = q
        // Manual sort order first, then name.
        .order_by_asc(item_category::Column::SortId)
        .order_by_asc(item_category::Column::Name)
        .offset(query.offset())
        .limit(query.per_page())
        .all(db)
        .await?;

    Ok(Page::new(rows, total, &query))
}

#[tauri::command]
pub async fn create_item_category(input: CategoryInput) -> CmdResult<item_category::Model> {
    let name = required(&input.name, "category name")?;
    let db = db();

    let existing = item_category::Entity::find()
        .filter(item_category::Column::Name.eq(&name))
        .filter(item_category::Column::DelStatus.eq(LIVE))
        .one(db)
        .await?;
    if existing.is_some() {
        return Err(CmdError::Conflict(format!("item category '{name}' already exists")));
    }

    let now = chrono::Utc::now();
    Ok(item_category::ActiveModel {
        name: Set(name),
        description: Set(input.description),
        sort_id: Set(input.sort_id),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

#[tauri::command]
pub async fn delete_item_category(id: i32) -> CmdResult<()> {
    let db = db();
    let Some(found) = item_category::Entity::find_by_id(id).one(db).await? else {
        return Err(CmdError::NotFound("item category".into()));
    };
    let mut model: item_category::ActiveModel = found.into();
    model.del_status = Set(DELETED.to_owned());
    model.updated_at = Set(chrono::Utc::now());
    model.update(db).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemInput {
    pub name: String,
    pub code: String,
    #[serde(default)]
    pub alternative_name: Option<String>,
    #[serde(default)]
    pub generic_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category_id: Option<i32>,
    #[serde(default)]
    pub brand_id: Option<i32>,
    #[serde(default)]
    pub purchase_unit_id: Option<i32>,
    #[serde(default)]
    pub sale_unit_id: Option<i32>,
    #[serde(default = "one")]
    pub conversion_rate: Decimal,
    #[serde(default)]
    pub purchase_price: Decimal,
    #[serde(default)]
    pub sale_price: Decimal,
    #[serde(default)]
    pub whole_sale_price: Option<Decimal>,
    #[serde(default)]
    pub alert_quantity: Option<Decimal>,
    #[serde(default)]
    pub loyalty_point: Decimal,
}

fn one() -> Decimal {
    Decimal::ONE
}

fn required(raw: &str, field: &str) -> CmdResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CmdError::Validation(format!("{field} is required")));
    }
    Ok(trimmed.to_owned())
}

fn validate(input: &ItemInput) -> CmdResult<()> {
    required(&input.name, "item name")?;
    required(&input.code, "item code")?;

    // Sale below cost is legal (clearance); below zero never is.
    if input.sale_price < Decimal::ZERO {
        return Err(CmdError::Validation("sale price cannot be negative".into()));
    }
    if input.purchase_price < Decimal::ZERO {
        return Err(CmdError::Validation("purchase price cannot be negative".into()));
    }
    if input.conversion_rate <= Decimal::ZERO {
        return Err(CmdError::Validation(
            "conversion rate must be greater than zero".into(),
        ));
    }
    Ok(())
}

/// Escapes LIKE wildcards so a search for `100%` is a literal match.
fn like_term(raw: &str) -> String {
    format!(
        "%{}%",
        raw.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

/// Marks a row deleted rather than removing it, so historical references stay valid.
///
/// Items and master data share this convention: `del_status` flips to `Deleted` and
/// every list query filters it out. An explicit function per entity rather than a
/// macro because each entity's `ActiveModel` is a distinct type.

#[tauri::command]
pub async fn list_items(query: PageQuery) -> CmdResult<Page<ItemView>> {
    let db = db();

    let rows = item::Entity::find()
        .filter(item::Column::DelStatus.eq(LIVE))
        .all(db)
        .await?;

    // Lookup tables are small and shared across every item, so each is loaded once
    // per request and indexed by id — one round trip each instead of a join per row.
    let categories = item_category::Entity::find().all(db).await?;
    let brands = brand::Entity::find().all(db).await?;
    let units = unit::Entity::find().all(db).await?;
    let categories = by_id(categories.into_iter().map(|r| (r.id, r.name)));
    let brands = by_id(brands.into_iter().map(|r| (r.id, r.name)));
    let units = by_id(units.into_iter().map(|r| (r.id, r.unit_name)));

    let mut views: Vec<ItemView> = rows
        .into_iter()
        .map(|i| ItemView {
            id: i.id,
            name: i.name,
            code: i.code,
            alternative_name: i.alternative_name,
            generic_name: i.generic_name,
            description: i.description,
            category_id: i.category_id,
            category_name: i.category_id.and_then(|id| categories.get(&id).cloned()),
            brand_id: i.brand_id,
            brand_name: i.brand_id.and_then(|id| brands.get(&id).cloned()),
            purchase_unit_id: i.purchase_unit_id,
            purchase_unit_name: i.purchase_unit_id.and_then(|id| units.get(&id).cloned()),
            sale_unit_id: i.sale_unit_id,
            sale_unit_name: i.sale_unit_id.and_then(|id| units.get(&id).cloned()),
            conversion_rate: i.conversion_rate,
            purchase_price: i.purchase_price,
            sale_price: i.sale_price,
            whole_sale_price: i.whole_sale_price,
            alert_quantity: i.alert_quantity,
            loyalty_point: i.loyalty_point,
            photo: i.photo,
        })
        .collect();

    if let Some(term) = query.term() {
        views.retain(|v| {
            v.name.to_lowercase().contains(&term)
                || v.code.to_lowercase().contains(&term)
                || v.alternative_name
                    .as_deref()
                    .is_some_and(|s| s.to_lowercase().contains(&term))
                || v.generic_name
                    .as_deref()
                    .is_some_and(|s| s.to_lowercase().contains(&term))
        });
    }

    let total = views.len() as u64;
    let rows = views
        .into_iter()
        .skip(query.offset() as usize)
        .take(query.per_page() as usize)
        .collect();

    Ok(Page::new(rows, total, &query))
}

/// Turn `(id, label)` pairs into a lookup map for O(1) joins in memory.
fn by_id<K: std::hash::Hash + Eq, V>(pairs: impl IntoIterator<Item = (K, V)>) -> HashMap<K, V> {
    pairs.into_iter().collect()
}

#[tauri::command]
pub async fn create_item(input: ItemInput) -> CmdResult<item::Model> {
    validate(&input)?;
    let db = db();

    let code = required(&input.code, "item code")?;
    let existing = item::Entity::find()
        .filter(item::Column::Code.eq(&code))
        .one(db)
        .await?;
    if existing.is_some() {
        return Err(CmdError::Conflict(format!("item code '{code}' already exists")));
    }

    let now = chrono::Utc::now();
    Ok(item::ActiveModel {
        name: Set(required(&input.name, "item name")?),
        code: Set(code),
        alternative_name: Set(input.alternative_name),
        generic_name: Set(input.generic_name),
        description: Set(input.description),
        category_id: Set(input.category_id),
        brand_id: Set(input.brand_id),
        purchase_unit_id: Set(input.purchase_unit_id),
        sale_unit_id: Set(input.sale_unit_id),
        conversion_rate: Set(input.conversion_rate),
        purchase_price: Set(input.purchase_price),
        sale_price: Set(input.sale_price),
        whole_sale_price: Set(input.whole_sale_price),
        alert_quantity: Set(input.alert_quantity),
        loyalty_point: Set(input.loyalty_point),
        photo: Set(None),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

#[tauri::command]
pub async fn update_item(id: i32, input: ItemInput) -> CmdResult<item::Model> {
    validate(&input)?;
    let db = db();

    let Some(found) = item::Entity::find_by_id(id).one(db).await? else {
        return Err(CmdError::NotFound("item".into()));
    };

    let code = required(&input.code, "item code")?;
    let clash = item::Entity::find()
        .filter(item::Column::Code.eq(&code))
        .filter(item::Column::Id.ne(id))
        .one(db)
        .await?;
    if clash.is_some() {
        return Err(CmdError::Conflict(format!("item code '{code}' already exists")));
    }

    let mut model: item::ActiveModel = found.into();
    model.name = Set(required(&input.name, "item name")?);
    model.code = Set(code);
    model.alternative_name = Set(input.alternative_name);
    model.generic_name = Set(input.generic_name);
    model.description = Set(input.description);
    model.category_id = Set(input.category_id);
    model.brand_id = Set(input.brand_id);
    model.purchase_unit_id = Set(input.purchase_unit_id);
    model.sale_unit_id = Set(input.sale_unit_id);
    model.conversion_rate = Set(input.conversion_rate);
    model.purchase_price = Set(input.purchase_price);
    model.sale_price = Set(input.sale_price);
    model.whole_sale_price = Set(input.whole_sale_price);
    model.alert_quantity = Set(input.alert_quantity);
    model.loyalty_point = Set(input.loyalty_point);
    model.updated_at = Set(chrono::Utc::now());

    Ok(model.update(db).await?)
}

#[tauri::command]
pub async fn delete_item(id: i32) -> CmdResult<()> {
    let db = db();
    let Some(found) = item::Entity::find_by_id(id).one(db).await? else {
        return Err(CmdError::NotFound("item".into()));
    };

    let mut model: item::ActiveModel = found.into();
    model.del_status = Set(DELETED.to_owned());
    model.updated_at = Set(chrono::Utc::now());
    model.update(db).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Sales and the stock ledger
// ---------------------------------------------------------------------------

const SALE_STATUS_DRAFT: &str = "Draft";
const SALE_STATUS_COMPLETED: &str = "Completed";
const SALE_PAYMENT_DEFAULT: &str = "Cash";

/// The ledger row checkout writes. Everything else — receipts, adjustments,
/// transfers — arrives with the stock stage.
const MOVEMENT_SALE: MovementType = MovementType::Sale;

/// On-hand quantity for one item: `SUM(quantity)` over its ledger rows.
///
/// This *is* the on-hand figure. There is no quantity column on `items` to keep in
/// step with it, so this is the only definition of the number and every reader
/// agrees on it by construction. Returns zero for an item that has never moved,
/// because `SUM` over no rows is `NULL`.
async fn on_hand_in<C: ConnectionTrait>(conn: &C, item_id: i32) -> Result<Decimal, DbErr> {
    let sum = stock_movement::Entity::find()
        .select_only()
        .column_as(stock_movement::Column::Quantity.sum(), "total")
        .filter(stock_movement::Column::ItemId.eq(item_id))
        // `SUM` over no rows is NULL rather than 0, so the read is an `Option` and
        // the zero is supplied here — that is also what makes a never-moved item
        // report 0 instead of failing.
        .into_tuple::<Option<Decimal>>()
        .one(conn)
        .await?
        .flatten();

    Ok(sum.unwrap_or(Decimal::ZERO).round_dp(MONEY_SCALE))
}

/// Derive the invoice number from the row's own primary key.
///
/// A sequence-backed primary key is unique by construction, so a function of it is
/// too — which makes this collision-free without a lock, a sequence table, or a
/// retry loop, and therefore correct under concurrent checkouts on any backend.
/// `MAX(invoice_no) + 1` would *not* be: two transactions can read the same MAX.
fn invoice_no_for(id: i32) -> String {
    format!("INV-{id:06}")
}

/// The invoice number written with the INSERT, before the primary key is known.
///
/// It has to satisfy the `UNIQUE` constraint for the moment between the insert and
/// the update, so it must not be a plausible final value. The timestamp keeps
/// distinct runs apart and the counter breaks ties inside one clock tick; this only
/// has to be unique among *uncommitted* rows.
fn provisional_invoice_no() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!(
        "PENDING-{}-{}",
        chrono::Utc::now().timestamp_micros(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

/// Every command below is a two-line shell over an `_in` function that takes a
/// connection. `db()` is a process-wide handle that only exists inside the Tauri
/// window, so threading the connection through is what lets the logic be exercised
/// against a throwaway in-memory database. The IPC surface is unchanged.

#[tauri::command]
pub async fn list_stock_movements(
    item_id: i32,
    query: PageQuery,
) -> CmdResult<Page<stock_movement::Model>> {
    list_stock_movements_in(db(), item_id, query).await
}

pub(crate) async fn list_stock_movements_in<C: ConnectionTrait>(
    conn: &C,
    item_id: i32,
    query: PageQuery,
) -> CmdResult<Page<stock_movement::Model>> {
    let mut q = stock_movement::Entity::find().filter(stock_movement::Column::ItemId.eq(item_id));

    if let Some(term) = query.term() {
        q = q.filter(stock_movement::Column::Reference.contains(like_term(&term)));
    }

    let total = q.clone().count(conn).await?;
    let rows = q
        // Newest first. `id` breaks ties so paging is stable when two movements
        // land inside the same clock tick.
        .order_by_desc(stock_movement::Column::CreatedAt)
        .order_by_desc(stock_movement::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;

    Ok(Page::new(rows, total, &query))
}

#[tauri::command]
pub async fn stock_on_hand(item_id: i32) -> CmdResult<Decimal> {
    stock_on_hand_in(db(), item_id).await
}

pub(crate) async fn stock_on_hand_in<C: ConnectionTrait>(conn: &C, item_id: i32) -> CmdResult<Decimal> {
    Ok(on_hand_in(conn, item_id).await?)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutLine {
    pub item_id: i32,
    /// Must be greater than zero. Fractional quantities are real (2.5 kg).
    pub quantity: Decimal,
    /// The price *at sale time*, from the client. Not the current catalog price —
    /// the till may apply a promotion or a price override that the backend has no
    /// way to know about.
    pub unit_price: Decimal,
    pub discount: Option<Decimal>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutInput {
    pub lines: Vec<CheckoutLine>,
    /// Order-level discount, applied *on top of* the per-line discounts.
    pub discount_total: Option<Decimal>,
    pub tax_total: Option<Decimal>,
    /// Defaults to the grand total, i.e. paid in full. A smaller figure is a
    /// part-paid / credit sale.
    pub paid_total: Option<Decimal>,
    pub payment_method: Option<String>,
    pub note: Option<String>,
    /// `false` leaves the sale as a `Draft` and writes no stock movements: an
    /// unpaid draft must not shrink the shelf. Defaults to `true`.
    pub promote: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemOnHand {
    pub item_id: i32,
    pub quantity: Decimal,
}

/// What checkout returns: the sale, its lines, and the resulting on-hand per item,
/// so the register can refresh without extra round-trips.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleView {
    pub sale: sale::Model,
    pub lines: Vec<sale_detail::Model>,
    /// One entry per distinct item touched by this sale.
    pub stock_on_hand: Vec<ItemOnHand>,
}

fn validate_checkout(input: &CheckoutInput) -> CmdResult<()> {
    if input.lines.is_empty() {
        return Err(CmdError::Validation("a sale needs at least one line".into()));
    }

    // Per-line checks. Line numbers are 1-based in the message because that is how
    // the client indexed them.
    for (i, line) in input.lines.iter().enumerate() {
        let where_ = format!("line {}", i + 1);
        if line.quantity <= Decimal::ZERO {
            return Err(CmdError::Validation(format!(
                "{where_}: quantity must be greater than zero"
            )));
        }
        if line.unit_price < Decimal::ZERO {
            return Err(CmdError::Validation(format!(
                "{where_}: unit price cannot be negative"
            )));
        }
        let discount = line.discount.unwrap_or(Decimal::ZERO);
        if discount < Decimal::ZERO {
            return Err(CmdError::Validation(format!(
                "{where_}: discount cannot be negative"
            )));
        }
        // Without this a discount larger than the line writes a negative
        // `line_total`, and negative money in the ledger is very hard to unwind.
        if discount > line.unit_price * line.quantity {
            return Err(CmdError::Validation(format!(
                "{where_}: discount is larger than the line total"
            )));
        }
    }

    // Order-level checks.
    if input.discount_total.is_some_and(|d| d < Decimal::ZERO) {
        return Err(CmdError::Validation("discount total cannot be negative".into()));
    }
    if input.tax_total.is_some_and(|t| t < Decimal::ZERO) {
        return Err(CmdError::Validation("tax total cannot be negative".into()));
    }
    if input.paid_total.is_some_and(|p| p < Decimal::ZERO) {
        return Err(CmdError::Validation("paid total cannot be negative".into()));
    }
    if let Some(method) = input.payment_method.as_deref() {
        required(method, "payment method")?;
    }

    Ok(())
}

/// Writes a sale, its lines, and the matching ledger rows as one unit.
///
/// The whole thing runs in a single transaction. `DatabaseTransaction`'s `Drop`
/// rolls back, so every early return through `?` leaves the database exactly as it
/// was — a half-written sale with stock already decremented is the bug this command
/// exists to prevent.
#[tauri::command]
pub async fn checkout(input: CheckoutInput) -> CmdResult<SaleView> {
    checkout_in(db(), input).await
}

pub(crate) async fn checkout_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    input: CheckoutInput,
) -> CmdResult<SaleView> {
    validate_checkout(&input)?;

    let promoted = input.promote.unwrap_or(true);
    let now = chrono::Utc::now();

    // TODO(stock): gate the oversell rejection below on an `allow_negative_stock`
    // setting instead. It needs the settings table from the stock stage; until that
    // exists a hard block is the safe default — refusing a sale on a stale count
    // loses real money, but so does an unexplained negative shelf.
    let txn = conn.begin().await?;

    // Blank means "use the default" rather than an error here — the till sends the
    // field whether or not the cashier touched it.
    let payment_method = match input.payment_method.as_deref() {
        None => SALE_PAYMENT_DEFAULT.to_owned(),
        Some(raw) if raw.trim().is_empty() => SALE_PAYMENT_DEFAULT.to_owned(),
        Some(raw) => required(raw, "payment method")?,
    };

    // Insert first with a provisional invoice number, because the real one is
    // derived from the primary key this insert produces.
    let header = sale::ActiveModel {
        invoice_no: Set(provisional_invoice_no()),
        status: Set(
            if promoted {
                SALE_STATUS_COMPLETED.to_owned()
            } else {
                SALE_STATUS_DRAFT.to_owned()
            },
        ),
        subtotal: Set(Decimal::ZERO),
        discount_total: Set(Decimal::ZERO),
        tax_total: Set(Decimal::ZERO),
        grand_total: Set(Decimal::ZERO),
        paid_total: Set(Decimal::ZERO),
        payment_method: Set(payment_method),
        customer_id: Set(None),
        note: Set(input.note),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    let sale_id = header.id;
    let invoice_no = invoice_no_for(sale_id);

    let mut subtotal = Decimal::ZERO;
    let mut line_discount_total = Decimal::ZERO;
    let mut lines = Vec::with_capacity(input.lines.len());
    let mut stock_on_hand: Vec<ItemOnHand> = Vec::with_capacity(input.lines.len());
    // Position in `stock_on_hand` per item, so a cart listing the same item twice
    // reports one row holding the final balance rather than two stale ones.
    let mut seen: HashMap<i32, usize> = HashMap::with_capacity(input.lines.len());

    for line in &input.lines {
        let Some(item) = item::Entity::find_by_id(line.item_id)
            // A soft-deleted item cannot be sold; treating it as missing keeps the
            // till from selling something the catalog screen no longer lists.
            .filter(item::Column::DelStatus.eq(LIVE))
            .one(&txn)
            .await?
        else {
            return Err(CmdError::NotFound(format!("item {}", line.item_id)));
        };

        let discount = line.discount.unwrap_or(Decimal::ZERO);
        let gross = line.unit_price * line.quantity;

        // Summed inside the transaction, so a repeated item in the same cart sees
        // the earlier line's decrement and cannot oversell between itself.
        let available = on_hand_in(&txn, line.item_id).await?;
        if line.quantity > available {
            return Err(CmdError::Validation(format!(
                "item '{}' has {available} in stock but {} was requested",
                item.name, line.quantity
            )));
        }

        lines.push(
            sale_detail::ActiveModel {
                sale_id: Set(sale_id),
                item_id: Set(line.item_id),
                // Snapshot: a later rename must not rewrite the receipt.
                item_name: Set(item.name.clone()),
                unit_price: Set(line.unit_price),
                quantity: Set(line.quantity),
                discount: Set(discount),
                line_total: Set(gross - discount),
                tax_amount: Set(Decimal::ZERO),
                created_at: Set(now),
                ..Default::default()
            }
            .insert(&txn)
            .await?,
        );
        subtotal += gross;
        line_discount_total += discount;

        if promoted {
            // Signed: negative leaves the shelf. `balance_after` is the running
            // on-hand for this item as of this row.
            let balance_after = available - line.quantity;
            stock_movement::ActiveModel {
                item_id: Set(line.item_id),
                sale_id: Set(Some(sale_id)),
                movement_type: Set(MOVEMENT_SALE.as_str().to_owned()),
                quantity: Set(-line.quantity),
                reference: Set(Some(invoice_no.clone())),
                balance_after: Set(balance_after),
                created_at: Set(now),
                ..Default::default()
            }
            .insert(&txn)
            .await?;

            match seen.entry(line.item_id) {
                Entry::Occupied(pos) => stock_on_hand[*pos.get()].quantity = balance_after,
                Entry::Vacant(pos) => {
                    pos.insert(stock_on_hand.len());
                    stock_on_hand.push(ItemOnHand {
                        item_id: line.item_id,
                        quantity: balance_after,
                    });
                }
            }
        }
    }

    // Totals are only known once the lines are, so the header is written twice:
    // once to obtain the primary key, once with the real invoice number and totals.
    // Both statements are in the same transaction, so no reader ever sees the
    // half-filled row.
    let discount_total = line_discount_total + input.discount_total.unwrap_or(Decimal::ZERO);
    // Same reason as the per-line guard: keep negative money out of the ledger.
    if discount_total > subtotal {
        return Err(CmdError::Validation(format!(
            "discount {discount_total} is larger than the subtotal {subtotal}"
        )));
    }
    let tax_total = input.tax_total.unwrap_or(Decimal::ZERO);
    let grand_total = subtotal - discount_total + tax_total;
    let mut header: sale::ActiveModel = header.into();
    header.invoice_no = Set(invoice_no);
    header.subtotal = Set(subtotal);
    header.discount_total = Set(discount_total);
    header.tax_total = Set(tax_total);
    header.grand_total = Set(grand_total);
    header.paid_total = Set(input.paid_total.unwrap_or(grand_total));
    // Only the `Set` fields above reach the SET clause — the rest came in as
    // `Unchanged` from the `Model -> ActiveModel` conversion — so this is a
    // six-column UPDATE keyed on the primary key, not a rewrite of the row.
    let row = header.update(&txn).await?;

    let view = SaleView {
        sale: row,
        lines,
        stock_on_hand,
    };
    // Committed last: everything above is invisible to other readers until here.
    txn.commit().await?;

    Ok(view)
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/// Cheap round-trip confirming the backend is reachable and the schema is present.
/// Counts a table rather than running `SELECT 1`, so a missing schema fails loudly.
#[tauri::command]
pub async fn health_check() -> CmdResult<i64> {
    let db = db();
    let units = unit::Entity::find().count(db).await?;
    Ok(units as i64)
}