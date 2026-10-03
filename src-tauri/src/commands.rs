//! Tauri commands. Every DB-touching command lives here.
//!
//! Conventions:
//! - Payload structs are camelCase on the wire (`rename_all = "camelCase"`), matching
//!   what the TypeScript side sends.
//! - Soft-deleted rows are filtered out of list queries via `del_status = 'Live'`;
//!   deletes set `del_status = 'Deleted'` rather than removing rows.
//! - Money and quantities are `Decimal`, never `f64`.

use std::collections::HashMap;

use sea_orm::prelude::Decimal;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};

use crate::db::db;
use crate::entities::catalog::{brand, item, item_category, unit};
use crate::entities::catalog::item::ItemView;

const LIVE: &str = "Live";
const DELETED: &str = "Deleted";

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