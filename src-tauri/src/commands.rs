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

use chrono::NaiveDateTime;
use sea_orm::prelude::Decimal;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, Condition, ConnectionTrait, DbErr,
    EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, TransactionSession,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};

use crate::db::db;
use crate::entities::auth::users;
use crate::entities::catalog::{brand, item, item_category, unit};
use crate::entities::catalog::item::ItemView;
use crate::entities::sales::stock_movement::MovementType;
use crate::entities::sales::{booking, combo_item, combo_sale, installment_sale, installment_sale_detail, promotion, quotation, quotation_detail, register, sale, sale_detail, sale_payment, sale_return, sale_return_detail, servicing, stock_movement, warranty};
use crate::entities::trade::{customer, customer_receive, supplier, supplier_payment};

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
    /// The signed-in operator is authenticated but not allowed to do this.
    ///
    /// Distinct from `Validation` on purpose: a rejected *value* is the caller's
    /// mistake to fix, while this says the caller's *account* cannot perform the
    /// action. The frontend hides what a permission forbids, so a surface hit means
    /// the UI and the guard disagree.
    #[error("{0}")]
    Forbidden(String),
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
// Customers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerInput {
    pub name: String,
    pub code: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub zip: Option<String>,
    pub tax_number: Option<String>,
    pub credit_limit: Decimal,
    pub loyalty_points: Decimal,
    pub note: Option<String>,
}

fn validate_customer(input: &CustomerInput) -> CmdResult<()> {
    required(&input.name, "customer name")?;
    if input.credit_limit < Decimal::ZERO {
        return Err(CmdError::Validation("credit limit cannot be negative".into()));
    }
    if input.loyalty_points < Decimal::ZERO {
        return Err(CmdError::Validation("loyalty points cannot be negative".into()));
    }
    Ok(())
}

/// Blank text becomes null rather than an empty string the server re-trims on write.
fn text(raw: Option<String>) -> Option<String> {
    raw.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

#[tauri::command]
pub async fn list_customers(query: PageQuery) -> CmdResult<Page<CustomerView>> {
    crate::commands_auth::require_permission(db(), "customer-list").await?;
    let db = db();
    let mut q = customer::Entity::find().filter(customer::Column::DelStatus.eq(LIVE));

    if let Some(term) = query.term() {
        let like = like_term(&term);
        q = q.filter(
            Condition::any()
                .add(customer::Column::Name.contains(like.clone()))
                .add(customer::Column::Code.contains(like.clone()))
                .add(customer::Column::Phone.contains(like.clone()))
                .add(customer::Column::Email.contains(like)),
        );
    }

    let total = q.clone().count(db).await?;
    let rows = q
        .order_by_asc(customer::Column::Name)
        .offset(query.offset())
        .limit(query.per_page())
        .all(db)
        .await?;

    // The balance is summed per row rather than joined, so a page of 20 costs 20
    // aggregates rather than a correlated subquery per column of every row.
    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        let balance = customer_balance_in(db, row.id).await?;
        views.push(CustomerView::from_row(row, balance));
    }
    Ok(Page::new(views, total, &query))
}

#[tauri::command]
pub async fn create_customer(input: CustomerInput) -> CmdResult<CustomerView> {
    crate::commands_auth::require_permission(db(), "customer-create").await?;
    create_customer_in(db(), input).await
}

pub async fn create_customer_in<C: ConnectionTrait>(conn: &C, input: CustomerInput) -> CmdResult<CustomerView> {
    validate_customer(&input)?;
    let name = required(&input.name, "customer name")?;

    let code = text(input.code.clone());
    if let Some(ref code) = code {
        if customer::Entity::find()
            .filter(customer::Column::Code.eq(code))
            .filter(customer::Column::DelStatus.eq(LIVE))
            .one(conn)
            .await?
            .is_some()
        {
            return Err(CmdError::Conflict(format!("customer code {code} is already used")));
        }
    }

    let row = customer::ActiveModel {
        name: Set(name),
        code: Set(code),
        email: Set(text(input.email.clone())),
        phone: Set(text(input.phone.clone())),
        address: Set(text(input.address.clone())),
        city: Set(text(input.city.clone())),
        country: Set(text(input.country.clone())),
        zip: Set(text(input.zip.clone())),
        tax_number: Set(text(input.tax_number.clone())),
        credit_limit: Set(input.credit_limit),
        loyalty_points: Set(input.loyalty_points),
        note: Set(text(input.note.clone())),
        photo: Set(None),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(crate::migration::now()),
        updated_at: Set(crate::migration::now()),
        ..Default::default()
    }
    .insert(conn)
    .await?;

    Ok(CustomerView::from_row(row, Decimal::ZERO))
}

#[tauri::command]
pub async fn update_customer(id: i32, input: CustomerInput) -> CmdResult<CustomerView> {
    crate::commands_auth::require_permission(db(), "customer-edit").await?;
    validate_customer(&input)?;
    let db = db();
    let found = customer::Entity::find_by_id(id)
        .filter(customer::Column::DelStatus.eq(LIVE))
        .one(db)
        .await?
        .ok_or_else(|| CmdError::NotFound("customer".into()))?;

    let balance = customer_balance_in(db, id).await?;
    let mut am: customer::ActiveModel = found.into();
    am.name = Set(required(&input.name, "customer name")?);
    am.email = Set(text(input.email.clone()));
    am.phone = Set(text(input.phone.clone()));
    am.address = Set(text(input.address.clone()));
    am.city = Set(text(input.city.clone()));
    am.country = Set(text(input.country.clone()));
    am.zip = Set(text(input.zip.clone()));
    am.tax_number = Set(text(input.tax_number.clone()));
    am.credit_limit = Set(input.credit_limit);
    am.loyalty_points = Set(input.loyalty_points);
    am.note = Set(text(input.note.clone()));
    am.updated_at = Set(crate::migration::now());
    let row = am.update(db).await?;

    Ok(CustomerView::from_row(row, balance))
}

/// Marks the row deleted rather than removing it, so sales keep naming a customer.
#[tauri::command]
pub async fn delete_customer(id: i32) -> CmdResult<()> {
    crate::commands_auth::require_permission(db(), "customer-destroy").await?;
    delete_customer_in(db(), id).await
}

pub async fn delete_customer_in<C: ConnectionTrait>(conn: &C, id: i32) -> CmdResult<()> {
    let found = customer::Entity::find_by_id(id)
        .filter(customer::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("customer".into()))?;

    // The receipt foreign key restricts, so a customer who has paid cannot be
    // deleted at all — which is the honest answer: their payment history is the
    // reason the row matters.
    if customer_receive::Entity::find()
        .filter(customer_receive::Column::CustomerId.eq(id))
        .one(conn)
        .await?
        .is_some()
    {
        return Err(CmdError::Conflict(
            "this customer has payments recorded and cannot be deleted".into(),
        ));
    }

    let mut am: customer::ActiveModel = found.into();
    am.del_status = Set(DELETED.to_owned());
    am.updated_at = Set(crate::migration::now());
    am.update(conn).await?;
    Ok(())
}

/// A customer plus what they owe.
///
/// `balance` is derived: completed sales less receipts. Nothing stores it, because a
/// stored balance is a read-modify-write that two concurrent sales can interleave.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerView {
    pub id: i32,
    pub name: String,
    pub code: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub zip: Option<String>,
    pub tax_number: Option<String>,
    pub credit_limit: Decimal,
    pub loyalty_points: Decimal,
    pub note: Option<String>,
    pub photo: Option<String>,
    pub created_at: NaiveDateTime,
    pub balance: Decimal,
    /// What they may still take on credit. Negative once they are over the limit.
    pub credit_available: Decimal,
}

impl CustomerView {
    fn from_row(row: customer::Model, balance: Decimal) -> Self {
        Self {
            id: row.id,
            name: row.name,
            code: row.code,
            email: row.email,
            phone: row.phone,
            address: row.address,
            city: row.city,
            country: row.country,
            zip: row.zip,
            tax_number: row.tax_number,
            credit_limit: row.credit_limit,
            loyalty_points: row.loyalty_points,
            note: row.note,
            photo: row.photo,
            created_at: row.created_at,
            balance,
            credit_available: row.credit_limit - balance,
        }
    }
}

/// Completed sales less receipts. Drafts are excluded: a draft is a basket nobody
/// has paid for, so counting it would show a debt that does not exist.
///
/// `SUM` over no rows is `NULL` rather than 0, so both reads are `Option` and the
/// zero is supplied here — same reason as `on_hand_in`.
pub async fn customer_balance_in<C: ConnectionTrait>(conn: &C, customer_id: i32) -> CmdResult<Decimal> {
    let charged = sale::Entity::find()
        .select_only()
        .column_as(sale::Column::GrandTotal.sum(), "total")
        .filter(sale::Column::CustomerId.eq(customer_id))
        .filter(sale::Column::Status.eq("Completed"))
        .into_tuple::<Option<Decimal>>()
        .one(conn)
        .await?
        .flatten()
        .unwrap_or(Decimal::ZERO);

    let paid = customer_receive::Entity::find()
        .select_only()
        .column_as(customer_receive::Column::Amount.sum(), "total")
        .filter(customer_receive::Column::CustomerId.eq(customer_id))
        .into_tuple::<Option<Decimal>>()
        .one(conn)
        .await?
        .flatten()
        .unwrap_or(Decimal::ZERO);

    Ok((charged - paid).round_dp(MONEY_SCALE))
}

#[tauri::command]
pub async fn customer_balance(id: i32) -> CmdResult<Decimal> {
    crate::commands_auth::require_permission(db(), "customer-show").await?;
    customer_balance_in(db(), id).await
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiveInput {
    pub amount: Decimal,
    pub reference: Option<String>,
    /// Optional so the caller can leave it out; defaults to now.
    pub paid_at: Option<NaiveDateTime>,
}

#[tauri::command]
pub async fn record_customer_receipt(customer_id: i32, input: ReceiveInput) -> CmdResult<customer_receive::Model> {
    crate::commands_auth::require_permission(db(), "customer-edit").await?;
    record_customer_receipt_in(db(), customer_id, input).await
}

pub async fn record_customer_receipt_in<C: ConnectionTrait>(
    conn: &C,
    customer_id: i32,
    input: ReceiveInput,
) -> CmdResult<customer_receive::Model> {
    if input.amount <= Decimal::ZERO {
        return Err(CmdError::Validation("amount must be greater than zero".into()));
    }
    customer::Entity::find_by_id(customer_id)
        .filter(customer::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("customer".into()))?;

    Ok(customer_receive::ActiveModel {
        customer_id: Set(customer_id),
        amount: Set(input.amount),
        reference: Set(text(input.reference)),
        paid_at: Set(input.paid_at.unwrap_or_else(crate::migration::now)),
        created_at: Set(crate::migration::now()),
        ..Default::default()
    }
    .insert(conn)
    .await?)
}

#[tauri::command]
pub async fn list_customer_receipts(customer_id: i32) -> CmdResult<Vec<customer_receive::Model>> {
    crate::commands_auth::require_permission(db(), "customer-show").await?;
    let db = db();
    customer_receive::Entity::find()
        .filter(customer_receive::Column::CustomerId.eq(customer_id))
        .order_by_desc(customer_receive::Column::PaidAt)
        .all(db)
        .await
        .map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Suppliers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplierInput {
    pub name: String,
    pub code: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub zip: Option<String>,
    pub tax_number: Option<String>,
    pub opening_balance: Decimal,
    pub note: Option<String>,
}

#[tauri::command]
pub async fn list_suppliers(query: PageQuery) -> CmdResult<Page<SupplierView>> {
    crate::commands_auth::require_permission(db(), "supplier-list").await?;
    let db = db();
    let mut q = supplier::Entity::find().filter(supplier::Column::DelStatus.eq(LIVE));

    if let Some(term) = query.term() {
        let like = like_term(&term);
        q = q.filter(
            Condition::any()
                .add(supplier::Column::Name.contains(like.clone()))
                .add(supplier::Column::Code.contains(like.clone()))
                .add(supplier::Column::Phone.contains(like.clone()))
                .add(supplier::Column::Email.contains(like)),
        );
    }

    let total = q.clone().count(db).await?;
    let rows = q
        .order_by_asc(supplier::Column::Name)
        .offset(query.offset())
        .limit(query.per_page())
        .all(db)
        .await?;

    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        let balance = supplier_balance_in(db, row.id).await?;
        views.push(SupplierView::from_row(row, balance));
    }
    Ok(Page::new(views, total, &query))
}

#[tauri::command]
pub async fn create_supplier(input: SupplierInput) -> CmdResult<SupplierView> {
    crate::commands_auth::require_permission(db(), "supplier-create").await?;
    let db = db();
    let name = required(&input.name, "supplier name")?;

    let code = text(input.code.clone());
    if let Some(ref code) = code {
        if supplier::Entity::find()
            .filter(supplier::Column::Code.eq(code))
            .filter(supplier::Column::DelStatus.eq(LIVE))
            .one(db)
            .await?
            .is_some()
        {
            return Err(CmdError::Conflict(format!("supplier code {code} is already used")));
        }
    }

    let row = supplier::ActiveModel {
        name: Set(name),
        code: Set(code),
        email: Set(text(input.email.clone())),
        phone: Set(text(input.phone.clone())),
        address: Set(text(input.address.clone())),
        city: Set(text(input.city.clone())),
        country: Set(text(input.country.clone())),
        zip: Set(text(input.zip.clone())),
        tax_number: Set(text(input.tax_number.clone())),
        opening_balance: Set(input.opening_balance),
        note: Set(text(input.note.clone())),
        photo: Set(None),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(crate::migration::now()),
        updated_at: Set(crate::migration::now()),
        ..Default::default()
    }
    .insert(db)
    .await?;

    Ok(SupplierView::from_row(row, input.opening_balance))
}

#[tauri::command]
pub async fn update_supplier(id: i32, input: SupplierInput) -> CmdResult<SupplierView> {
    crate::commands_auth::require_permission(db(), "supplier-edit").await?;
    let db = db();
    let found = supplier::Entity::find_by_id(id)
        .filter(supplier::Column::DelStatus.eq(LIVE))
        .one(db)
        .await?
        .ok_or_else(|| CmdError::NotFound("supplier".into()))?;

    let balance = supplier_balance_in(db, id).await?;
    let mut am: supplier::ActiveModel = found.into();
    am.name = Set(required(&input.name, "supplier name")?);
    am.email = Set(text(input.email.clone()));
    am.phone = Set(text(input.phone.clone()));
    am.address = Set(text(input.address.clone()));
    am.city = Set(text(input.city.clone()));
    am.country = Set(text(input.country.clone()));
    am.zip = Set(text(input.zip.clone()));
    am.tax_number = Set(text(input.tax_number.clone()));
    am.opening_balance = Set(input.opening_balance);
    am.note = Set(text(input.note.clone()));
    am.updated_at = Set(crate::migration::now());
    let row = am.update(db).await?;

    Ok(SupplierView::from_row(row, balance))
}

#[tauri::command]
pub async fn delete_supplier(id: i32) -> CmdResult<()> {
    crate::commands_auth::require_permission(db(), "supplier-destroy").await?;
    let db = db();
    let found = supplier::Entity::find_by_id(id)
        .filter(supplier::Column::DelStatus.eq(LIVE))
        .one(db)
        .await?
        .ok_or_else(|| CmdError::NotFound("supplier".into()))?;

    if supplier_payment::Entity::find()
        .filter(supplier_payment::Column::SupplierId.eq(id))
        .one(db)
        .await?
        .is_some()
    {
        return Err(CmdError::Conflict(
            "this supplier has payments recorded and cannot be deleted".into(),
        ));
    }

    let mut am: supplier::ActiveModel = found.into();
    am.del_status = Set(DELETED.to_owned());
    am.updated_at = Set(crate::migration::now());
    am.update(db).await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplierView {
    pub id: i32,
    pub name: String,
    pub code: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub zip: Option<String>,
    pub tax_number: Option<String>,
    pub opening_balance: Decimal,
    pub note: Option<String>,
    pub photo: Option<String>,
    pub created_at: NaiveDateTime,
    /// What we owe them. Positive means the shop is in debt to this supplier.
    pub balance: Decimal,
}

impl SupplierView {
    fn from_row(row: supplier::Model, balance: Decimal) -> Self {
        Self {
            id: row.id,
            name: row.name,
            code: row.code,
            email: row.email,
            phone: row.phone,
            address: row.address,
            city: row.city,
            country: row.country,
            zip: row.zip,
            tax_number: row.tax_number,
            opening_balance: row.opening_balance,
            note: row.note,
            photo: row.photo,
            created_at: row.created_at,
            balance,
        }
    }
}

/// What we owe: the opening balance less payments. Purchases are Stage 6, so nothing
/// else moves this number yet — adding them later is one more term in the sum.
pub async fn supplier_balance_in<C: ConnectionTrait>(conn: &C, supplier_id: i32) -> CmdResult<Decimal> {
    let opening = supplier::Entity::find_by_id(supplier_id)
        .select_only()
        .column(supplier::Column::OpeningBalance)
        .into_tuple::<Decimal>()
        .one(conn)
        .await?
        .unwrap_or(Decimal::ZERO);

    let paid = supplier_payment::Entity::find()
        .select_only()
        .column_as(supplier_payment::Column::Amount.sum(), "total")
        .filter(supplier_payment::Column::SupplierId.eq(supplier_id))
        .into_tuple::<Option<Decimal>>()
        .one(conn)
        .await?
        .flatten()
        .unwrap_or(Decimal::ZERO);

    Ok((opening - paid).round_dp(MONEY_SCALE))
}

#[tauri::command]
pub async fn supplier_balance(id: i32) -> CmdResult<Decimal> {
    crate::commands_auth::require_permission(db(), "supplier-show").await?;
    supplier_balance_in(db(), id).await
}

#[tauri::command]
pub async fn record_supplier_payment(
    supplier_id: i32,
    input: ReceiveInput,
) -> CmdResult<supplier_payment::Model> {
    crate::commands_auth::require_permission(db(), "supplier-edit").await?;
    if input.amount <= Decimal::ZERO {
        return Err(CmdError::Validation("amount must be greater than zero".into()));
    }
    let db = db();
    supplier::Entity::find_by_id(supplier_id)
        .filter(supplier::Column::DelStatus.eq(LIVE))
        .one(db)
        .await?
        .ok_or_else(|| CmdError::NotFound("supplier".into()))?;

    Ok(supplier_payment::ActiveModel {
        supplier_id: Set(supplier_id),
        amount: Set(input.amount),
        reference: Set(text(input.reference)),
        paid_at: Set(input.paid_at.unwrap_or_else(crate::migration::now)),
        created_at: Set(crate::migration::now()),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

#[tauri::command]
pub async fn list_supplier_payments(supplier_id: i32) -> CmdResult<Vec<supplier_payment::Model>> {
    crate::commands_auth::require_permission(db(), "supplier-show").await?;
    let db = db();
    supplier_payment::Entity::find()
        .filter(supplier_payment::Column::SupplierId.eq(supplier_id))
        .order_by_desc(supplier_payment::Column::PaidAt)
        .all(db)
        .await
        .map_err(Into::into)
}

/// How a sale leaves the shop. Closed vocabulary like booking status: a free-text
/// field here would split one channel across spellings in every report.
pub const ORDER_TYPES: &[&str] = &["InStore", "Pickup", "Delivery", "Online"];

/// Omitted or blank means a counter sale — the common case, and the backfill for
/// every sale written before the column existed.
fn resolve_order_type(raw: Option<&str>) -> CmdResult<String> {
    match raw.map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok("InStore".to_owned()),
        Some(order_type) if ORDER_TYPES.contains(&order_type) => Ok(order_type.to_owned()),
        Some(order_type) => Err(CmdError::Validation(format!("{order_type} is not an order type"))),
    }
}

// ---------------------------------------------------------------------------
// Sales
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleFilter {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub customer_id: Option<i32>,
    /// One of the closed order types. An unknown value matches nothing rather than
    /// everything — a typo in the URL should not quietly show the full day's sales.
    #[serde(default)]
    pub order_type: Option<String>,
    /// Inclusive start of the day, as `YYYY-MM-DD`.
    #[serde(default)]
    pub from: Option<String>,
    /// Inclusive end of the day. Compared as a half-open `< next day`, so a sale at
    /// 23:59 is not lost to a `BETWEEN` on dates.
    #[serde(default)]
    pub to: Option<String>,
}

#[tauri::command]
pub async fn get_sale(id: i32) -> CmdResult<SaleView> {
    crate::commands_auth::require_permission(db(), "sale-show").await?;
    get_sale_in(db(), id).await
}

/// One sale with its lines, for the sale detail screen and the receipt.
pub async fn get_sale_in<C: ConnectionTrait>(conn: &C, id: i32) -> CmdResult<SaleView> {
    let sale = sale::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("sale".into()))?;

    let lines = sale_detail::Entity::find()
        .filter(sale_detail::Column::SaleId.eq(id))
        .order_by_asc(sale_detail::Column::Id)
        .all(conn)
        .await?;

    // Distinct items, not one row per line: a cart listing the same item twice should
    // report one balance, which is the figure the sale left behind.
    let mut seen = Vec::new();
    for line in &lines {
        if !seen.contains(&line.item_id) {
            seen.push(line.item_id);
        }
    }
    let mut stock_on_hand = Vec::with_capacity(seen.len());
    for item_id in seen {
        stock_on_hand.push(ItemOnHand { item_id, quantity: on_hand_in(conn, item_id).await? });
    }

    Ok(SaleView { sale, lines, stock_on_hand, payments: list_payments_in(conn, id).await? })
}

/// A sale row with the customer named, so the list does not show ids.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleSummary {
    pub id: i32,
    pub invoice_no: String,
    pub status: String,
    pub grand_total: Decimal,
    pub paid_total: Decimal,
    pub payment_method: String,
    pub customer_id: Option<i32>,
    pub customer_name: Option<String>,
    pub order_type: String,
    pub note: Option<String>,
    pub created_at: NaiveDateTime,
}

#[tauri::command]
pub async fn list_sales(filter: SaleFilter, query: PageQuery) -> CmdResult<Page<SaleSummary>> {
    crate::commands_auth::require_permission(db(), "sale-list").await?;
    list_sales_in(db(), &filter, &query).await
}

pub async fn list_sales_in<C: ConnectionTrait>(
    conn: &C,
    filter: &SaleFilter,
    query: &PageQuery,
) -> CmdResult<Page<SaleSummary>> {
    let mut q = sale::Entity::find();

    if let Some(ref status) = filter.status {
        let trimmed = status.trim();
        if !trimmed.is_empty() {
            q = q.filter(sale::Column::Status.eq(trimmed));
        }
    }
    if let Some(customer_id) = filter.customer_id {
        q = q.filter(sale::Column::CustomerId.eq(customer_id));
    }
    // Closed vocabulary means no `contains` ambiguity: either it names a channel
    // or it matches nothing.
    if let Some(order_type) = filter.order_type.as_deref().map(str::trim).filter(|t| !t.is_empty())
    {
        q = q.filter(sale::Column::OrderType.eq(order_type));
    }
    // A bad date is ignored rather than refused: a filter box with a typo should show
    // the unfiltered list, not an error the cashier has to dismiss before seeing sales.
    if let Some((start, _)) = filter.from.as_deref().and_then(day_bounds) {
        q = q.filter(sale::Column::CreatedAt.gte(start));
    }
    // Half-open on the *next* day, so a sale at 23:59 on the named day is included.
    if let Some((_, next)) = filter.to.as_deref().and_then(day_bounds) {
        q = q.filter(sale::Column::CreatedAt.lt(next));
    }

    let total = q.clone().count(conn).await?;
    let rows = q
        .order_by_desc(sale::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;

    // One query for the page rather than one per row.
    let names = customer_names(conn, &rows).await?;

    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        views.push(SaleSummary {
            customer_name: row.customer_id.and_then(|id| names.get(&id).cloned()),
            id: row.id,
            invoice_no: row.invoice_no,
            status: row.status,
            grand_total: row.grand_total,
            paid_total: row.paid_total,
            payment_method: row.payment_method,
            customer_id: row.customer_id,
            order_type: row.order_type,
            note: row.note,
            created_at: row.created_at,
        });
    }
    Ok(Page::new(views, total, query))
}

/// Midnight at the start of `YYYY-MM-DD`, and midnight at the start of the next day.
/// `None` for anything unparseable, so a typo narrows nothing rather than erroring.
fn day_bounds(raw: &str) -> Option<(NaiveDateTime, NaiveDateTime)> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    let date = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?;
    Some((date.and_hms_opt(0, 0, 0)?, date.succ_opt()?.and_hms_opt(0, 0, 0)?))
}

/// Names for the page's customers, keyed by id. Walk-in sales are simply absent.
async fn customer_names<C: ConnectionTrait>(
    conn: &C,
    rows: &[sale::Model],
) -> CmdResult<std::collections::HashMap<i32, String>> {
    let ids: Vec<i32> = rows.iter().filter_map(|r| r.customer_id).collect();
    if ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let found = customer::Entity::find()
        .filter(customer::Column::Id.is_in(ids))
        .all(conn)
        .await?;
    Ok(found.into_iter().map(|c| (c.id, c.name)).collect())
}

/// One line being handed back.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnLine {
    /// Which line of the original sale this reverses.
    pub sale_detail_id: i32,
    /// May be less than what was sold, for a partial return.
    pub quantity: Decimal,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnInput {
    pub sale_id: i32,
    /// A closed vocabulary rather than free text, so returns group in a report.
    pub reason: String,
    pub note: Option<String>,
    pub lines: Vec<ReturnLine>,
}

/// The `return` reasons a shop needs. A closed set so a report can group them; free
/// text would need string matching to answer "how much came back because it was broken".
pub const RETURN_REASONS: &[&str] = &[
    "Damaged",
    "Wrong item",
    "Customer changed mind",
    "Not as described",
    "Expired",
    "Other",
];

/// A return with its lines and the stock it put back.
///
/// The header fields are repeated rather than nested under a `return` key, so the
/// wire shape matches the table and a screen reads `view.refundedTotal`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnView {
    pub id: i32,
    pub sale_id: i32,
    pub return_no: String,
    pub reason: String,
    pub refunded_total: Decimal,
    pub returned_by: Option<i32>,
    pub note: Option<String>,
    pub created_at: NaiveDateTime,
    pub lines: Vec<sale_return_detail::Model>,
    pub stock_on_hand: Vec<ItemOnHand>,
}

impl From<sale_return::Model> for ReturnView {
    fn from(row: sale_return::Model) -> Self {
        Self {
            id: row.id,
            sale_id: row.sale_id,
            return_no: row.return_no,
            reason: row.reason,
            refunded_total: row.refunded_total,
            returned_by: row.returned_by,
            note: row.note,
            created_at: row.created_at,
            lines: Vec::new(),
            stock_on_hand: Vec::new(),
        }
    }
}

#[tauri::command]
pub async fn create_return(input: ReturnInput) -> CmdResult<ReturnView> {
    crate::commands_auth::require_permission(db(), "sale-create").await?;
    create_return_in(db(), input, crate::auth::current_user_id()).await
}

pub async fn create_return_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    input: ReturnInput,
    returned_by: Option<i32>,
) -> CmdResult<ReturnView> {
    let reason = required(&input.reason, "return reason")?;
    if !RETURN_REASONS.contains(&reason.as_str()) {
        return Err(CmdError::Validation(format!(
            "'{reason}' is not a return reason — the list is closed so reports can group them"
        )));
    }
    if input.lines.is_empty() {
        return Err(CmdError::Validation("a return needs at least one line".into()));
    }

    let txn = conn.begin().await?;

    let original = sale::Entity::find_by_id(input.sale_id)
        .one(&txn)
        .await?
        .ok_or_else(|| CmdError::NotFound("sale".into()))?;
    if original.status != SALE_STATUS_COMPLETED {
        return Err(CmdError::Validation(
            "only a completed sale can be returned against".into(),
        ));
    }

    // Provisional first, for the same reason `sales` does it: the return number is
    // derived from the primary key this insert produces.
    let now = crate::migration::now();
    let header = sale_return::ActiveModel {
        sale_id: Set(original.id),
        return_no: Set(provisional_return_no()),
        reason: Set(reason),
        refunded_total: Set(Decimal::ZERO),
        returned_by: Set(returned_by),
        note: Set(text(input.note.clone())),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    let return_id = header.id;

    let mut lines = Vec::with_capacity(input.lines.len());
    let mut refunded = Decimal::ZERO;
    // (item, qty) pairs the shelf grows by. A bundle line explodes into its
    // components here, prorated from the explosion audit — never from the catalog,
    // whose definition may have changed since the sale.
    let mut back: Vec<(i32, Decimal)> = Vec::new();

    for line in &input.lines {
        if line.quantity <= Decimal::ZERO {
            return Err(CmdError::Validation(format!(
                "return quantity must be greater than zero"
            )));
        }
        let sold = sale_detail::Entity::find_by_id(line.sale_detail_id)
            .filter(sale_detail::Column::SaleId.eq(original.id))
            .one(&txn)
            .await?
            .ok_or_else(|| {
                CmdError::Validation(format!(
                    "line {} does not belong to this sale",
                    line.sale_detail_id
                ))
            })?;

        let already = returned_quantity(&txn, sold.id).await?;
        let remaining = sold.quantity - already;
        if line.quantity > remaining {
            return Err(CmdError::Validation(format!(
                "{} was sold {} and {} has already come back; only {remaining} left",
                sold.item_name, sold.quantity, already
            )));
        }

        let amount = sold.unit_price * line.quantity;
        lines.push(
            sale_return_detail::ActiveModel {
                sale_return_id: Set(return_id),
                sale_detail_id: Set(sold.id),
                item_id: Set(sold.item_id),
                // Snapshotted from the sale line, so a rename since does not rewrite
                // what the customer was told they were returning.
                item_name: Set(sold.item_name.clone()),
                quantity: Set(line.quantity),
                unit_price: Set(sold.unit_price),
                amount: Set(amount),
                ..Default::default()
            }
            .insert(&txn)
            .await?,
        );
        refunded += amount;

        let explosion: Vec<combo_sale::Model> = combo_sale::Entity::find()
            .filter(combo_sale::Column::SaleId.eq(original.id))
            .filter(combo_sale::Column::SaleDetailId.eq(sold.id))
            .all(&txn)
            .await?;
        if explosion.is_empty() {
            back.push((sold.item_id, line.quantity));
        } else {
            for row in explosion {
                back.push((
                    row.item_id,
                    (row.quantity * line.quantity / sold.quantity).round_dp(MONEY_SCALE),
                ));
            }
        }
    }

    let return_no = return_no_for(return_id);
    let mut am: sale_return::ActiveModel = header.into();
    am.return_no = Set(return_no.clone());
    am.refunded_total = Set(refunded.round_dp(MONEY_SCALE));
    let return_row = am.update(&txn).await?;

    // Stock comes back: positive quantity, so the shelf grows by exactly what went out.
    let mut touched: Vec<i32> = Vec::new();
    for (item_id, _) in &back {
        if !touched.contains(item_id) {
            touched.push(*item_id);
        }
    }
    let mut stock_on_hand = Vec::with_capacity(touched.len());
    for item_id in touched {
        let quantity: Decimal = back
            .iter()
            .filter(|(id, _)| *id == item_id)
            .map(|(_, qty)| *qty)
            .sum();
        let balance_after = on_hand_in(&txn, item_id).await? + quantity;
        record_return_movement(&txn, item_id, return_id, &return_no, quantity, balance_after, now).await?;
        stock_on_hand.push(ItemOnHand { item_id, quantity: balance_after });
    }

    let view = ReturnView { lines, stock_on_hand, ..return_row.into() };
    // Committed last. Without this the transaction rolls back on `Drop` and the
    // return, the stock it put back and the ledger row all vanish together.
    txn.commit().await?;

    Ok(view)
}

/// How much of a sale line has already come back. Summed, not stored: a stored
/// counter is a read-modify-write two partial returns can interleave.
pub async fn returned_quantity<C: ConnectionTrait>(conn: &C, sale_detail_id: i32) -> CmdResult<Decimal> {
    let sum = sale_return_detail::Entity::find()
        .select_only()
        .column_as(sale_return_detail::Column::Quantity.sum(), "total")
        .filter(sale_return_detail::Column::SaleDetailId.eq(sale_detail_id))
        .into_tuple::<Option<Decimal>>()
        .one(conn)
        .await?
        .flatten();
    Ok(sum.unwrap_or(Decimal::ZERO))
}

/// The ledger row for goods coming back. Shares nothing with `record_sale_movement` on
/// purpose: the quantity is positive here, and folding both into one function is how a
/// sign error gets in.
async fn record_return_movement<C: ConnectionTrait>(
    conn: &C,
    item_id: i32,
    return_id: i32,
    return_no: &str,
    quantity: Decimal,
    balance_after: Decimal,
    now: NaiveDateTime,
) -> CmdResult<()> {
    stock_movement::ActiveModel {
        item_id: Set(item_id),
        sale_id: Set(None),
        movement_type: Set(MovementType::SaleReturn.as_str().to_owned()),
        quantity: Set(quantity),
        reference: Set(Some(return_no.to_owned())),
        balance_after: Set(balance_after),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(conn)
    .await?;

    Ok(())
}

/// Unique within the process only — the column's own uniqueness comes from the real
/// number, which is derived from the primary key.
fn provisional_return_no() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!(
        "RET-PENDING-{}-{}",
        crate::migration::now().timestamp_micros(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

fn return_no_for(id: i32) -> String {
    format!("RET-{id:06}")
}

#[tauri::command]
pub async fn list_returns(sale_id: Option<i32>) -> CmdResult<Vec<ReturnView>> {
    crate::commands_auth::require_permission(db(), "sale-list").await?;
    list_returns_in(db(), sale_id).await
}

pub async fn list_returns_in<C: ConnectionTrait>(
    conn: &C,
    sale_id: Option<i32>,
) -> CmdResult<Vec<ReturnView>> {
    let mut q = sale_return::Entity::find();
    if let Some(id) = sale_id {
        q = q.filter(sale_return::Column::SaleId.eq(id));
    }
    let rows = q.order_by_desc(sale_return::Column::Id).all(conn).await?;

    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        let lines = sale_return_detail::Entity::find()
            .filter(sale_return_detail::Column::SaleReturnId.eq(row.id))
            .order_by_asc(sale_return_detail::Column::Id)
            .all(conn)
            .await?;
        views.push(ReturnView { lines, stock_on_hand: Vec::new(), ..row.into() });
    }
    Ok(views)
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
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "unit-list").await?;
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
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "unit-create").await?;
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

    let now = crate::migration::now();
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
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "unit-destroy").await?;
    let db = db();
    let Some(found) = unit::Entity::find_by_id(id).one(db).await? else {
        return Err(CmdError::NotFound("unit".into()));
    };
    let mut model: unit::ActiveModel = found.into();
    model.del_status = Set(DELETED.to_owned());
    model.updated_at = Set(crate::migration::now());
    model.update(db).await?;
    Ok(())
}

#[tauri::command]
pub async fn list_brands(query: PageQuery) -> CmdResult<Page<brand::Model>> {
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "brand-list").await?;
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
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "brand-create").await?;
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

    let now = crate::migration::now();
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
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "brand-destroy").await?;
    let db = db();
    let Some(found) = brand::Entity::find_by_id(id).one(db).await? else {
        return Err(CmdError::NotFound("brand".into()));
    };
    let mut model: brand::ActiveModel = found.into();
    model.del_status = Set(DELETED.to_owned());
    model.updated_at = Set(crate::migration::now());
    model.update(db).await?;
    Ok(())
}

#[tauri::command]
pub async fn list_item_categories(query: PageQuery) -> CmdResult<Page<item_category::Model>> {
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "item_category-list").await?;
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
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "item_category-create").await?;
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

    let now = crate::migration::now();
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
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "item_category-destroy").await?;
    let db = db();
    let Some(found) = item_category::Entity::find_by_id(id).one(db).await? else {
        return Err(CmdError::NotFound("item category".into()));
    };
    let mut model: item_category::ActiveModel = found.into();
    model.del_status = Set(DELETED.to_owned());
    model.updated_at = Set(crate::migration::now());
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
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "item-list").await?;
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
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "item-create").await?;
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

    let now = crate::migration::now();
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
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "item-edit").await?;
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
    model.updated_at = Set(crate::migration::now());

    Ok(model.update(db).await?)
}

#[tauri::command]
pub async fn delete_item(id: i32) -> CmdResult<()> {
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "item-destroy").await?;
    let db = db();
    let Some(found) = item::Entity::find_by_id(id).one(db).await? else {
        return Err(CmdError::NotFound("item".into()));
    };

    let mut model: item::ActiveModel = found.into();
    model.del_status = Set(DELETED.to_owned());
    model.updated_at = Set(crate::migration::now());
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
        crate::migration::now().timestamp_micros(),
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
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "stock-stock").await?;
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
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "stock-stock").await?;
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
    /// Who the sale is to. `None` is a walk-in, which is the common case at a
    /// counter and must not be forced through a customer row.
    #[serde(default)]
    pub customer_id: Option<i32>,
    /// How the sale leaves the shop. Omitted means counter sale.
    #[serde(default)]
    pub order_type: Option<String>,
    /// One entry per tender, for a split payment. When present these *replace*
    /// `paid_total` and `payment_method` rather than sitting beside them, so the
    /// figure can only come from one place.
    #[serde(default)]
    pub payments: Option<Vec<PaymentLine>>,
}

/// One tender against a sale.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentLine {
    pub method: String,
    /// Must be greater than zero. A zero "tender" is not a tender.
    pub amount: Decimal,
    /// Gateway reference, receipt number, or whatever the tender produces.
    pub reference: Option<String>,
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
    /// One entry per tender. Empty for a sale paid in a single method recorded the
    /// pre-split way, and for a draft that has not been paid.
    pub payments: Vec<sale_payment::Model>,
}

/// One word for a split sale: `"Cash + Card"` beats `"Split"` because a receipt or a
/// report then names what actually happened.
fn summarise_methods(lines: &[PaymentLine]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for line in lines {
        let method = line.method.trim();
        if !seen.contains(&method) {
            seen.push(method);
        }
    }
    seen.join(" + ")
}

async fn write_payment<C: ConnectionTrait>(
    conn: &C,
    sale_id: i32,
    line: &PaymentLine,
) -> CmdResult<()> {
    sale_payment::ActiveModel {
        sale_id: Set(sale_id),
        method: Set(line.method.trim().to_owned()),
        amount: Set(line.amount),
        reference: Set(text(line.reference.clone())),
        created_at: Set(crate::migration::now()),
        ..Default::default()
    }
    .insert(conn)
    .await?;

    Ok(())
}

#[tauri::command]
pub async fn list_sale_payments(sale_id: i32) -> CmdResult<Vec<sale_payment::Model>> {
    crate::commands_auth::require_permission(db(), "sale-show").await?;
    list_payments_in(db(), sale_id).await
}

pub async fn list_payments_in<C: ConnectionTrait>(conn: &C, sale_id: i32) -> CmdResult<Vec<sale_payment::Model>> {
    Ok(sale_payment::Entity::find()
        .filter(sale_payment::Column::SaleId.eq(sale_id))
        .order_by_asc(sale_payment::Column::Id)
        .all(conn)
        .await?)
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
    resolve_order_type(input.order_type.as_deref())?;
    if input.tax_total.is_some_and(|t| t < Decimal::ZERO) {
        return Err(CmdError::Validation("tax total cannot be negative".into()));
    }
    if input.paid_total.is_some_and(|p| p < Decimal::ZERO) {
        return Err(CmdError::Validation("paid total cannot be negative".into()));
    }
    if let Some(method) = input.payment_method.as_deref() {
        required(method, "payment method")?;
    }
    if let Some(lines) = input.payments.as_ref() {
        validate_payments(lines)?;
    }

    Ok(())
}

/// Every tender must name a method and carry a positive amount. The total is checked
/// against the sale once the grand total is known, not here.
fn validate_payments(lines: &[PaymentLine]) -> CmdResult<()> {
    if lines.is_empty() {
        return Err(CmdError::Validation(
            "no payment lines given — omit the field entirely for a single tender".into(),
        ));
    }
    for (i, line) in lines.iter().enumerate() {
        let where_ = format!("payment {}", i + 1);
        required(&line.method, &format!("{where_} method"))?;
        if line.amount <= Decimal::ZERO {
            return Err(CmdError::Validation(format!(
                "{where_}: amount must be greater than zero"
            )));
        }
    }
    Ok(())
}

/// The gate every write path shares: the item must still be sellable, and the shelf
/// must still cover the line.
///
/// Returns the item's current name and its on-hand quantity so a caller never has to
/// read either again — and, more importantly, so `checkout` and draft promotion
/// cannot drift into disagreeing about what "sellable" means.
///
/// A soft-deleted item is treated as missing rather than as a stock problem, which
/// keeps the till from selling something the catalog screen no longer lists.
async fn guard_line_sellable<C: ConnectionTrait>(
    conn: &C,
    line: &CheckoutLine,
) -> CmdResult<(String, Decimal)> {
    let Some(item) = item::Entity::find_by_id(line.item_id)
        .filter(item::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
    else {
        return Err(CmdError::NotFound(format!("item {}", line.item_id)));
    };

    let available = on_hand_in(conn, line.item_id).await?;
    if line.quantity > available {
        return Err(CmdError::Validation(format!(
            "item '{}' has {available} in stock but {} was requested",
            item.name, line.quantity
        )));
    }

    Ok((item.name, available))
}

/// One ledger row for a sold line. Shared so `checkout` and draft promotion write an
/// identical row for an identical event — the ledger is load-bearing, and two
/// spellings of "a sale happened" would make an audit a matter of opinion.
async fn record_sale_movement<C: ConnectionTrait>(
    conn: &C,
    item_id: i32,
    sale_id: i32,
    invoice_no: &str,
    quantity: Decimal,
    balance_after: Decimal,
    now: NaiveDateTime,
) -> CmdResult<()> {
    // Signed: negative leaves the shelf. `balance_after` is the running on-hand for
    // this item as of this row.
    stock_movement::ActiveModel {
        item_id: Set(item_id),
        sale_id: Set(Some(sale_id)),
        movement_type: Set(MOVEMENT_SALE.as_str().to_owned()),
        quantity: Set(-quantity),
        reference: Set(Some(invoice_no.to_owned())),
        balance_after: Set(balance_after),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(conn)
    .await?;

    Ok(())
}

/// Fold a line's resulting balance into the view's per-item list, so a sale that
/// lists the same item twice reports one row holding the final balance rather than
/// two stale ones.
fn record_on_hand(
    view: &mut Vec<ItemOnHand>,
    seen: &mut HashMap<i32, usize>,
    item_id: i32,
    balance_after: Decimal,
) {
    match seen.entry(item_id) {
        Entry::Occupied(pos) => view[*pos.get()].quantity = balance_after,
        Entry::Vacant(pos) => {
            pos.insert(view.len());
            view.push(ItemOnHand {
                item_id,
                quantity: balance_after,
            });
        }
    }
}

/// Order-level guard shared by checkout and draft promotion: a discount larger than
/// the subtotal writes a negative grand total, and negative money in the ledger is
/// very hard to unwind.
fn guard_discount_within_subtotal(discount_total: Decimal, subtotal: Decimal) -> CmdResult<()> {
    if discount_total > subtotal {
        return Err(CmdError::Validation(format!(
            "discount {discount_total} is larger than the subtotal {subtotal}"
        )));
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
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "sale-create").await?;
    checkout_in(db(), input).await
}

pub(crate) async fn checkout_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    input: CheckoutInput,
) -> CmdResult<SaleView> {
    validate_checkout(&input)?;

    let promoted = input.promote.unwrap_or(true);
    let now = crate::migration::now();

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
    let order_type = resolve_order_type(input.order_type.as_deref())?;

    // Resolved inside the transaction so a customer deleted between the check and the
    // insert cannot end up named on a sale. `None` stays `None` — a walk-in sale is the
    // common case and must not be forced through a customer row.
    let customer_id = match input.customer_id {
        None => None,
        Some(id) => {
            customer::Entity::find_by_id(id)
                .filter(customer::Column::DelStatus.eq(LIVE))
                .one(&txn)
                .await?
                .ok_or_else(|| CmdError::NotFound("customer".into()))?;
            Some(id)
        }
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
        customer_id: Set(customer_id),
        order_type: Set(order_type),
        note: Set(input.note),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    let sale_id = header.id;
    let invoice_no = invoice_no_for(sale_id);

    // Promotions apply at completion, not at park time: a draft stores manual-only
    // lines, and the rules are re-read when it promotes — the catalog may have
    // changed while it waited, same reason the shelf is re-read then.
    let promo_lines: Vec<PromoLine> = input
        .lines
        .iter()
        .map(|line| PromoLine {
            item_id: line.item_id,
            quantity: line.quantity,
            unit_price: line.unit_price,
            manual: line.discount.unwrap_or(Decimal::ZERO),
        })
        .collect();
    let promo = if promoted {
        apply_promotions(&txn, &promo_lines, now).await?
    } else {
        PromoOutcome { line_promos: vec![Decimal::ZERO; promo_lines.len()], order_promo: Decimal::ZERO }
    };

    let mut subtotal = Decimal::ZERO;
    let mut line_discount_total = Decimal::ZERO;
    let mut lines = Vec::with_capacity(input.lines.len());
    let mut stock_on_hand: Vec<ItemOnHand> = Vec::with_capacity(input.lines.len());
    // Position in `stock_on_hand` per item, so a cart listing the same item twice
    // reports one row holding the final balance rather than two stale ones.
    let mut seen: HashMap<i32, usize> = HashMap::with_capacity(input.lines.len());

    for (index, line) in input.lines.iter().enumerate() {
        // Looked up before the guard: a bundle skips the shelf check below, because
        // the virtual item holds no stock and would refuse every bundle sale.
        let is_bundle = !combo_components(&txn, line.item_id).await?.is_empty();
        let (item_name, available) = if is_bundle {
            let item = item::Entity::find_by_id(line.item_id)
                .filter(item::Column::DelStatus.eq(LIVE))
                .one(&txn)
                .await?
                .ok_or_else(|| CmdError::NotFound(format!("item {}", line.item_id)))?;
            (item.name, Decimal::ZERO)
        } else {
            guard_line_sellable(&txn, line).await?
        };
        let discount = line.discount.unwrap_or(Decimal::ZERO) + promo.line_promos[index];
        let gross = line.unit_price * line.quantity;

        let detail = sale_detail::ActiveModel {
            sale_id: Set(sale_id),
            item_id: Set(line.item_id),
            // Snapshot: a later rename must not rewrite the receipt.
            item_name: Set(item_name.clone()),
            unit_price: Set(line.unit_price),
            quantity: Set(line.quantity),
            discount: Set(discount),
            line_total: Set(gross - discount),
            tax_amount: Set(Decimal::ZERO),
            created_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        lines.push(detail.clone());
        subtotal += gross;
        line_discount_total += discount;

        if promoted {
            // A bundle sells as one line but moves its components: the bundle item
            // itself is virtual and holds no stock.
            if is_bundle {
                let needs = guard_combo_sellable(&txn, line.item_id, &item_name, line.quantity).await?;
                record_combo_sale(
                    &txn, sale_id, detail.id, line.item_id, &needs, &invoice_no, now,
                    &mut stock_on_hand, &mut seen,
                )
                .await?;
            } else {
                // `available` was read inside the transaction, so a repeated item in the
                // same cart sees the earlier line's decrement and cannot oversell between
                // itself.
                let balance_after = available - line.quantity;
                record_sale_movement(
                    &txn,
                    line.item_id,
                    sale_id,
                    &invoice_no,
                    line.quantity,
                    balance_after,
                    now,
                )
                .await?;
                record_on_hand(&mut stock_on_hand, &mut seen, line.item_id, balance_after);
            }
        }
    }

    // Totals are only known once the lines are, so the header is written twice:
    // once to obtain the primary key, once with the real invoice number and totals.
    // Both statements are in the same transaction, so no reader ever sees the
    // half-filled row.
    let discount_total = line_discount_total + input.discount_total.unwrap_or(Decimal::ZERO) + promo.order_promo;
    guard_discount_within_subtotal(discount_total, subtotal)?;
    let tax_total = input.tax_total.unwrap_or(Decimal::ZERO);
    let grand_total = subtotal - discount_total + tax_total;
    let mut header: sale::ActiveModel = header.into();
    header.invoice_no = Set(invoice_no);
    header.subtotal = Set(subtotal);
    header.discount_total = Set(discount_total);
    header.tax_total = Set(tax_total);
    header.grand_total = Set(grand_total);
    // Taken from the tenders when they are given, so the figure has one source. A
    // tender list summing above the total is change the cashier holds, not a payment,
    // so that is refused rather than recorded as a negative sale.
    let payments = input.payments.filter(|lines| !lines.is_empty());
    if let Some(lines) = payments.as_ref() {
        let tendered: Decimal = lines.iter().map(|l| l.amount).sum();
        if tendered > grand_total {
            return Err(CmdError::Validation(format!(
                "payments total {tendered}, which is more than the sale total {grand_total}"
            )));
        }
        header.paid_total = Set(tendered);
        header.payment_method = Set(summarise_methods(lines));
    } else {
        header.paid_total = Set(input.paid_total.unwrap_or(grand_total));
    }
    // Only the `Set` fields above reach the SET clause — the rest came in as
    // `Unchanged` from the `Model -> ActiveModel` conversion — so this is a
    // six-column UPDATE keyed on the primary key, not a rewrite of the row.
    let row = header.update(&txn).await?;

    if let Some(lines) = payments.as_ref() {
        for line in lines {
            write_payment(&txn, sale_id, line).await?;
        }
    }

    let view = SaleView {
        sale: row,
        lines,
        stock_on_hand,
        payments: list_payments_in(&txn, sale_id).await?,
    };
    // Committed last: everything above is invisible to other readers until here.
    txn.commit().await?;

    Ok(view)
}

// ---------------------------------------------------------------------------
// The draft lifecycle
//
// `checkout(promote: false)` already writes a recoverable `Draft`: a header, its
// lines, and deliberately no stock movements. These three commands are the other
// half of that promise — reading a draft back after a crash, turning one into a real
// sale, and throwing one away. Without the read-back the draft is unreachable and
// the crash recovery is theoretical.
// ---------------------------------------------------------------------------

/// A draft and its lines: everything needed to put the cashier back where they were.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftSale {
    pub sale: sale::Model,
    pub lines: Vec<sale_detail::Model>,
}

/// Every draft that is worth resuming, newest first.
///
/// An empty draft is omitted: it is a cart the cashier opened and walked away from,
/// and offering one back is noise rather than recovery.
#[tauri::command]
pub async fn list_draft_sales() -> CmdResult<Vec<DraftSale>> {
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "sale-pos").await?;
    list_draft_sales_in(db()).await
}

pub(crate) async fn list_draft_sales_in<C: ConnectionTrait>(conn: &C) -> CmdResult<Vec<DraftSale>> {
    let headers = sale::Entity::find()
        .filter(sale::Column::Status.eq(SALE_STATUS_DRAFT))
        // Newest first, `id` breaking ties so the order is stable when two drafts land
        // inside the same clock tick — the same rule the stock ledger follows.
        .order_by_desc(sale::Column::CreatedAt)
        .order_by_desc(sale::Column::Id)
        .all(conn)
        .await?;

    if headers.is_empty() {
        return Ok(Vec::new());
    }

    // One query for every draft's lines, grouped in memory. A resume screen wants all
    // of them, so a per-draft read would be N round trips for the same rows.
    let ids: Vec<i32> = headers.iter().map(|h| h.id).collect();
    let rows = sale_detail::Entity::find()
        .filter(sale_detail::Column::SaleId.is_in(ids))
        .order_by_asc(sale_detail::Column::Id)
        .all(conn)
        .await?;

    let mut grouped: HashMap<i32, Vec<sale_detail::Model>> = HashMap::new();
    for row in rows {
        grouped.entry(row.sale_id).or_default().push(row);
    }

    let mut out = Vec::with_capacity(headers.len());
    for header in headers {
        let Some(lines) = grouped.remove(&header.id) else {
            continue;
        };
        if lines.is_empty() {
            continue;
        }
        out.push(DraftSale {
            sale: header,
            lines,
        });
    }
    Ok(out)
}

/// Completes a draft: re-validates it, recomputes the money from the stored lines,
/// and writes the stock movements the draft deliberately withheld.
///
/// Nothing from the draft's own header is believed: it was written when the cart was
/// first scanned, and the draft may have been sitting for days behind a catalog that
/// has since changed — including a shelf another till has emptied.
#[tauri::command]
pub async fn promote_draft(
    sale_id: i32,
    paid_total: Option<Decimal>,
    payment_method: Option<String>,
) -> CmdResult<SaleView> {
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "sale-create").await?;
    promote_draft_in(db(), sale_id, paid_total, payment_method).await
}

pub(crate) async fn promote_draft_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    sale_id: i32,
    paid_total: Option<Decimal>,
    payment_method: Option<String>,
) -> CmdResult<SaleView> {
    let now = crate::migration::now();
    let txn = conn.begin().await?;

    let Some(header) = sale::Entity::find_by_id(sale_id).one(&txn).await? else {
        return Err(CmdError::NotFound("sale".into()));
    };
    // The friendly half of the double-spend guard: it turns the common case — a second
    // click on a button that already worked — into a clear message. The decisive half
    // is the conditional UPDATE at the bottom of this function, which is what makes
    // two *concurrent* promotions safe.
    if header.status != SALE_STATUS_DRAFT {
        return Err(CmdError::Conflict(format!(
            "sale {sale_id} is {}, not a draft",
            header.status
        )));
    }

    let stored = sale_detail::Entity::find()
        .filter(sale_detail::Column::SaleId.eq(sale_id))
        .order_by_asc(sale_detail::Column::Id)
        .all(&txn)
        .await?;
    if stored.is_empty() {
        return Err(CmdError::Validation(format!(
            "draft sale {sale_id} has no lines"
        )));
    }

    // Blank means "keep what the draft already had", matching checkout's tolerance of
    // the till sending the field whether or not the cashier touched it.
    let method = match payment_method.as_deref() {
        Some(raw) if !raw.trim().is_empty() => required(raw, "payment method")?,
        _ if !header.payment_method.trim().is_empty() => header.payment_method.clone(),
        _ => SALE_PAYMENT_DEFAULT.to_owned(),
    };

    // The lines are the record; the header's totals are a cache of them, so every
    // figure is rebuilt from the lines rather than read back.
    let mut subtotal = Decimal::ZERO;
    let mut line_discount_total = Decimal::ZERO;
    for l in &stored {
        subtotal += l.unit_price * l.quantity;
        line_discount_total += l.discount;
    }
    // An order-level discount is a cashier input with nowhere else to live, so it
    // carries over — recovered as the *excess* over the line discounts, which makes
    // the line part of `discount_total` the rebuilt figure rather than the stored one.
    // Dropping it instead would silently charge the customer full price, so the
    // `max` only guards a header inconsistent with its own lines.
    let order_discount = (header.discount_total - line_discount_total).max(Decimal::ZERO);
    let discount_total = line_discount_total + order_discount;
    let tax_total = header.tax_total;

    // The same validator a fresh checkout passes, over the same input shape, so the
    // rules live in exactly one place: quantities positive, prices and discounts
    // non-negative, no discount larger than its line, no negative money anywhere.
    validate_checkout(&CheckoutInput {
        lines: stored
            .iter()
            .map(|l| CheckoutLine {
                item_id: l.item_id,
                quantity: l.quantity,
                unit_price: l.unit_price,
                discount: Some(l.discount),
            })
            .collect(),
        discount_total: Some(discount_total),
        tax_total: Some(tax_total),
        paid_total,
        payment_method: Some(method.clone()),
        note: header.note.clone(),
        promote: None,
        customer_id: None,
        order_type: None,
        payments: None,
    })?;
    guard_discount_within_subtotal(discount_total, subtotal)?;

    // Promotions are evaluated at promote time, not park time: the rules live in the
    // catalog and may have changed while the draft waited — same reason the shelf
    // and the line totals are re-read below rather than trusted.
    let promo_lines: Vec<PromoLine> = stored
        .iter()
        .map(|l| PromoLine {
            item_id: l.item_id,
            quantity: l.quantity,
            unit_price: l.unit_price,
            manual: l.discount,
        })
        .collect();
    let promo = apply_promotions(&txn, &promo_lines, now).await?;
    // The stored header figures are manual-only; the promo line discounts land on
    // the rows below, so they join the header total here — otherwise the receipt
    // shows discounted lines under an undiscounted grand total.
    let promo_lines_sum: Decimal = promo.line_promos.iter().sum();
    let discount_total = discount_total + promo_lines_sum + promo.order_promo;

    let grand_total = subtotal - discount_total + tax_total;
    let paid = paid_total.unwrap_or(grand_total);

    let mut lines: Vec<sale_detail::Model> = Vec::with_capacity(stored.len());
    let mut stock_on_hand: Vec<ItemOnHand> = Vec::with_capacity(stored.len());
    let mut seen: HashMap<i32, usize> = HashMap::with_capacity(stored.len());

    for (index, stored_line) in stored.iter().enumerate() {
        let line = CheckoutLine {
            item_id: stored_line.item_id,
            quantity: stored_line.quantity,
            unit_price: stored_line.unit_price,
            discount: Some(stored_line.discount),
        };
        let is_bundle = !combo_components(&txn, line.item_id).await?.is_empty();
        // The check that matters: the shelf is re-read now, not trusted from scan time,
        // because another till may have sold this stock while the draft waited. Also
        // rejects the sale if the item was soft-deleted in the meantime. Bundles skip
        // the shelf check here; their components are checked below.
        let available = if is_bundle {
            item::Entity::find_by_id(line.item_id)
                .filter(item::Column::DelStatus.eq(LIVE))
                .one(&txn)
                .await?
                .ok_or_else(|| CmdError::NotFound(format!("item {}", line.item_id)))?;
            Decimal::ZERO
        } else {
            let (_, available) = guard_line_sellable(&txn, &line).await?;
            available
        };
        let discount = stored_line.discount + promo.line_promos[index];
        let net = stored_line.unit_price * stored_line.quantity - discount;

        let line_row = if discount == stored_line.discount && net == stored_line.line_total {
            stored_line.clone()
        } else {
            // Persist the rebuilt figure, so the stored row matches the money actually
            // charged rather than the provisional one written at draft time.
            let mut am: sale_detail::ActiveModel = stored_line.clone().into();
            am.discount = Set(discount);
            am.line_total = Set(net);
            am.update(&txn).await?
        };
        lines.push(line_row);

        if is_bundle {
            let needs =
                guard_combo_sellable(&txn, line.item_id, &stored_line.item_name, line.quantity).await?;
            record_combo_sale(
                &txn, sale_id, stored_line.id, line.item_id, &needs, &header.invoice_no, now,
                &mut stock_on_hand, &mut seen,
            )
            .await?;
            continue;
        }

        let balance_after = available - line.quantity;
        record_sale_movement(
            &txn,
            line.item_id,
            sale_id,
            &header.invoice_no,
            line.quantity,
            balance_after,
            now,
        )
        .await?;
        record_on_hand(&mut stock_on_hand, &mut seen, line.item_id, balance_after);
    }

    // The decisive double-spend guard, and the point of no return. `status = 'Draft'`
    // in the WHERE clause makes this a compare-and-swap: a second promoter racing this
    // one matches no row, sees `rows_affected == 0`, and its whole transaction — the
    // movements above included — rolls back. There is no window in which two
    // promotions both commit, which is what makes this safe on a second terminal as
    // well as on a double click.
    let flipped = sale::Entity::update_many()
        .set(sale::ActiveModel {
            status: Set(SALE_STATUS_COMPLETED.to_owned()),
            subtotal: Set(subtotal),
            discount_total: Set(discount_total),
            tax_total: Set(tax_total),
            grand_total: Set(grand_total),
            paid_total: Set(paid),
            payment_method: Set(method),
            updated_at: Set(now),
            ..Default::default()
        })
        .filter(sale::Column::Id.eq(sale_id))
        .filter(sale::Column::Status.eq(SALE_STATUS_DRAFT))
        .exec(&txn)
        .await?;

    if flipped.rows_affected == 0 {
        return Err(CmdError::Conflict(format!(
            "sale {sale_id} is no longer a draft"
        )));
    }

    // Read the header back instead of assembling it here, so the view is what the
    // database holds rather than what this function believes it wrote.
    let row = sale::Entity::find_by_id(sale_id)
        .one(&txn)
        .await?
        .ok_or_else(|| CmdError::NotFound("sale".into()))?;

    let view = SaleView {
        sale: row,
        lines,
        stock_on_hand,
        payments: list_payments_in(&txn, sale_id).await?,
    };
    // Committed last: the movements and the status flip become visible together, or
    // neither does.
    txn.commit().await?;

    Ok(view)
}

/// Throws a draft away.
///
/// A hard delete, not a soft one: a draft was never completed, so it carries no
/// financial history worth preserving, and `sales` has no `del_status` by design.
/// `sale_details` cascades from the foreign key; a draft wrote no stock movements, so
/// there is no ledger row to reconcile and nothing to give back.
#[tauri::command]
pub async fn discard_draft(sale_id: i32) -> CmdResult<()> {
    // Guarded like the reference's `middleware('permission:…')`: check the
    // session before anything else, so an unauthorised caller cannot use
    // validation messages to probe the command.
    crate::commands_auth::require_permission(db(), "sale-destroy").await?;
    discard_draft_in(db(), sale_id).await
}

pub(crate) async fn discard_draft_in<C: ConnectionTrait>(conn: &C, sale_id: i32) -> CmdResult<()> {
    let Some(header) = sale::Entity::find_by_id(sale_id).one(conn).await? else {
        return Err(CmdError::NotFound("sale".into()));
    };
    if header.status != SALE_STATUS_DRAFT {
        return Err(CmdError::Conflict(format!(
            "sale {sale_id} is {} and cannot be discarded",
            header.status
        )));
    }

    // Conditional delete: `status = 'Draft'` in the WHERE makes this the point where a
    // promotion that got there first wins, instead of this call erasing a completed
    // sale's header out from under it.
    let deleted = sale::Entity::delete_many()
        .filter(sale::Column::Id.eq(sale_id))
        .filter(sale::Column::Status.eq(SALE_STATUS_DRAFT))
        .exec(conn)
        .await?;

    if deleted.rows_affected == 0 {
        return Err(CmdError::Conflict(format!(
            "sale {sale_id} is no longer a draft"
        )));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Registers
// ---------------------------------------------------------------------------

const REGISTER_OPEN: &str = "Open";
const REGISTER_CLOSED: &str = "Closed";

/// One tender bucket. Shared by the opening float and the close summary so both
/// sides spell a per-method figure the same way.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MethodTotal {
    pub method: String,
    pub amount: Decimal,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRegisterInput {
    pub opening_balance: Decimal,
    pub opening_details: Option<Vec<MethodTotal>>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseRegisterInput {
    /// What the cashier counted in the drawer.
    pub closing_balance: Decimal,
    pub note: Option<String>,
}

/// A shift's numbers, derived at read time for an open register and snapshotted at
/// close. Only cash touches the drawer, so card and QRIS tenders reconcile elsewhere:
/// `cash_total` is what the drawer should hold, `other_total` is everything paid by
/// other means.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterSummary {
    pub sales_count: u64,
    pub sales_total: Decimal,
    pub collected_total: Decimal,
    pub cash_total: Decimal,
    pub other_total: Decimal,
    pub refunded_total: Decimal,
    pub receipts_total: Decimal,
    pub expected_balance: Decimal,
    pub methods: Vec<MethodTotal>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterView {
    pub id: i32,
    pub status: String,
    pub opened_at: NaiveDateTime,
    pub closed_at: Option<NaiveDateTime>,
    pub opening_balance: Decimal,
    pub opening_details: Option<Vec<MethodTotal>>,
    pub closing_balance: Option<Decimal>,
    pub expected_balance: Option<Decimal>,
    /// Counted minus expected. Negative means the drawer is short.
    pub variance: Option<Decimal>,
    pub note: Option<String>,
}

impl RegisterView {
    fn from_row(row: register::Model) -> CmdResult<Self> {
        let opening_details = row
            .opening_details
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| CmdError::Validation(format!("stored opening details are not valid JSON: {e}")))?;
        let variance = match (row.closing_balance, row.expected_balance) {
            (Some(counted), Some(expected)) => Some((counted - expected).round_dp(MONEY_SCALE)),
            _ => None,
        };
        Ok(Self {
            id: row.id,
            status: row.status,
            opened_at: row.opened_at,
            closed_at: row.closed_at,
            opening_balance: row.opening_balance,
            opening_details,
            closing_balance: row.closing_balance,
            expected_balance: row.expected_balance,
            variance,
            note: row.note,
        })
    }
}

#[tauri::command]
pub async fn open_register(input: OpenRegisterInput) -> CmdResult<RegisterView> {
    crate::commands_auth::require_permission(db(), "register-open").await?;
    let Some(user_id) = crate::auth::current_user_id() else {
        return Err(CmdError::Forbidden("you are not signed in".into()));
    };
    open_register_in(db(), user_id, input).await
}

pub async fn open_register_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    user_id: i32,
    input: OpenRegisterInput,
) -> CmdResult<RegisterView> {
    if input.opening_balance < Decimal::ZERO {
        return Err(CmdError::Validation("opening float cannot be negative".into()));
    }
    if let Some(details) = input.opening_details.as_ref() {
        let mut total = Decimal::ZERO;
        for d in details {
            if d.amount < Decimal::ZERO {
                return Err(CmdError::Validation(format!("opening amount for {} cannot be negative", d.method)));
            }
            total += d.amount;
        }
        // The breakdown is the cashier's own count, so a mismatch with the float is
        // a typo rather than a second figure to reconcile — refuse it now.
        if total.round_dp(MONEY_SCALE) != input.opening_balance.round_dp(MONEY_SCALE) {
            return Err(CmdError::Validation(format!(
                "opening details total {total}, not the {float} float given",
                float = input.opening_balance
            )));
        }
    }

    let txn = conn.begin().await?;
    if open_register_row(&txn, user_id).await?.is_some() {
        return Err(CmdError::Conflict("a register is already open for this user".into()));
    }

    let now = crate::migration::now();
    let row = register::ActiveModel {
        user_id: Set(user_id),
        status: Set(REGISTER_OPEN.to_owned()),
        opened_at: Set(now),
        closed_at: Set(None),
        opening_balance: Set(input.opening_balance),
        opening_details: Set(input
            .opening_details
            .map(|d| serde_json::to_string(&d))
            .transpose()
            .map_err(|e| CmdError::Validation(format!("opening details are not valid JSON: {e}")))?),
        closing_balance: Set(None),
        expected_balance: Set(None),
        note: Set(input.note),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    txn.commit().await?;

    RegisterView::from_row(row)
}

async fn open_register_row<C: ConnectionTrait>(
    conn: &C,
    user_id: i32,
) -> CmdResult<Option<register::Model>> {
    Ok(register::Entity::find()
        .filter(register::Column::UserId.eq(user_id))
        .filter(register::Column::Status.eq(REGISTER_OPEN))
        .order_by_desc(register::Column::Id)
        .one(conn)
        .await?)
}

#[tauri::command]
pub async fn current_register() -> CmdResult<Option<RegisterView>> {
    crate::commands_auth::require_permission(db(), "register-summary").await?;
    let Some(user_id) = crate::auth::current_user_id() else {
        return Err(CmdError::Forbidden("you are not signed in".into()));
    };
    current_register_in(db(), user_id).await
}

pub async fn current_register_in<C: ConnectionTrait>(
    conn: &C,
    user_id: i32,
) -> CmdResult<Option<RegisterView>> {
    Ok(match open_register_row(conn, user_id).await? {
        Some(row) => Some(RegisterView::from_row(row)?),
        None => None,
    })
}

#[tauri::command]
pub async fn list_registers() -> CmdResult<Vec<RegisterView>> {
    crate::commands_auth::require_permission(db(), "register-list").await?;
    let Some(user_id) = crate::auth::current_user_id() else {
        return Err(CmdError::Forbidden("you are not signed in".into()));
    };
    let rows = register::Entity::find()
        .filter(register::Column::UserId.eq(user_id))
        .order_by_desc(register::Column::Id)
        .all(db())
        .await?;
    rows.into_iter().map(RegisterView::from_row).collect()
}

#[tauri::command]
pub async fn register_summary() -> CmdResult<Option<RegisterSummary>> {
    crate::commands_auth::require_permission(db(), "register-summary").await?;
    let Some(user_id) = crate::auth::current_user_id() else {
        return Err(CmdError::Forbidden("you are not signed in".into()));
    };
    register_summary_in(db(), user_id).await
}

pub async fn register_summary_in<C: ConnectionTrait>(
    conn: &C,
    user_id: i32,
) -> CmdResult<Option<RegisterSummary>> {
    Ok(match open_register_row(conn, user_id).await? {
        Some(row) => Some(summarise_register(conn, &row, crate::migration::now()).await?),
        None => None,
    })
}

async fn summarise_register<C: ConnectionTrait>(
    conn: &C,
    row: &register::Model,
    to: NaiveDateTime,
) -> CmdResult<RegisterSummary> {
    let sales: Vec<(Decimal, Decimal)> = sale::Entity::find()
        .select_only()
        .column(sale::Column::GrandTotal)
        .column(sale::Column::PaidTotal)
        .filter(sale::Column::Status.eq(SALE_STATUS_COMPLETED))
        .filter(sale::Column::CreatedAt.gte(row.opened_at))
        .filter(sale::Column::CreatedAt.lt(to))
        .into_tuple()
        .all(conn)
        .await?;
    let sales_count = sales.len() as u64;
    let (sales_total, collected_total) = sales
        .into_iter()
        .fold((Decimal::ZERO, Decimal::ZERO), |(g, p), (grand, paid)| {
            (g + grand, p + paid)
        });

    let sale_ids: Vec<i32> = sale::Entity::find()
        .select_only()
        .column(sale::Column::Id)
        .filter(sale::Column::Status.eq(SALE_STATUS_COMPLETED))
        .filter(sale::Column::CreatedAt.gte(row.opened_at))
        .filter(sale::Column::CreatedAt.lt(to))
        .into_tuple()
        .all(conn)
        .await?;
    let mut by_method: HashMap<String, Decimal> = HashMap::new();
    let mut tendered_sale_ids: std::collections::HashSet<i32> = std::collections::HashSet::new();
    if !sale_ids.is_empty() {
        let tenders: Vec<(i32, String, Decimal)> = sale_payment::Entity::find()
            .select_only()
            .column(sale_payment::Column::SaleId)
            .column(sale_payment::Column::Method)
            .column(sale_payment::Column::Amount)
            .filter(sale_payment::Column::SaleId.is_in(sale_ids))
            .into_tuple()
            .all(conn)
            .await?;
        for (sale_id, method, amount) in tenders {
            tendered_sale_ids.insert(sale_id);
            *by_method.entry(method).or_insert(Decimal::ZERO) += amount;
        }
    }
    // A single-tender sale writes no tender rows — the header carries the method and
    // the figure instead — so those sales fall back to the header. Only sales with
    // no rows qualify, so a tender is never counted twice.
    let mut untendered_q = sale::Entity::find()
        .select_only()
        .column(sale::Column::PaidTotal)
        .column(sale::Column::PaymentMethod)
        .filter(sale::Column::Status.eq(SALE_STATUS_COMPLETED))
        .filter(sale::Column::CreatedAt.gte(row.opened_at))
        .filter(sale::Column::CreatedAt.lt(to));
    // `NOT IN ()` is not valid SQL, so the filter only applies when there is
    // something to exclude.
    if !tendered_sale_ids.is_empty() {
        untendered_q = untendered_q
            .filter(sale::Column::Id.is_not_in(tendered_sale_ids.into_iter().collect::<Vec<_>>()));
    }
    let untendered: Vec<(Decimal, String)> = untendered_q.into_tuple().all(conn).await?;
    for (paid, method) in untendered {
        *by_method.entry(method).or_insert(Decimal::ZERO) += paid;
    }
    let mut methods: Vec<MethodTotal> = by_method
        .into_iter()
        .map(|(method, amount)| MethodTotal { method, amount: amount.round_dp(MONEY_SCALE) })
        .collect();
    methods.sort_by(|a, b| a.method.cmp(&b.method));
    let cash_total = methods
        .iter()
        .filter(|m| m.method == "Cash")
        .map(|m| m.amount)
        .sum::<Decimal>();
    let other_total = (collected_total - cash_total).round_dp(MONEY_SCALE);

    let refunded_total: Option<Decimal> = sale_return::Entity::find()
        .select_only()
        .column_as(sale_return::Column::RefundedTotal.sum(), "total")
        .filter(sale_return::Column::CreatedAt.gte(row.opened_at))
        .filter(sale_return::Column::CreatedAt.lt(to))
        .into_tuple()
        .one(conn)
        .await?
        .flatten();
    let refunded_total = refunded_total.unwrap_or(Decimal::ZERO);

    let receipts_total: Option<Decimal> = customer_receive::Entity::find()
        .select_only()
        .column_as(customer_receive::Column::Amount.sum(), "total")
        .filter(customer_receive::Column::CreatedAt.gte(row.opened_at))
        .filter(customer_receive::Column::CreatedAt.lt(to))
        .into_tuple()
        .one(conn)
        .await?
        .flatten();
    let receipts_total = receipts_total.unwrap_or(Decimal::ZERO);

    // Only cash touches the drawer: card and QRIS settle to the bank, and a refund
    // hands cash back out of it. Receipts are debt collected at the till in cash.
    let expected_balance =
        (row.opening_balance + cash_total + receipts_total - refunded_total).round_dp(MONEY_SCALE);

    Ok(RegisterSummary {
        sales_count,
        sales_total: sales_total.round_dp(MONEY_SCALE),
        collected_total: collected_total.round_dp(MONEY_SCALE),
        cash_total: cash_total.round_dp(MONEY_SCALE),
        other_total,
        refunded_total: refunded_total.round_dp(MONEY_SCALE),
        receipts_total: receipts_total.round_dp(MONEY_SCALE),
        expected_balance,
        methods,
    })
}

#[tauri::command]
pub async fn close_register(input: CloseRegisterInput) -> CmdResult<RegisterView> {
    crate::commands_auth::require_permission(db(), "register-close").await?;
    let Some(user_id) = crate::auth::current_user_id() else {
        return Err(CmdError::Forbidden("you are not signed in".into()));
    };
    close_register_in(db(), user_id, input).await
}

pub async fn close_register_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    user_id: i32,
    input: CloseRegisterInput,
) -> CmdResult<RegisterView> {
    if input.closing_balance < Decimal::ZERO {
        return Err(CmdError::Validation("counted cash cannot be negative".into()));
    }

    let txn = conn.begin().await?;
    let Some(row) = open_register_row(&txn, user_id).await? else {
        return Err(CmdError::Conflict("no open register for this user".into()));
    };

    let now = crate::migration::now();
    let summary = summarise_register(&txn, &row, now).await?;

    let mut am: register::ActiveModel = row.into();
    am.status = Set(REGISTER_CLOSED.to_owned());
    am.closed_at = Set(Some(now));
    am.closing_balance = Set(Some(input.closing_balance));
    am.expected_balance = Set(Some(summary.expected_balance));
    if let Some(note) = input.note {
        am.note = Set(Some(note));
    }
    let row = am.update(&txn).await?;
    txn.commit().await?;

    RegisterView::from_row(row)
}

// ---------------------------------------------------------------------------
// Quotations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotationLine {
    pub item_id: i32,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    pub discount: Option<Decimal>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotationInput {
    pub customer_id: i32,
    /// `YYYY-MM-DD`. Defaults to today: an offer is priced as of when it is written.
    pub quoted_at: Option<String>,
    pub reference_no: Option<String>,
    pub discount_total: Option<Decimal>,
    pub note: Option<String>,
    pub lines: Vec<QuotationLine>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotationView {
    pub id: i32,
    pub quotation_no: String,
    pub customer_id: i32,
    pub customer_name: Option<String>,
    pub quoted_at: NaiveDateTime,
    pub reference_no: Option<String>,
    pub subtotal: Decimal,
    pub discount_total: Decimal,
    pub grand_total: Decimal,
    pub note: Option<String>,
    pub created_at: NaiveDateTime,
    pub lines: Vec<quotation_detail::Model>,
}

fn quotation_no_for(id: i32) -> String {
    format!("QT-{id:06}")
}

/// Unique within the process only — the column's own uniqueness comes from the
/// real number, derived from the primary key like every other document number.
fn provisional_quotation_no() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!(
        "PENDING-{}-{}",
        crate::migration::now().and_utc().timestamp_micros(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

fn parse_quote_day(raw: Option<&str>) -> CmdResult<NaiveDateTime> {
    match raw.map(str::trim).filter(|t| !t.is_empty()) {
        // A wrong date on an issued offer matters, so this refuses where the sales
        // list filter merely ignores: one is a write, the other a read.
        None => Ok(crate::migration::now()),
        Some(text) => chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .ok_or_else(|| CmdError::Validation(format!("{text} is not a YYYY-MM-DD date"))),
    }
}

fn validate_quotation_lines(lines: &[QuotationLine]) -> CmdResult<()> {
    if lines.is_empty() {
        return Err(CmdError::Validation("a quotation needs at least one line".into()));
    }
    for (i, line) in lines.iter().enumerate() {
        let where_ = format!("line {}", i + 1);
        if line.quantity <= Decimal::ZERO {
            return Err(CmdError::Validation(format!("{where_}: quantity must be greater than zero")));
        }
        if line.unit_price < Decimal::ZERO {
            return Err(CmdError::Validation(format!("{where_}: unit price cannot be negative")));
        }
        let discount = line.discount.unwrap_or(Decimal::ZERO);
        if discount < Decimal::ZERO {
            return Err(CmdError::Validation(format!("{where_}: discount cannot be negative")));
        }
        if discount > line.unit_price * line.quantity {
            return Err(CmdError::Validation(format!("{where_}: discount is larger than the line total")));
        }
    }
    Ok(())
}

fn quotation_totals(lines: &[QuotationLine], discount_total: Decimal) -> CmdResult<(Decimal, Decimal, Decimal)> {
    let mut subtotal = Decimal::ZERO;
    let mut line_discount = Decimal::ZERO;
    for line in lines {
        subtotal += line.unit_price * line.quantity;
        line_discount += line.discount.unwrap_or(Decimal::ZERO);
    }
    if discount_total < Decimal::ZERO {
        return Err(CmdError::Validation("discount total cannot be negative".into()));
    }
    let discount_all = line_discount + discount_total;
    if discount_all > subtotal {
        return Err(CmdError::Validation(format!(
            "discount {discount_all} is larger than the subtotal {subtotal}"
        )));
    }
    Ok((
        subtotal.round_dp(MONEY_SCALE),
        discount_all.round_dp(MONEY_SCALE),
        (subtotal - discount_all).round_dp(MONEY_SCALE),
    ))
}

async fn write_quotation_lines<C: ConnectionTrait>(
    conn: &C,
    quotation_id: i32,
    lines: &[QuotationLine],
) -> CmdResult<Vec<quotation_detail::Model>> {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let item = item::Entity::find_by_id(line.item_id)
            .filter(item::Column::DelStatus.eq(LIVE))
            .one(conn)
            .await?
            .ok_or_else(|| CmdError::NotFound(format!("item {}", line.item_id)))?;
        let discount = line.discount.unwrap_or(Decimal::ZERO);
        out.push(
            quotation_detail::ActiveModel {
                quotation_id: Set(quotation_id),
                item_id: Set(line.item_id),
                item_name: Set(item.name),
                quantity: Set(line.quantity),
                unit_price: Set(line.unit_price),
                discount: Set(discount),
                line_total: Set((line.unit_price * line.quantity - discount).round_dp(MONEY_SCALE)),
                ..Default::default()
            }
            .insert(conn)
            .await?,
        );
    }
    Ok(out)
}

async fn resolve_quote_customer<C: ConnectionTrait>(conn: &C, customer_id: i32) -> CmdResult<()> {
    customer::Entity::find_by_id(customer_id)
        .filter(customer::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("customer".into()))
        .map(|_| ())
}

fn quotation_view(
    row: quotation::Model,
    customer_name: Option<String>,
    lines: Vec<quotation_detail::Model>,
) -> QuotationView {
    QuotationView {
        id: row.id,
        quotation_no: row.quotation_no,
        customer_id: row.customer_id,
        customer_name,
        quoted_at: row.quoted_at,
        reference_no: row.reference_no,
        subtotal: row.subtotal,
        discount_total: row.discount_total,
        grand_total: row.grand_total,
        note: row.note,
        created_at: row.created_at,
        lines,
    }
}

#[tauri::command]
pub async fn create_quotation(input: QuotationInput) -> CmdResult<QuotationView> {
    crate::commands_auth::require_permission(db(), "quotation-create").await?;
    create_quotation_in(db(), input).await
}

pub async fn create_quotation_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    input: QuotationInput,
) -> CmdResult<QuotationView> {
    validate_quotation_lines(&input.lines)?;
    let quoted_at = parse_quote_day(input.quoted_at.as_deref())?;
    let order_discount = input.discount_total.unwrap_or(Decimal::ZERO);

    let txn = conn.begin().await?;
    resolve_quote_customer(&txn, input.customer_id).await?;
    let (subtotal, discount_total, grand_total) = quotation_totals(&input.lines, order_discount)?;

    let now = crate::migration::now();
    let header = quotation::ActiveModel {
        customer_id: Set(input.customer_id),
        quotation_no: Set(provisional_quotation_no()),
        quoted_at: Set(quoted_at),
        reference_no: Set(text(input.reference_no.clone())),
        subtotal: Set(subtotal),
        discount_total: Set(discount_total),
        grand_total: Set(grand_total),
        created_by: Set(crate::auth::current_user_id()),
        note: Set(text(input.note.clone())),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    let quotation_id = header.id;
    let lines = write_quotation_lines(&txn, quotation_id, &input.lines).await?;

    let mut am: quotation::ActiveModel = header.into();
    am.quotation_no = Set(quotation_no_for(quotation_id));
    let row = am.update(&txn).await?;
    txn.commit().await?;

    let name = customer::Entity::find_by_id(row.customer_id)
        .one(conn)
        .await?
        .map(|c| c.name);
    Ok(quotation_view(row, name, lines))
}

#[tauri::command]
pub async fn list_quotations(query: PageQuery) -> CmdResult<Page<QuotationView>> {
    crate::commands_auth::require_permission(db(), "quotation-list").await?;
    list_quotations_in(db(), &query).await
}

pub async fn list_quotations_in<C: ConnectionTrait>(
    conn: &C,
    query: &PageQuery,
) -> CmdResult<Page<QuotationView>> {
    let total = quotation::Entity::find().count(conn).await?;
    let rows = quotation::Entity::find()
        .order_by_desc(quotation::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;

    let ids: Vec<i32> = rows.iter().map(|r| r.customer_id).collect();
    let names: HashMap<i32, String> = if ids.is_empty() {
        HashMap::new()
    } else {
        customer::Entity::find()
            .filter(customer::Column::Id.is_in(ids))
            .all(conn)
            .await?
            .into_iter()
            .map(|c| (c.id, c.name))
            .collect()
    };
    let views = rows
        .into_iter()
        .map(|row| {
            let name = names.get(&row.customer_id).cloned();
            quotation_view(row, name, Vec::new())
        })
        .collect();
    Ok(Page::new(views, total, query))
}

#[tauri::command]
pub async fn get_quotation(id: i32) -> CmdResult<QuotationView> {
    crate::commands_auth::require_permission(db(), "quotation-show").await?;
    let db = db();
    let row = quotation::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| CmdError::NotFound("quotation".into()))?;
    let lines = quotation_detail::Entity::find()
        .filter(quotation_detail::Column::QuotationId.eq(id))
        .order_by_asc(quotation_detail::Column::Id)
        .all(db)
        .await?;
    let name = customer::Entity::find_by_id(row.customer_id)
        .one(db)
        .await?
        .map(|c| c.name);
    Ok(quotation_view(row, name, lines))
}

#[tauri::command]
pub async fn update_quotation(id: i32, input: QuotationInput) -> CmdResult<QuotationView> {
    crate::commands_auth::require_permission(db(), "quotation-edit").await?;
    update_quotation_in(db(), id, input).await
}

pub async fn update_quotation_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    id: i32,
    input: QuotationInput,
) -> CmdResult<QuotationView> {
    validate_quotation_lines(&input.lines)?;
    let quoted_at = parse_quote_day(input.quoted_at.as_deref())?;
    let order_discount = input.discount_total.unwrap_or(Decimal::ZERO);

    let txn = conn.begin().await?;
    let found = quotation::Entity::find_by_id(id)
        .one(&txn)
        .await?
        .ok_or_else(|| CmdError::NotFound("quotation".into()))?;
    resolve_quote_customer(&txn, input.customer_id).await?;
    let (subtotal, discount_total, grand_total) = quotation_totals(&input.lines, order_discount)?;

    // Replaced, not appended: the pivot rule applies to any child collection with
    // no uniqueness beyond its parent.
    quotation_detail::Entity::delete_many()
        .filter(quotation_detail::Column::QuotationId.eq(id))
        .exec(&txn)
        .await?;
    let lines = write_quotation_lines(&txn, id, &input.lines).await?;

    let mut am: quotation::ActiveModel = found.into();
    am.customer_id = Set(input.customer_id);
    am.quoted_at = Set(quoted_at);
    am.reference_no = Set(text(input.reference_no.clone()));
    am.subtotal = Set(subtotal);
    am.discount_total = Set(discount_total);
    am.grand_total = Set(grand_total);
    am.note = Set(text(input.note.clone()));
    let row = am.update(&txn).await?;
    txn.commit().await?;

    let name = customer::Entity::find_by_id(row.customer_id)
        .one(conn)
        .await?
        .map(|c| c.name);
    Ok(quotation_view(row, name, lines))
}

#[tauri::command]
pub async fn delete_quotation(id: i32) -> CmdResult<()> {
    crate::commands_auth::require_permission(db(), "quotation-destroy").await?;
    delete_quotation_in(db(), id).await
}

pub async fn delete_quotation_in<C: ConnectionTrait>(conn: &C, id: i32) -> CmdResult<()> {
    // Hard delete: an offer carries no money and moves no stock, so unlike a sale
    // there is no financial history to preserve. Details cascade from the FK.
    let deleted = quotation::Entity::delete_by_id(id).exec(conn).await?;
    if deleted.rows_affected == 0 {
        return Err(CmdError::NotFound("quotation".into()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Bookings
// ---------------------------------------------------------------------------

pub const BOOKING_STATUSES: &[&str] = &["Booked", "Waiting", "Completed", "Cancelled"];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookingInput {
    pub customer_id: i32,
    pub service_seller_id: Option<i32>,
    pub status: Option<String>,
    /// `YYYY-MM-DDTHH:MM`, as a datetime-local field sends it.
    pub start_at: String,
    pub end_at: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookingFilter {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub customer_id: Option<i32>,
    /// `YYYY-MM-DD`, inclusive. Unparseable narrows nothing, like the sales filter.
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookingView {
    pub id: i32,
    pub customer_id: i32,
    pub customer_name: Option<String>,
    pub service_seller_id: Option<i32>,
    pub service_seller_name: Option<String>,
    pub status: String,
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub note: Option<String>,
    pub created_at: NaiveDateTime,
}

fn parse_booking_moment(raw: &str, field: &str) -> CmdResult<NaiveDateTime> {
    chrono::NaiveDateTime::parse_from_str(raw.trim(), "%Y-%m-%dT%H:%M")
        .map_err(|_| CmdError::Validation(format!("{field} is not a valid date and time")))
}

fn booking_view(
    row: booking::Model,
    customer_name: Option<String>,
    seller_name: Option<String>,
) -> BookingView {
    BookingView {
        id: row.id,
        customer_id: row.customer_id,
        customer_name,
        service_seller_id: row.service_seller_id,
        service_seller_name: seller_name,
        status: row.status,
        start_at: row.start_at,
        end_at: row.end_at,
        note: row.note,
        created_at: row.created_at,
    }
}

async fn booking_names<C: ConnectionTrait>(
    conn: &C,
    rows: &[booking::Model],
) -> CmdResult<(HashMap<i32, String>, HashMap<i32, String>)> {
    let customer_ids: Vec<i32> = rows.iter().map(|r| r.customer_id).collect();
    let customers: HashMap<i32, String> = if customer_ids.is_empty() {
        HashMap::new()
    } else {
        customer::Entity::find()
            .filter(customer::Column::Id.is_in(customer_ids))
            .all(conn)
            .await?
            .into_iter()
            .map(|c| (c.id, c.name))
            .collect()
    };
    let seller_ids: Vec<i32> = rows.iter().filter_map(|r| r.service_seller_id).collect();
    let sellers: HashMap<i32, String> = if seller_ids.is_empty() {
        HashMap::new()
    } else {
        users::Entity::find()
            .filter(users::Column::Id.is_in(seller_ids))
            .all(conn)
            .await?
            .into_iter()
            .map(|u| (u.id, u.name))
            .collect()
    };
    Ok((customers, sellers))
}

async fn resolve_booking_parties<C: ConnectionTrait>(
    conn: &C,
    customer_id: i32,
    service_seller_id: Option<i32>,
) -> CmdResult<()> {
    customer::Entity::find_by_id(customer_id)
        .filter(customer::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("customer".into()))
        .map(|_| ())?;
    if let Some(seller_id) = service_seller_id {
        users::Entity::find_by_id(seller_id)
            .filter(users::Column::DelStatus.eq(LIVE))
            .one(conn)
            .await?
            .ok_or_else(|| CmdError::NotFound("service seller".into()))
            .map(|_| ())?;
    }
    Ok(())
}

fn resolve_booking_status(raw: Option<&str>) -> CmdResult<String> {
    match raw.map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok("Booked".to_owned()),
        Some(status) if BOOKING_STATUSES.contains(&status) => Ok(status.to_owned()),
        Some(status) => Err(CmdError::Validation(format!("{status} is not a booking status"))),
    }
}

#[tauri::command]
pub async fn create_booking(input: BookingInput) -> CmdResult<BookingView> {
    crate::commands_auth::require_permission(db(), "booking-create").await?;
    create_booking_in(db(), input, crate::auth::current_user_id()).await
}

pub async fn create_booking_in<C: ConnectionTrait>(
    conn: &C,
    input: BookingInput,
    created_by: Option<i32>,
) -> CmdResult<BookingView> {
    let status = resolve_booking_status(input.status.as_deref())?;
    let start_at = parse_booking_moment(&input.start_at, "start")?;
    let end_at = parse_booking_moment(&input.end_at, "end")?;
    // A booking starts today or later: backdating one invents history the shop
    // cannot verify, and the date comparison is on the day, not the clock.
    let today = crate::migration::now().date();
    if start_at.date() < today {
        return Err(CmdError::Validation("a booking cannot start in the past".into()));
    }
    if end_at < start_at {
        return Err(CmdError::Validation("the booking cannot end before it starts".into()));
    }
    resolve_booking_parties(conn, input.customer_id, input.service_seller_id).await?;

    let now = crate::migration::now();
    let row = booking::ActiveModel {
        customer_id: Set(input.customer_id),
        service_seller_id: Set(input.service_seller_id),
        created_by: Set(created_by),
        status: Set(status),
        start_at: Set(start_at),
        end_at: Set(end_at),
        note: Set(text(input.note.clone())),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(conn)
    .await?;

    let (customers, sellers) = booking_names(conn, std::slice::from_ref(&row)).await?;
    Ok(booking_view(
        row.clone(),
        customers.get(&row.customer_id).cloned(),
        row.service_seller_id.and_then(|id| sellers.get(&id).cloned()),
    ))
}

#[tauri::command]
pub async fn list_bookings(filter: BookingFilter, query: PageQuery) -> CmdResult<Page<BookingView>> {
    crate::commands_auth::require_permission(db(), "booking-list").await?;
    list_bookings_in(db(), &filter, &query).await
}

pub async fn list_bookings_in<C: ConnectionTrait>(
    conn: &C,
    filter: &BookingFilter,
    query: &PageQuery,
) -> CmdResult<Page<BookingView>> {
    let mut q = booking::Entity::find().filter(booking::Column::DelStatus.eq(LIVE));

    if let Some(status) = filter.status.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        q = q.filter(booking::Column::Status.eq(status));
    }
    if let Some(customer_id) = filter.customer_id {
        q = q.filter(booking::Column::CustomerId.eq(customer_id));
    }
    if let Some((start, _)) = filter.from.as_deref().and_then(day_bounds) {
        q = q.filter(booking::Column::StartAt.gte(start));
    }
    if let Some((_, next)) = filter.to.as_deref().and_then(day_bounds) {
        q = q.filter(booking::Column::StartAt.lt(next));
    }

    let total = q.clone().count(conn).await?;
    // Soonest first: a booking list is a schedule, not an archive.
    let rows = q
        .order_by_asc(booking::Column::StartAt)
        .order_by_asc(booking::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;

    let (customers, sellers) = booking_names(conn, &rows).await?;
    let views = rows
        .into_iter()
        .map(|row| {
            let name = customers.get(&row.customer_id).cloned();
            let seller = row.service_seller_id.and_then(|id| sellers.get(&id).cloned());
            booking_view(row, name, seller)
        })
        .collect();
    Ok(Page::new(views, total, query))
}

#[tauri::command]
pub async fn get_booking(id: i32) -> CmdResult<BookingView> {
    crate::commands_auth::require_permission(db(), "booking-show").await?;
    let db = db();
    let row = booking::Entity::find_by_id(id)
        .filter(booking::Column::DelStatus.eq(LIVE))
        .one(db)
        .await?
        .ok_or_else(|| CmdError::NotFound("booking".into()))?;
    let (customers, sellers) = booking_names(db, std::slice::from_ref(&row)).await?;
    Ok(booking_view(
        row.clone(),
        customers.get(&row.customer_id).cloned(),
        row.service_seller_id.and_then(|id| sellers.get(&id).cloned()),
    ))
}

#[tauri::command]
pub async fn update_booking(id: i32, input: BookingInput) -> CmdResult<BookingView> {
    crate::commands_auth::require_permission(db(), "booking-edit").await?;
    update_booking_in(db(), id, input).await
}

pub async fn update_booking_in<C: ConnectionTrait>(
    conn: &C,
    id: i32,
    input: BookingInput,
) -> CmdResult<BookingView> {
    let found = booking::Entity::find_by_id(id)
        .filter(booking::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("booking".into()))?;

    let status = resolve_booking_status(input.status.as_deref())?;
    let start_at = parse_booking_moment(&input.start_at, "start")?;
    let end_at = parse_booking_moment(&input.end_at, "end")?;
    // A past booking can still be edited, but not moved further back: the floor is
    // whichever is earlier, its original start or today.
    let floor = found.start_at.date().min(crate::migration::now().date());
    if start_at.date() < floor {
        return Err(CmdError::Validation("the booking cannot be moved before it was".into()));
    }
    if end_at < start_at {
        return Err(CmdError::Validation("the booking cannot end before it starts".into()));
    }
    resolve_booking_parties(conn, input.customer_id, input.service_seller_id).await?;

    let mut am: booking::ActiveModel = found.into();
    am.customer_id = Set(input.customer_id);
    am.service_seller_id = Set(input.service_seller_id);
    am.status = Set(status);
    am.start_at = Set(start_at);
    am.end_at = Set(end_at);
    am.note = Set(text(input.note.clone()));
    am.updated_at = Set(crate::migration::now());
    let row = am.update(conn).await?;

    let (customers, sellers) = booking_names(conn, std::slice::from_ref(&row)).await?;
    Ok(booking_view(
        row.clone(),
        customers.get(&row.customer_id).cloned(),
        row.service_seller_id.and_then(|id| sellers.get(&id).cloned()),
    ))
}

#[tauri::command]
pub async fn delete_booking(id: i32) -> CmdResult<()> {
    crate::commands_auth::require_permission(db(), "booking-destroy").await?;
    delete_booking_in(db(), id).await
}

pub async fn delete_booking_in<C: ConnectionTrait>(conn: &C, id: i32) -> CmdResult<()> {
    // Soft-delete: an appointment is revocable, not financial history, so unlike a
    // sale there is nothing to preserve — but unlike a draft it was visible to a
    // customer, so unlike a draft it is not erased.
    let found = booking::Entity::find_by_id(id)
        .filter(booking::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("booking".into()))?;
    let mut am: booking::ActiveModel = found.into();
    am.del_status = Set(DELETED.to_owned());
    am.updated_at = Set(crate::migration::now());
    am.update(conn).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Promotions
// ---------------------------------------------------------------------------

pub const PROMOTION_KINDS: &[&str] = &["ItemPercent", "ItemFixed", "OrderPercent", "OrderFixed", "BuyGet"];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionInput {
    pub title: String,
    pub kind: String,
    pub target_item_id: Option<i32>,
    pub reward_item_id: Option<i32>,
    pub percent: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub min_total: Option<Decimal>,
    pub buy_qty: Option<Decimal>,
    pub get_qty: Option<Decimal>,
    /// `YYYY-MM-DD`, inclusive on both ends.
    pub start_at: String,
    pub end_at: String,
}

fn parse_promo_day(raw: &str) -> CmdResult<NaiveDateTime> {
    chrono::NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .ok_or_else(|| CmdError::Validation(format!("{raw} is not a YYYY-MM-DD date")))
}

/// A positive decimal or nothing. Zero is not a discount, a threshold, or a
/// quantity — accepting it would make a rule that never fires and reads as live.
fn positive(raw: Option<Decimal>, field: &str) -> CmdResult<Option<Decimal>> {
    match raw {
        None => Ok(None),
        Some(v) if v > Decimal::ZERO => Ok(Some(v)),
        _ => Err(CmdError::Validation(format!("{field} must be greater than zero"))),
    }
}

fn non_negative(raw: Option<Decimal>, field: &str) -> CmdResult<Decimal> {
    match raw {
        None => Ok(Decimal::ZERO),
        Some(v) if v >= Decimal::ZERO => Ok(v),
        _ => Err(CmdError::Validation(format!("{field} cannot be negative"))),
    }
}

async fn resolve_promo_item<C: ConnectionTrait>(conn: &C, item_id: i32, field: &str) -> CmdResult<()> {
    item::Entity::find_by_id(item_id)
        .filter(item::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound(format!("{field} item {item_id}")))
        .map(|_| ())
}

/// The validated shape of one promotion row. Kind and columns must agree — a second
/// table per kind would only move that check, not remove it.
struct ResolvedPromotion {
    kind: &'static str,
    target_item_id: Option<i32>,
    reward_item_id: Option<i32>,
    percent: Option<Decimal>,
    amount: Option<Decimal>,
    min_total: Decimal,
    buy_qty: Option<Decimal>,
    get_qty: Option<Decimal>,
    start_at: NaiveDateTime,
    end_at: NaiveDateTime,
}

async fn resolve_promotion<C: ConnectionTrait>(conn: &C, input: &PromotionInput) -> CmdResult<ResolvedPromotion> {
    let title = required(&input.title, "title")?;
    let _ = title;
    let kind = match input.kind.trim() {
        "ItemPercent" => "ItemPercent",
        "ItemFixed" => "ItemFixed",
        "OrderPercent" => "OrderPercent",
        "OrderFixed" => "OrderFixed",
        "BuyGet" => "BuyGet",
        other => return Err(CmdError::Validation(format!("{other} is not a promotion kind"))),
    };
    // Day bounds are half-open: the end date is fully included, the day after is not.
    let start_at = parse_promo_day(&input.start_at)?;
    let end_day = parse_promo_day(&input.end_at)?;
    let end_at = end_day
        .checked_add_days(chrono::Days::new(1))
        .ok_or_else(|| CmdError::Validation("end date is out of range".to_owned()))?;
    if end_at <= start_at {
        return Err(CmdError::Validation("the promotion cannot end before it starts".into()));
    }

    // Columns a kind does not use must stay empty, so a row always means what its
    // kind says and a report never has to guess which column won.
    let only = |want: &[&str], name: &str, present: bool| -> CmdResult<()> {
        if present && !want.contains(&name) {
            return Err(CmdError::Validation(format!("{name} does not apply to a {kind} promotion")));
        }
        Ok(())
    };
    let has_target = input.target_item_id.is_some();
    let has_reward = input.reward_item_id.is_some();
    let has_percent = input.percent.is_some();
    let has_amount = input.amount.is_some();
    let has_min = input.min_total.is_some();
    let has_buy = input.buy_qty.is_some();
    let has_get = input.get_qty.is_some();

    let resolved = match kind {
        "ItemPercent" => {
            only(&["target", "percent"], "reward_item", has_reward)?;
            only(&["target", "percent"], "amount", has_amount)?;
            only(&["target", "percent"], "min_total", has_min)?;
            only(&["target", "percent"], "buy_qty", has_buy)?;
            only(&["target", "percent"], "get_qty", has_get)?;
            let target = input.target_item_id.ok_or_else(|| CmdError::Validation("an item promotion needs an item".into()))?;
            let percent = positive(input.percent, "percent")?
                .ok_or_else(|| CmdError::Validation("an item promotion needs a percent".into()))?;
            if percent > Decimal::new(100, 0) {
                return Err(CmdError::Validation("percent cannot exceed 100".into()));
            }
            resolve_promo_item(conn, target, "target").await?;
            ResolvedPromotion { kind, target_item_id: Some(target), reward_item_id: None,
                percent: Some(percent), amount: None, min_total: Decimal::ZERO,
                buy_qty: None, get_qty: None, start_at, end_at }
        }
        "ItemFixed" => {
            only(&["target", "amount"], "reward_item", has_reward)?;
            only(&["target", "amount"], "percent", has_percent)?;
            only(&["target", "amount"], "min_total", has_min)?;
            only(&["target", "amount"], "buy_qty", has_buy)?;
            only(&["target", "amount"], "get_qty", has_get)?;
            let target = input.target_item_id.ok_or_else(|| CmdError::Validation("an item promotion needs an item".into()))?;
            let amount = positive(input.amount, "amount")?
                .ok_or_else(|| CmdError::Validation("an item promotion needs an amount".into()))?;
            resolve_promo_item(conn, target, "target").await?;
            ResolvedPromotion { kind, target_item_id: Some(target), reward_item_id: None,
                percent: None, amount: Some(amount), min_total: Decimal::ZERO,
                buy_qty: None, get_qty: None, start_at, end_at }
        }
        "OrderPercent" => {
            only(&["percent", "min_total"], "target_item", has_target)?;
            only(&["percent", "min_total"], "reward_item", has_reward)?;
            only(&["percent", "min_total"], "amount", has_amount)?;
            only(&["percent", "min_total"], "buy_qty", has_buy)?;
            only(&["percent", "min_total"], "get_qty", has_get)?;
            let percent = positive(input.percent, "percent")?
                .ok_or_else(|| CmdError::Validation("an order promotion needs a percent".into()))?;
            if percent > Decimal::new(100, 0) {
                return Err(CmdError::Validation("percent cannot exceed 100".into()));
            }
            ResolvedPromotion { kind, target_item_id: None, reward_item_id: None,
                percent: Some(percent), amount: None, min_total: non_negative(input.min_total, "min_total")?,
                buy_qty: None, get_qty: None, start_at, end_at }
        }
        "OrderFixed" => {
            only(&["amount", "min_total"], "target_item", has_target)?;
            only(&["amount", "min_total"], "reward_item", has_reward)?;
            only(&["amount", "min_total"], "percent", has_percent)?;
            only(&["amount", "min_total"], "buy_qty", has_buy)?;
            only(&["amount", "min_total"], "get_qty", has_get)?;
            let amount = positive(input.amount, "amount")?
                .ok_or_else(|| CmdError::Validation("an order promotion needs an amount".into()))?;
            ResolvedPromotion { kind, target_item_id: None, reward_item_id: None,
                percent: None, amount: Some(amount), min_total: non_negative(input.min_total, "min_total")?,
                buy_qty: None, get_qty: None, start_at, end_at }
        }
        _ => {
            only(&["target_item", "reward_item", "buy_qty", "get_qty"], "percent", has_percent)?;
            only(&["target_item", "reward_item", "buy_qty", "get_qty"], "amount", has_amount)?;
            only(&["target_item", "reward_item", "buy_qty", "get_qty"], "min_total", has_min)?;
            let target = input.target_item_id.ok_or_else(|| CmdError::Validation("a buy-get promotion needs a buy item".into()))?;
            let reward = input.reward_item_id.ok_or_else(|| CmdError::Validation("a buy-get promotion needs a free item".into()))?;
            let buy_qty = positive(input.buy_qty, "buy_qty")?
                .ok_or_else(|| CmdError::Validation("a buy-get promotion needs a buy quantity".into()))?;
            let get_qty = positive(input.get_qty, "get_qty")?
                .ok_or_else(|| CmdError::Validation("a buy-get promotion needs a free quantity".into()))?;
            resolve_promo_item(conn, target, "buy").await?;
            resolve_promo_item(conn, reward, "free").await?;
            ResolvedPromotion { kind, target_item_id: Some(target), reward_item_id: Some(reward),
                percent: None, amount: None, min_total: Decimal::ZERO,
                buy_qty: Some(buy_qty), get_qty: Some(get_qty), start_at, end_at }
        }
    };
    Ok(resolved)
}

/// A live promotion overlapping this one's dates on the same scope. Item promos
/// collide per item and BuyGet collides per buy item: two discounts on one line
/// stop being explainable at the till, so the second one is refused instead of
/// stacked. Order promos are exempt — several can coexist because only the best
/// one ever applies — which is also why "best wins" needs no tie-break.
async fn overlapping_promotion<C: ConnectionTrait>(
    conn: &C,
    kind: &str,
    target_item_id: Option<i32>,
    start_at: NaiveDateTime,
    end_at: NaiveDateTime,
    exclude_id: Option<i32>,
) -> CmdResult<Option<promotion::Model>> {
    if kind != "BuyGet" && kind != "ItemPercent" && kind != "ItemFixed" {
        return Ok(None);
    }
    let target = target_item_id.ok_or_else(|| CmdError::Validation("promotion has no target item".into()))?;
    let mut q = promotion::Entity::find()
        .filter(promotion::Column::DelStatus.eq(LIVE))
        .filter(promotion::Column::Kind.eq(kind))
        .filter(promotion::Column::TargetItemId.eq(target))
        .filter(promotion::Column::StartAt.lt(end_at))
        .filter(promotion::Column::EndAt.gt(start_at));
    if let Some(id) = exclude_id {
        q = q.filter(promotion::Column::Id.ne(id));
    }
    Ok(q.one(conn).await?)
}

#[tauri::command]
pub async fn create_promotion(input: PromotionInput) -> CmdResult<promotion::Model> {
    crate::commands_auth::require_permission(db(), "promotion-create").await?;
    create_promotion_in(db(), input).await
}

pub async fn create_promotion_in<C: ConnectionTrait>(
    conn: &C,
    input: PromotionInput,
) -> CmdResult<promotion::Model> {
    let title = required(&input.title, "title")?;
    let r = resolve_promotion(conn, &input).await?;
    if overlapping_promotion(conn, r.kind, r.target_item_id, r.start_at, r.end_at, None)
        .await?
        .is_some()
    {
        return Err(CmdError::Conflict("a promotion already covers this item and these dates".into()));
    }
    let now = crate::migration::now();
    Ok(promotion::ActiveModel {
        title: Set(title),
        kind: Set(r.kind.to_owned()),
        target_item_id: Set(r.target_item_id),
        reward_item_id: Set(r.reward_item_id),
        percent: Set(r.percent),
        amount: Set(r.amount),
        min_total: Set(Some(r.min_total)),
        buy_qty: Set(r.buy_qty),
        get_qty: Set(r.get_qty),
        start_at: Set(r.start_at),
        end_at: Set(r.end_at),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(conn)
    .await?)
}

#[tauri::command]
pub async fn list_promotions(query: PageQuery) -> CmdResult<Page<promotion::Model>> {
    crate::commands_auth::require_permission(db(), "promotion-list").await?;
    list_promotions_in(db(), &query).await
}

pub async fn list_promotions_in<C: ConnectionTrait>(
    conn: &C,
    query: &PageQuery,
) -> CmdResult<Page<promotion::Model>> {
    let mut q = promotion::Entity::find().filter(promotion::Column::DelStatus.eq(LIVE));
    if let Some(term) = query.term() {
        q = q.filter(promotion::Column::Title.contains(like_term(&term)));
    }
    let total = q.clone().count(conn).await?;
    let rows = q
        .order_by_desc(promotion::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;
    Ok(Page::new(rows, total, query))
}

#[tauri::command]
pub async fn get_promotion(id: i32) -> CmdResult<promotion::Model> {
    crate::commands_auth::require_permission(db(), "promotion-show").await?;
    promotion::Entity::find_by_id(id)
        .filter(promotion::Column::DelStatus.eq(LIVE))
        .one(db())
        .await?
        .ok_or_else(|| CmdError::NotFound("promotion".into()))
}

#[tauri::command]
pub async fn update_promotion(id: i32, input: PromotionInput) -> CmdResult<promotion::Model> {
    crate::commands_auth::require_permission(db(), "promotion-edit").await?;
    update_promotion_in(db(), id, input).await
}

pub async fn update_promotion_in<C: ConnectionTrait>(
    conn: &C,
    id: i32,
    input: PromotionInput,
) -> CmdResult<promotion::Model> {
    let found = promotion::Entity::find_by_id(id)
        .filter(promotion::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("promotion".into()))?;
    let title = required(&input.title, "title")?;
    let r = resolve_promotion(conn, &input).await?;
    if overlapping_promotion(conn, r.kind, r.target_item_id, r.start_at, r.end_at, Some(id))
        .await?
        .is_some()
    {
        return Err(CmdError::Conflict("a promotion already covers this item and these dates".into()));
    }
    let mut am: promotion::ActiveModel = found.into();
    am.title = Set(title);
    am.kind = Set(r.kind.to_owned());
    am.target_item_id = Set(r.target_item_id);
    am.reward_item_id = Set(r.reward_item_id);
    am.percent = Set(r.percent);
    am.amount = Set(r.amount);
    am.min_total = Set(Some(r.min_total));
    am.buy_qty = Set(r.buy_qty);
    am.get_qty = Set(r.get_qty);
    am.start_at = Set(r.start_at);
    am.end_at = Set(r.end_at);
    am.updated_at = Set(crate::migration::now());
    Ok(am.update(conn).await?)
}

#[tauri::command]
pub async fn delete_promotion(id: i32) -> CmdResult<()> {
    crate::commands_auth::require_permission(db(), "promotion-destroy").await?;
    delete_promotion_in(db(), id).await
}

pub async fn delete_promotion_in<C: ConnectionTrait>(conn: &C, id: i32) -> CmdResult<()> {
    // Hard delete: a rule carries no money, and past sales record the discount
    // amount on their lines without pointing back at the rule.
    let deleted = promotion::Entity::delete_many()
        .filter(promotion::Column::Id.eq(id))
        .filter(promotion::Column::DelStatus.eq(LIVE))
        .exec(conn)
        .await?;
    if deleted.rows_affected == 0 {
        return Err(CmdError::NotFound("promotion".into()));
    }
    Ok(())
}

/// One cart line as the promotion engine sees it. Manual discounts ride along so
/// the engine can leave them alone: a cashier's explicit discount always wins and
/// a promo never stacks on top of it.
struct PromoLine {
    item_id: i32,
    quantity: Decimal,
    unit_price: Decimal,
    manual: Decimal,
}

struct PromoOutcome {
    /// Promo discount per line, aligned with the input order.
    line_promos: Vec<Decimal>,
    /// The winning order-level promo amount, already capped.
    order_promo: Decimal,
}

/// Discounts the till applies by itself. Pure computation over loaded rows: the
/// only database touch is reading the active rules, so this is unit-testable
/// through checkout rather than needing its own harness.
async fn apply_promotions<C: ConnectionTrait>(
    conn: &C,
    lines: &[PromoLine],
    at: NaiveDateTime,
) -> CmdResult<PromoOutcome> {
    let promos: Vec<promotion::Model> = promotion::Entity::find()
        .filter(promotion::Column::DelStatus.eq(LIVE))
        .filter(promotion::Column::StartAt.lte(at))
        .filter(promotion::Column::EndAt.gt(at))
        .all(conn)
        .await?;

    let subtotal: Decimal = lines.iter().map(|l| l.unit_price * l.quantity).sum();
    let mut line_promos = vec![Decimal::ZERO; lines.len()];

    for (i, line) in lines.iter().enumerate() {
        if line.manual > Decimal::ZERO {
            continue;
        }
        let gross = line.unit_price * line.quantity;
        let mut best = Decimal::ZERO;
        for promo in promos.iter().filter(|p| {
            (p.kind == "ItemPercent" || p.kind == "ItemFixed") && p.target_item_id == Some(line.item_id)
        }) {
            let value = match promo.kind.as_str() {
                "ItemPercent" => gross * promo.percent.unwrap_or(Decimal::ZERO) / Decimal::new(100, 0),
                _ => promo.amount.unwrap_or(Decimal::ZERO) * line.quantity,
            };
            // Capped at the line: a fixed amount larger than a cheap line is a
            // free item, not a negative one.
            best = best.max(value.min(gross));
        }
        line_promos[i] = best.round_dp(MONEY_SCALE);
    }

    for (i, line) in lines.iter().enumerate() {
        let gross = line.unit_price * line.quantity;
        let mut earned_value = Decimal::ZERO;
        for promo in promos.iter().filter(|p| p.kind == "BuyGet") {
            let Some(target) = promo.target_item_id else { continue };
            let bought: Decimal = lines
                .iter()
                .filter(|l| l.item_id == target)
                .map(|l| l.quantity)
                .sum();
            let sets = (bought / promo.buy_qty.unwrap_or(Decimal::ONE)).floor();
            if sets <= Decimal::ZERO {
                continue;
            }
            // Valued at this line's own price: the free units are worth what the
            // customer would otherwise have paid for them here.
            if Some(line.item_id) == promo.reward_item_id && line.manual <= Decimal::ZERO {
                earned_value += sets * promo.get_qty.unwrap_or(Decimal::ZERO) * line.unit_price;
            }
        }
        if earned_value > Decimal::ZERO {
            line_promos[i] = (line_promos[i] + earned_value.min(gross)).round_dp(MONEY_SCALE);
        }
    }

    let mut order_promo = Decimal::ZERO;
    for promo in promos.iter().filter(|p| p.kind == "OrderPercent" || p.kind == "OrderFixed") {
        if subtotal < promo.min_total.unwrap_or(Decimal::ZERO) {
            continue;
        }
        let value = match promo.kind.as_str() {
            "OrderPercent" => subtotal * promo.percent.unwrap_or(Decimal::ZERO) / Decimal::new(100, 0),
            _ => promo.amount.unwrap_or(Decimal::ZERO),
        };
        order_promo = order_promo.max(value);
    }
    // Capped at what is left after the line promos: the grand total cannot go
    // negative no matter how the rules combine.
    let line_total: Decimal = line_promos.iter().sum();
    order_promo = order_promo.min((subtotal - line_total).max(Decimal::ZERO)).round_dp(MONEY_SCALE);

    Ok(PromoOutcome { line_promos, order_promo })
}

// ---------------------------------------------------------------------------
// Combos
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComboItemInput {
    pub combo_item_id: i32,
    pub item_id: i32,
    pub quantity: Decimal,
}

/// Components of a bundle, each with the total units one sale line moves.
async fn combo_needs<C: ConnectionTrait>(
    conn: &C,
    combo_item_id: i32,
    bundle_qty: Decimal,
) -> CmdResult<Vec<(i32, Decimal)>> {
    let rows: Vec<combo_item::Model> = combo_item::Entity::find()
        .filter(combo_item::Column::ComboItemId.eq(combo_item_id))
        .order_by_asc(combo_item::Column::ItemId)
        .all(conn)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push((row.item_id, (row.quantity * bundle_qty).round_dp(MONEY_SCALE)));
    }
    Ok(out)
}

/// Guard for a bundle line. The bundle itself needs no stock — it is virtual, and
/// requiring shelf for it would refuse every bundle sale. Each component must
/// exist, be Live, and cover its share; the message names the component, because
/// that is the shelf the cashier has to go and look at.
async fn guard_combo_sellable<C: ConnectionTrait>(
    conn: &C,
    combo_item_id: i32,
    bundle_name: &str,
    bundle_qty: Decimal,
) -> CmdResult<Vec<(i32, Decimal)>> {
    let needs = combo_needs(conn, combo_item_id, bundle_qty).await?;
    for (item_id, need) in &needs {
        let Some(component) = item::Entity::find_by_id(*item_id)
            .filter(item::Column::DelStatus.eq(LIVE))
            .one(conn)
            .await?
        else {
            return Err(CmdError::NotFound(format!("item {item_id}")));
        };
        if !combo_components(conn, *item_id).await?.is_empty() {
            return Err(CmdError::Validation(format!(
                "'{bundle_name}' contains '{}', which is itself a bundle",
                component.name,
            )));
        }
        let available = on_hand_in(conn, *item_id).await?;
        if *need > available {
            return Err(CmdError::Validation(format!(
                "'{bundle_name}' needs {need} of '{}' but only {available} in stock",
                component.name,
            )));
        }
    }
    Ok(needs)
}

async fn combo_components<C: ConnectionTrait>(
    conn: &C,
    combo_item_id: i32,
) -> CmdResult<Vec<combo_item::Model>> {
    Ok(combo_item::Entity::find()
        .filter(combo_item::Column::ComboItemId.eq(combo_item_id))
        .all(conn)
        .await?)
}

/// Writes the explosion audit plus one ledger movement per component. Shared by
/// checkout and promote so the two spell a bundle sale identically.
async fn record_combo_sale<C: ConnectionTrait>(
    conn: &C,
    sale_id: i32,
    detail_id: i32,
    combo_item_id: i32,
    needs: &[(i32, Decimal)],
    invoice_no: &str,
    now: NaiveDateTime,
    stock_on_hand: &mut Vec<ItemOnHand>,
    seen: &mut HashMap<i32, usize>,
) -> CmdResult<()> {
    for (item_id, need) in needs {
        combo_sale::ActiveModel {
            sale_id: Set(sale_id),
            sale_detail_id: Set(detail_id),
            combo_item_id: Set(combo_item_id),
            item_id: Set(*item_id),
            quantity: Set(*need),
            ..Default::default()
        }
        .insert(conn)
        .await?;
        let balance_after = on_hand_in(conn, *item_id).await? - *need;
        record_sale_movement(conn, *item_id, sale_id, invoice_no, *need, balance_after, now).await?;
        record_on_hand(stock_on_hand, seen, *item_id, balance_after);
    }
    Ok(())
}

#[tauri::command]
pub async fn create_combo_item(input: ComboItemInput) -> CmdResult<combo_item::Model> {
    // Guarded as catalog, not its own group: a bundle definition is what an item
    // means, and a fourth permission group for three commands buys nothing.
    crate::commands_auth::require_permission(db(), "item-create").await?;
    create_combo_item_in(db(), input).await
}

pub async fn create_combo_item_in<C: ConnectionTrait>(
    conn: &C,
    input: ComboItemInput,
) -> CmdResult<combo_item::Model> {
    if input.quantity <= Decimal::ZERO {
        return Err(CmdError::Validation("component quantity must be greater than zero".into()));
    }
    if input.combo_item_id == input.item_id {
        return Err(CmdError::Validation("an item cannot contain itself".into()));
    }
    for (id, field) in [(input.combo_item_id, "bundle"), (input.item_id, "component")] {
        item::Entity::find_by_id(id)
            .filter(item::Column::DelStatus.eq(LIVE))
            .one(conn)
            .await?
            .ok_or_else(|| CmdError::NotFound(format!("{field} item {id}")))?;
    }
    if !combo_components(conn, input.item_id).await?.is_empty() {
        return Err(CmdError::Validation(
            "a bundle cannot contain another bundle: the explosion is one level".into(),
        ));
    }
    if combo_item::Entity::find()
        .filter(combo_item::Column::ComboItemId.eq(input.combo_item_id))
        .filter(combo_item::Column::ItemId.eq(input.item_id))
        .one(conn)
        .await?
        .is_some()
    {
        return Err(CmdError::Conflict("this item is already a component of the bundle".into()));
    }
    Ok(combo_item::ActiveModel {
        combo_item_id: Set(input.combo_item_id),
        item_id: Set(input.item_id),
        quantity: Set(input.quantity),
        ..Default::default()
    }
    .insert(conn)
    .await?)
}

#[tauri::command]
pub async fn list_combo_items(combo_item_id: Option<i32>, query: PageQuery) -> CmdResult<Page<combo_item::Model>> {
    crate::commands_auth::require_permission(db(), "item-list").await?;
    let db = db();
    let mut q = combo_item::Entity::find();
    if let Some(bundle) = combo_item_id {
        q = q.filter(combo_item::Column::ComboItemId.eq(bundle));
    }
    let total = q.clone().count(db).await?;
    let rows = q
        .order_by_asc(combo_item::Column::ComboItemId)
        .order_by_asc(combo_item::Column::ItemId)
        .offset(query.offset())
        .limit(query.per_page())
        .all(db)
        .await?;
    Ok(Page::new(rows, total, &query))
}

#[tauri::command]
pub async fn delete_combo_item(id: i32) -> CmdResult<()> {
    crate::commands_auth::require_permission(db(), "item-destroy").await?;
    delete_combo_item_in(db(), id).await
}

pub async fn delete_combo_item_in<C: ConnectionTrait>(conn: &C, id: i32) -> CmdResult<()> {
    // Hard delete like promotions: past sales keep their own explosion rows, so no
    // history points here.
    let deleted = combo_item::Entity::delete_by_id(id).exec(conn).await?;
    if deleted.rows_affected == 0 {
        return Err(CmdError::NotFound("combo item".into()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Installments
// ---------------------------------------------------------------------------

/// A credit sale: one item handed over now, the balance split into dated dues.
/// Totals are derived server-side — the client sends the knobs, never the answer.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstallmentInput {
    pub customer_id: i32,
    pub item_id: i32,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    #[serde(default)]
    pub discount_amount: Option<Decimal>,
    /// Percent on the discounted price, e.g. `10` for ten percent.
    #[serde(default)]
    pub interest_percent: Option<Decimal>,
    #[serde(default)]
    pub other_charges: Option<Decimal>,
    #[serde(default)]
    pub down_payment: Option<Decimal>,
    #[serde(default)]
    pub down_payment_method: Option<String>,
    pub number_of_installments: i32,
    /// Days between dues. Defaults to 30, the reference's `installment_type`.
    #[serde(default)]
    pub interval_days: Option<i32>,
    #[serde(default)]
    pub note: Option<String>,
}

fn installment_totals(input: &CreateInstallmentInput) -> CmdResult<(Decimal, Decimal, Decimal)> {
    if input.quantity <= Decimal::ZERO {
        return Err(CmdError::Validation("quantity must be greater than zero".into()));
    }
    if input.unit_price < Decimal::ZERO {
        return Err(CmdError::Validation("unit price cannot be negative".into()));
    }
    let subtotal = input.quantity * input.unit_price;
    let discount = input.discount_amount.unwrap_or(Decimal::ZERO);
    if discount < Decimal::ZERO {
        return Err(CmdError::Validation("discount cannot be negative".into()));
    }
    if discount > subtotal {
        return Err(CmdError::Validation("discount is larger than the price".into()));
    }
    let interest_percent = input.interest_percent.unwrap_or(Decimal::ZERO);
    if interest_percent < Decimal::ZERO {
        return Err(CmdError::Validation("interest cannot be negative".into()));
    }
    let other = input.other_charges.unwrap_or(Decimal::ZERO);
    if other < Decimal::ZERO {
        return Err(CmdError::Validation("other charges cannot be negative".into()));
    }
    // `(price - discount) + interest + other`, like the reference — except every
    // figure is `Decimal`, so `0.1 + 0.2` never reaches the ledger.
    let interest = (subtotal - discount) * interest_percent / Decimal::new(100, 0);
    let total = subtotal - discount + interest + other;

    let down = input.down_payment.unwrap_or(Decimal::ZERO);
    if down < Decimal::ZERO {
        return Err(CmdError::Validation("down payment cannot be negative".into()));
    }
    if down > total {
        return Err(CmdError::Validation("down payment is larger than the total".into()));
    }
    if input.number_of_installments < 1 {
        return Err(CmdError::Validation("at least one installment is required".into()));
    }
    if input.interval_days.unwrap_or(30) < 1 {
        return Err(CmdError::Validation("the interval must be at least a day".into()));
    }
    Ok((total, down, interest))
}

/// One due with its derived remainder and status, so the screen never computes money.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallmentDetailView {
    pub id: i32,
    pub due_date: NaiveDateTime,
    pub amount: Decimal,
    pub paid_amount: Decimal,
    pub remaining_amount: Decimal,
    /// `Unpaid`, `Partial` or `Paid` — derived from `amount - paid_amount`.
    pub paid_status: String,
    pub paid_date: Option<NaiveDateTime>,
    pub payment_method: Option<String>,
}

fn detail_view(row: installment_sale_detail::Model) -> InstallmentDetailView {
    let remaining = row.amount - row.paid_amount;
    let paid_status = if row.paid_amount <= Decimal::ZERO {
        "Unpaid"
    } else if remaining <= Decimal::ZERO {
        "Paid"
    } else {
        "Partial"
    };
    InstallmentDetailView {
        id: row.id,
        due_date: row.due_date,
        amount: row.amount,
        paid_amount: row.paid_amount,
        remaining_amount: remaining.max(Decimal::ZERO),
        paid_status: paid_status.to_owned(),
        paid_date: row.paid_date,
        payment_method: row.payment_method,
    }
}

/// An installment sale with its schedule and derived paid/due figures.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallmentView {
    pub sale: installment_sale::Model,
    pub details: Vec<InstallmentDetailView>,
    pub customer_name: Option<String>,
    pub item_name: String,
    /// `down_payment + SUM(details.paid_amount)`. Derived, never stored.
    pub paid_total: Decimal,
    pub due_total: Decimal,
    /// `Active` while anything is due, `Completed` once it is all paid.
    pub status: String,
}

async fn installment_view<C: ConnectionTrait>(
    conn: &C,
    header: installment_sale::Model,
) -> CmdResult<InstallmentView> {
    let rows = installment_sale_detail::Entity::find()
        .filter(installment_sale_detail::Column::InstallmentSaleId.eq(header.id))
        .filter(installment_sale_detail::Column::DelStatus.eq(LIVE))
        .order_by_asc(installment_sale_detail::Column::DueDate)
        .all(conn)
        .await?;

    let customer_name = customer::Entity::find_by_id(header.customer_id)
        .one(conn)
        .await?
        .map(|c| c.name);
    let item_name = item::Entity::find_by_id(header.item_id)
        .one(conn)
        .await?
        .map(|i| i.name)
        .unwrap_or_default();

    let mut paid = header.down_payment;
    let mut details = Vec::with_capacity(rows.len());
    for row in rows {
        paid += row.paid_amount;
        details.push(detail_view(row));
    }
    let due = (header.total - paid).max(Decimal::ZERO);
    let status = if due > Decimal::ZERO { "Active" } else { "Completed" };
    Ok(InstallmentView {
        sale: header,
        details,
        customer_name,
        item_name,
        paid_total: paid,
        due_total: due,
        status: status.to_owned(),
    })
}

#[tauri::command]
pub async fn create_installment_sale(input: CreateInstallmentInput) -> CmdResult<InstallmentView> {
    crate::commands_auth::require_permission(db(), "installment-create").await?;
    let Some(user_id) = crate::auth::current_user_id() else {
        return Err(CmdError::Forbidden("you are not signed in".into()));
    };
    create_installment_sale_in(db(), user_id, input).await
}

pub(crate) async fn create_installment_sale_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    user_id: i32,
    input: CreateInstallmentInput,
) -> CmdResult<InstallmentView> {
    let (total, down, interest) = installment_totals(&input)?;
    let interval = input.interval_days.unwrap_or(30);
    let now = crate::migration::now();

    let txn = conn.begin().await?;

    customer::Entity::find_by_id(input.customer_id)
        .filter(customer::Column::DelStatus.eq(LIVE))
        .one(&txn)
        .await?
        .ok_or_else(|| CmdError::NotFound("customer".into()))?;

    let item = item::Entity::find_by_id(input.item_id)
        .filter(item::Column::DelStatus.eq(LIVE))
        .one(&txn)
        .await?
        .ok_or_else(|| CmdError::NotFound(format!("item {}", input.item_id)))?;

    // Same hard block as checkout: the shelf cannot go negative until the
    // `allow_negative_stock` setting exists to say otherwise.
    let available = on_hand_in(&txn, input.item_id).await?;
    if input.quantity > available {
        return Err(CmdError::Validation(format!(
            "item '{}' has {available} in stock but {} was requested",
            item.name, input.quantity
        )));
    }

    let header = installment_sale::ActiveModel {
        reference_no: Set(String::new()),
        customer_id: Set(input.customer_id),
        item_id: Set(input.item_id),
        quantity: Set(input.quantity),
        unit_price: Set(input.unit_price),
        discount_amount: Set(input.discount_amount.unwrap_or(Decimal::ZERO)),
        interest_percent: Set(input.interest_percent.unwrap_or(Decimal::ZERO)),
        interest_amount: Set(interest),
        other_charges: Set(input.other_charges.unwrap_or(Decimal::ZERO)),
        total: Set(total),
        down_payment: Set(down),
        down_payment_method: Set(input.down_payment_method),
        number_of_installments: Set(input.number_of_installments),
        interval_days: Set(interval),
        created_by: Set(Some(user_id)),
        note: Set(input.note),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;

    // The reference number is a function of the primary key, so it cannot collide
    // under concurrency — same derivation as the sale invoice number.
    let reference_no = format!("INST-{:06}", header.id);
    let mut header_am: installment_sale::ActiveModel = header.into();
    header_am.reference_no = Set(reference_no.clone());
    let header = header_am.update(&txn).await?;

    // Auto-split, floor division with the remainder on the first due — the
    // reference's split, so the dues always add up to the remaining figure.
    let remaining = total - down;
    let count = input.number_of_installments;
    let divided =
        (remaining / Decimal::from(count)).trunc_with_scale(crate::migration::DECIMAL_SCALE);
    for i in 1..=count {
        let amount = if i == 1 { remaining - divided * Decimal::from(count - 1) } else { divided };
        let due_date = now
            .checked_add_days(chrono::Days::new(interval as u64 * i as u64))
            .ok_or_else(|| CmdError::Validation("due date is out of range".to_owned()))?;
        installment_sale_detail::ActiveModel {
            installment_sale_id: Set(header.id),
            due_date: Set(due_date),
            amount: Set(amount),
            paid_amount: Set(Decimal::ZERO),
            paid_date: Set(None),
            payment_method: Set(None),
            del_status: Set(LIVE.to_owned()),
            created_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
    }

    // The goods leave now, not when the last due clears — a handover is a handover.
    let balance_after = on_hand_in(&txn, input.item_id).await? - input.quantity;
    stock_movement::ActiveModel {
        item_id: Set(input.item_id),
        sale_id: Set(None),
        installment_sale_id: Set(Some(header.id)),
        movement_type: Set(MovementType::InstallmentSale.as_str().to_owned()),
        quantity: Set(-input.quantity),
        reference: Set(Some(reference_no)),
        balance_after: Set(balance_after),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;

    txn.commit().await?;
    installment_view(conn, header).await
}

#[tauri::command]
pub async fn collect_installment_payment(
    detail_id: i32,
    amount: Decimal,
    payment_method: Option<String>,
) -> CmdResult<InstallmentView> {
    crate::commands_auth::require_permission(db(), "installment-collect").await?;
    collect_installment_payment_in(db(), detail_id, amount, payment_method).await
}

pub(crate) async fn collect_installment_payment_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    detail_id: i32,
    amount: Decimal,
    payment_method: Option<String>,
) -> CmdResult<InstallmentView> {
    if amount <= Decimal::ZERO {
        return Err(CmdError::Validation("payment must be greater than zero".into()));
    }
    if let Some(method) = payment_method.as_deref() {
        required(method, "payment method")?;
    }

    let txn = conn.begin().await?;
    let due = installment_sale_detail::Entity::find_by_id(detail_id)
        .filter(installment_sale_detail::Column::DelStatus.eq(LIVE))
        .one(&txn)
        .await?
        .ok_or_else(|| CmdError::NotFound("installment due".into()))?;
    // The header going soft-deleted closes the plan; collecting against a closed
    // plan would take money for a sale that no longer exists.
    let header = installment_sale::Entity::find_by_id(due.installment_sale_id)
        .filter(installment_sale::Column::DelStatus.eq(LIVE))
        .one(&txn)
        .await?
        .ok_or_else(|| CmdError::NotFound("installment sale".into()))?;

    // Refused, not applied to the next due: an overpayment is change the cashier
    // holds, same rule as tenders above the total at the till.
    let outstanding = due.amount - due.paid_amount;
    if amount > outstanding {
        return Err(CmdError::Validation(format!(
            "that due has {outstanding} outstanding, not {amount}"
        )));
    }

    let paid = due.paid_amount + amount;
    let fully_paid = paid >= due.amount;
    let mut due_am: installment_sale_detail::ActiveModel = due.into();
    due_am.paid_amount = Set(paid);
    if fully_paid {
        due_am.paid_date = Set(Some(crate::migration::now()));
    }
    due_am.payment_method = Set(payment_method);
    due_am.update(&txn).await?;

    txn.commit().await?;
    installment_view(conn, header).await
}

/// A credit sale row with the customer and item named, so the list shows no ids.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallmentSummary {
    pub id: i32,
    pub reference_no: String,
    pub customer_id: i32,
    pub customer_name: Option<String>,
    pub item_name: String,
    pub total: Decimal,
    pub paid_total: Decimal,
    pub due_total: Decimal,
    pub status: String,
    pub created_at: NaiveDateTime,
}

#[tauri::command]
pub async fn list_installments(
    customer_id: Option<i32>,
    query: PageQuery,
) -> CmdResult<Page<InstallmentSummary>> {
    crate::commands_auth::require_permission(db(), "installment-list").await?;
    list_installments_in(db(), customer_id, &query).await
}

pub async fn list_installments_in<C: ConnectionTrait>(
    conn: &C,
    customer_id: Option<i32>,
    query: &PageQuery,
) -> CmdResult<Page<InstallmentSummary>> {
    let mut q = installment_sale::Entity::find()
        .filter(installment_sale::Column::DelStatus.eq(LIVE));
    if let Some(customer_id) = customer_id {
        q = q.filter(installment_sale::Column::CustomerId.eq(customer_id));
    }

    let total = q.clone().count(conn).await?;
    let rows = q
        .order_by_desc(installment_sale::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;

    // One query for the names on the page rather than one per row.
    let customer_ids: Vec<i32> = rows.iter().map(|r| r.customer_id).collect();
    let customers = customer::Entity::find()
        .filter(customer::Column::Id.is_in(customer_ids))
        .all(conn)
        .await?;
    let customer_names: std::collections::HashMap<i32, String> =
        customers.into_iter().map(|c| (c.id, c.name)).collect();
    let item_ids: Vec<i32> = rows.iter().map(|r| r.item_id).collect();
    let items = item::Entity::find()
        .filter(item::Column::Id.is_in(item_ids))
        .all(conn)
        .await?;
    let item_names: std::collections::HashMap<i32, String> =
        items.into_iter().map(|i| (i.id, i.name)).collect();

    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        let detail_paid: Decimal = installment_sale_detail::Entity::find()
            .filter(installment_sale_detail::Column::InstallmentSaleId.eq(row.id))
            .filter(installment_sale_detail::Column::DelStatus.eq(LIVE))
            .all(conn)
            .await?
            .iter()
            .map(|d| d.paid_amount)
            .sum();
        let paid = row.down_payment + detail_paid;
        let due = (row.total - paid).max(Decimal::ZERO);
        views.push(InstallmentSummary {
            id: row.id,
            reference_no: row.reference_no,
            customer_id: row.customer_id,
            customer_name: customer_names.get(&row.customer_id).cloned(),
            item_name: item_names.get(&row.item_id).cloned().unwrap_or_default(),
            total: row.total,
            paid_total: paid,
            due_total: due,
            status: if due > Decimal::ZERO { "Active".to_owned() } else { "Completed".to_owned() },
            created_at: row.created_at,
        });
    }
    Ok(Page::new(views, total, query))
}

#[tauri::command]
pub async fn get_installment_sale(id: i32) -> CmdResult<InstallmentView> {
    crate::commands_auth::require_permission(db(), "installment-show").await?;
    get_installment_sale_in(db(), id).await
}

pub async fn get_installment_sale_in<C: ConnectionTrait>(
    conn: &C,
    id: i32,
) -> CmdResult<InstallmentView> {
    let header = installment_sale::Entity::find_by_id(id)
        .filter(installment_sale::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("installment sale".into()))?;
    installment_view(conn, header).await
}

// ---------------------------------------------------------------------------
// Warranty and servicing
// ---------------------------------------------------------------------------

/// Where a warranty ticket sits in the pipeline. Decoded from the reference:
/// received from customer, sent to vendor, received from vendor, delivered.
pub const WARRANTY_STATUSES: &[&str] = &["R_F_C", "S_T_V", "R_T_V", "D_T_C"];

/// Where a paid repair job sits. Closed here; the reference leaves it free text,
/// which splits one state across spellings in every report.
pub const SERVICING_STATUSES: &[&str] = &["Received", "InRepair", "Ready", "Delivered"];

/// Midnight at the start of `YYYY-MM-DD`. Repair dates are days, not moments.
fn parse_service_day(raw: Option<&str>, field: &str) -> CmdResult<Option<NaiveDateTime>> {
    match raw.map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(text) => {
            match chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
            {
                Some(at) => Ok(Some(at)),
                None => Err(CmdError::Validation(format!("{field} is not a date"))),
            }
        }
    }
}

/// The receiving date is always present — a ticket without one never happened.
fn require_service_day(raw: &str, field: &str) -> CmdResult<NaiveDateTime> {
    parse_service_day(Some(raw), field)?.ok_or_else(|| CmdError::Validation(format!("{field} is required")))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WarrantyInput {
    pub customer_id: i32,
    pub product_name: String,
    #[serde(default)]
    pub product_serial_no: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// `YYYY-MM-DD`.
    pub receiving_date: String,
    /// `YYYY-MM-DD`, at or after the receiving date.
    #[serde(default)]
    pub delivery_date: Option<String>,
    #[serde(default)]
    pub technician_id: Option<i32>,
    #[serde(default)]
    pub present_location: Option<String>,
    #[serde(default)]
    pub sender_service_center: Option<String>,
    #[serde(default)]
    pub receiver_service_center: Option<String>,
}

fn resolve_warranty_status(raw: Option<&str>) -> CmdResult<String> {
    match raw.map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok("R_F_C".to_owned()),
        Some(status) if WARRANTY_STATUSES.contains(&status) => Ok(status.to_owned()),
        Some(status) => Err(CmdError::Validation(format!("{status} is not a warranty status"))),
    }
}

async fn guard_ticket_party<C: ConnectionTrait>(
    conn: &C,
    customer_id: i32,
    technician_id: Option<i32>,
) -> CmdResult<()> {
    customer::Entity::find_by_id(customer_id)
        .filter(customer::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("customer".into()))?;
    if let Some(technician_id) = technician_id {
        users::Entity::find_by_id(technician_id)
            .one(conn)
            .await?
            .ok_or_else(|| CmdError::NotFound("technician".into()))?;
    }
    Ok(())
}

/// A warranty row with the customer named, so the list shows no ids.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarrantySummary {
    pub id: i32,
    pub customer_name: Option<String>,
    pub product_name: String,
    pub product_serial_no: Option<String>,
    pub receiving_date: NaiveDateTime,
    pub delivery_date: Option<NaiveDateTime>,
    pub current_status: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarrantyView {
    pub warranty: warranty::Model,
    pub customer_name: Option<String>,
}

fn warranty_summary(
    row: warranty::Model,
    names: &std::collections::HashMap<i32, String>,
) -> WarrantySummary {
    WarrantySummary {
        id: row.id,
        customer_name: names.get(&row.customer_id).cloned(),
        product_name: row.product_name,
        product_serial_no: row.product_serial_no,
        receiving_date: row.receiving_date,
        delivery_date: row.delivery_date,
        current_status: row.current_status,
        created_at: row.created_at,
    }
}

async fn ticket_customer_names<C: ConnectionTrait>(
    conn: &C,
    ids: &[i32],
) -> CmdResult<std::collections::HashMap<i32, String>> {
    if ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    Ok(customer::Entity::find()
        .filter(customer::Column::Id.is_in(ids.to_vec()))
        .all(conn)
        .await?
        .into_iter()
        .map(|c| (c.id, c.name))
        .collect())
}

#[tauri::command]
pub async fn create_warranty(input: WarrantyInput) -> CmdResult<WarrantyView> {
    crate::commands_auth::require_permission(db(), "warranty-create").await?;
    let Some(user_id) = crate::auth::current_user_id() else {
        return Err(CmdError::Forbidden("you are not signed in".into()));
    };
    create_warranty_in(db(), user_id, input).await
}

pub(crate) async fn create_warranty_in<C: ConnectionTrait>(
    conn: &C,
    user_id: i32,
    input: WarrantyInput,
) -> CmdResult<WarrantyView> {
    let product_name = required(&input.product_name, "product name")?;
    let receiving_date = require_service_day(&input.receiving_date, "receiving date")?;
    let delivery_date = parse_service_day(input.delivery_date.as_deref(), "delivery date")?;
    if delivery_date.is_some_and(|d| d < receiving_date) {
        return Err(CmdError::Validation("delivery cannot be before receiving".into()));
    }
    guard_ticket_party(conn, input.customer_id, input.technician_id).await?;

    let now = crate::migration::now();
    let row = warranty::ActiveModel {
        customer_id: Set(input.customer_id),
        product_name: Set(product_name),
        product_serial_no: Set(blank_to_none(input.product_serial_no)),
        description: Set(blank_to_none(input.description)),
        receiving_date: Set(receiving_date),
        delivery_date: Set(delivery_date),
        current_status: Set(resolve_warranty_status(None)?),
        technician_id: Set(input.technician_id),
        present_location: Set(blank_to_none(input.present_location)),
        sender_service_center: Set(blank_to_none(input.sender_service_center)),
        receiver_service_center: Set(blank_to_none(input.receiver_service_center)),
        created_by: Set(Some(user_id)),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(conn)
    .await?;
    get_warranty_in(conn, row.id).await
}

/// Empty boxes arrive as `""`, and the column wants NULL, not a blank string.
fn blank_to_none(raw: Option<String>) -> Option<String> {
    raw.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

#[tauri::command]
pub async fn set_warranty_status(id: i32, status: String) -> CmdResult<WarrantyView> {
    crate::commands_auth::require_permission(db(), "warranty-status").await?;
    set_warranty_status_in(db(), id, &status).await
}

pub(crate) async fn set_warranty_status_in<C: ConnectionTrait>(
    conn: &C,
    id: i32,
    status: &str,
) -> CmdResult<WarrantyView> {
    // Any of the four, in any order: a shop may hand a unit straight back without
    // involving a vendor, so enforcing the pipeline order would refuse real work.
    let status = resolve_warranty_status(Some(status))?;
    let ticket = warranty::Entity::find_by_id(id)
        .filter(warranty::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("warranty".into()))?;
    let mut am: warranty::ActiveModel = ticket.into();
    am.current_status = Set(status);
    am.updated_at = Set(crate::migration::now());
    am.update(conn).await?;
    get_warranty_in(conn, id).await
}

#[tauri::command]
pub async fn list_warranties(
    customer_id: Option<i32>,
    query: PageQuery,
) -> CmdResult<Page<WarrantySummary>> {
    crate::commands_auth::require_permission(db(), "warranty-list").await?;
    list_warranties_in(db(), customer_id, &query).await
}

pub async fn list_warranties_in<C: ConnectionTrait>(
    conn: &C,
    customer_id: Option<i32>,
    query: &PageQuery,
) -> CmdResult<Page<WarrantySummary>> {
    let mut q = warranty::Entity::find().filter(warranty::Column::DelStatus.eq(LIVE));
    if let Some(customer_id) = customer_id {
        q = q.filter(warranty::Column::CustomerId.eq(customer_id));
    }

    let total = q.clone().count(conn).await?;
    let rows = q
        .order_by_desc(warranty::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;

    let ids: Vec<i32> = rows.iter().map(|r| r.customer_id).collect();
    let names = ticket_customer_names(conn, &ids).await?;
    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        views.push(warranty_summary(row, &names));
    }
    Ok(Page::new(views, total, query))
}

#[tauri::command]
pub async fn get_warranty(id: i32) -> CmdResult<WarrantyView> {
    crate::commands_auth::require_permission(db(), "warranty-show").await?;
    get_warranty_in(db(), id).await
}

pub async fn get_warranty_in<C: ConnectionTrait>(conn: &C, id: i32) -> CmdResult<WarrantyView> {
    let row = warranty::Entity::find_by_id(id)
        .filter(warranty::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("warranty".into()))?;
    let customer_name = customer::Entity::find_by_id(row.customer_id)
        .one(conn)
        .await?
        .map(|c| c.name);
    Ok(WarrantyView { warranty: row, customer_name })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicingInput {
    pub customer_id: i32,
    pub product_name: String,
    #[serde(default)]
    pub product_model: Option<String>,
    #[serde(default)]
    pub problem_description: Option<String>,
    /// `YYYY-MM-DD`.
    pub receiving_date: String,
    /// `YYYY-MM-DD`, at or after the receiving date.
    #[serde(default)]
    pub delivery_date: Option<String>,
    pub servicing_charge: Decimal,
    #[serde(default)]
    pub technician_id: Option<i32>,
}

fn resolve_servicing_status(raw: Option<&str>) -> CmdResult<String> {
    match raw.map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok("Received".to_owned()),
        Some(status) if SERVICING_STATUSES.contains(&status) => Ok(status.to_owned()),
        Some(status) => Err(CmdError::Validation(format!("{status} is not a servicing status"))),
    }
}

/// A servicing row with the customer named and the due derived.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicingSummary {
    pub id: i32,
    pub customer_name: Option<String>,
    pub product_name: String,
    pub servicing_charge: Decimal,
    pub paid_amount: Decimal,
    /// `charge - paid`, floored at zero. Derived, never stored.
    pub due_amount: Decimal,
    pub current_status: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicingView {
    pub servicing: servicing::Model,
    pub customer_name: Option<String>,
    pub due_amount: Decimal,
}

fn servicing_due(row: &servicing::Model) -> Decimal {
    (row.servicing_charge - row.paid_amount).max(Decimal::ZERO)
}

#[tauri::command]
pub async fn create_servicing(input: ServicingInput) -> CmdResult<ServicingView> {
    crate::commands_auth::require_permission(db(), "servicing-create").await?;
    let Some(user_id) = crate::auth::current_user_id() else {
        return Err(CmdError::Forbidden("you are not signed in".into()));
    };
    create_servicing_in(db(), user_id, input).await
}

pub(crate) async fn create_servicing_in<C: ConnectionTrait>(
    conn: &C,
    user_id: i32,
    input: ServicingInput,
) -> CmdResult<ServicingView> {
    let product_name = required(&input.product_name, "product name")?;
    let receiving_date = require_service_day(&input.receiving_date, "receiving date")?;
    let delivery_date = parse_service_day(input.delivery_date.as_deref(), "delivery date")?;
    if delivery_date.is_some_and(|d| d < receiving_date) {
        return Err(CmdError::Validation("delivery cannot be before receiving".into()));
    }
    if input.servicing_charge < Decimal::ZERO {
        return Err(CmdError::Validation("charge cannot be negative".into()));
    }
    guard_ticket_party(conn, input.customer_id, input.technician_id).await?;

    let now = crate::migration::now();
    let row = servicing::ActiveModel {
        customer_id: Set(input.customer_id),
        product_name: Set(product_name),
        product_model: Set(blank_to_none(input.product_model)),
        problem_description: Set(blank_to_none(input.problem_description)),
        receiving_date: Set(receiving_date),
        delivery_date: Set(delivery_date),
        servicing_charge: Set(input.servicing_charge),
        paid_amount: Set(Decimal::ZERO),
        current_status: Set(resolve_servicing_status(None)?),
        technician_id: Set(input.technician_id),
        created_by: Set(Some(user_id)),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(conn)
    .await?;
    get_servicing_in(conn, row.id).await
}

#[tauri::command]
pub async fn collect_servicing_payment(id: i32, amount: Decimal) -> CmdResult<ServicingView> {
    crate::commands_auth::require_permission(db(), "servicing-collect").await?;
    collect_servicing_payment_in(db(), id, amount).await
}

pub(crate) async fn collect_servicing_payment_in<C: ConnectionTrait>(
    conn: &C,
    id: i32,
    amount: Decimal,
) -> CmdResult<ServicingView> {
    if amount <= Decimal::ZERO {
        return Err(CmdError::Validation("payment must be greater than zero".into()));
    }
    let job = servicing::Entity::find_by_id(id)
        .filter(servicing::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("servicing".into()))?;
    let outstanding = servicing_due(&job);
    if amount > outstanding {
        return Err(CmdError::Validation(format!(
            "that job has {outstanding} outstanding, not {amount}"
        )));
    }
    let paid = job.paid_amount + amount;
    let mut am: servicing::ActiveModel = job.into();
    am.paid_amount = Set(paid);
    am.updated_at = Set(crate::migration::now());
    am.update(conn).await?;
    get_servicing_in(conn, id).await
}

#[tauri::command]
pub async fn list_servicings(
    customer_id: Option<i32>,
    query: PageQuery,
) -> CmdResult<Page<ServicingSummary>> {
    crate::commands_auth::require_permission(db(), "servicing-list").await?;
    list_servicings_in(db(), customer_id, &query).await
}

pub async fn list_servicings_in<C: ConnectionTrait>(
    conn: &C,
    customer_id: Option<i32>,
    query: &PageQuery,
) -> CmdResult<Page<ServicingSummary>> {
    let mut q = servicing::Entity::find().filter(servicing::Column::DelStatus.eq(LIVE));
    if let Some(customer_id) = customer_id {
        q = q.filter(servicing::Column::CustomerId.eq(customer_id));
    }

    let total = q.clone().count(conn).await?;
    let rows = q
        .order_by_desc(servicing::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;

    let ids: Vec<i32> = rows.iter().map(|r| r.customer_id).collect();
    let names = ticket_customer_names(conn, &ids).await?;
    let views = rows
        .into_iter()
        .map(|row| {
            let due_amount = servicing_due(&row);
            ServicingSummary {
                id: row.id,
                customer_name: names.get(&row.customer_id).cloned(),
                product_name: row.product_name,
                servicing_charge: row.servicing_charge,
                paid_amount: row.paid_amount,
                due_amount,
                current_status: row.current_status,
                created_at: row.created_at,
            }
        })
        .collect();
    Ok(Page::new(views, total, query))
}

#[tauri::command]
pub async fn get_servicing(id: i32) -> CmdResult<ServicingView> {
    crate::commands_auth::require_permission(db(), "servicing-show").await?;
    get_servicing_in(db(), id).await
}

pub async fn get_servicing_in<C: ConnectionTrait>(conn: &C, id: i32) -> CmdResult<ServicingView> {
    let row = servicing::Entity::find_by_id(id)
        .filter(servicing::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("servicing".into()))?;
    let due = servicing_due(&row);
    let customer_name = customer::Entity::find_by_id(row.customer_id)
        .one(conn)
        .await?
        .map(|c| c.name);
    Ok(ServicingView { servicing: row, customer_name, due_amount: due })
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