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
use crate::entities::catalog::{brand, item, item_category, unit};
use crate::entities::catalog::item::ItemView;
use crate::entities::sales::stock_movement::MovementType;
use crate::entities::sales::{sale, sale_detail, sale_payment, stock_movement};
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
        let (item_name, available) = guard_line_sellable(&txn, line).await?;
        let discount = line.discount.unwrap_or(Decimal::ZERO);
        let gross = line.unit_price * line.quantity;

        lines.push(
            sale_detail::ActiveModel {
                sale_id: Set(sale_id),
                item_id: Set(line.item_id),
                // Snapshot: a later rename must not rewrite the receipt.
                item_name: Set(item_name),
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

    // Totals are only known once the lines are, so the header is written twice:
    // once to obtain the primary key, once with the real invoice number and totals.
    // Both statements are in the same transaction, so no reader ever sees the
    // half-filled row.
    let discount_total = line_discount_total + input.discount_total.unwrap_or(Decimal::ZERO);
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
        payments: None,
    })?;
    guard_discount_within_subtotal(discount_total, subtotal)?;

    let grand_total = subtotal - discount_total + tax_total;
    let paid = paid_total.unwrap_or(grand_total);

    let mut lines: Vec<sale_detail::Model> = Vec::with_capacity(stored.len());
    let mut stock_on_hand: Vec<ItemOnHand> = Vec::with_capacity(stored.len());
    let mut seen: HashMap<i32, usize> = HashMap::with_capacity(stored.len());

    for stored_line in &stored {
        let line = CheckoutLine {
            item_id: stored_line.item_id,
            quantity: stored_line.quantity,
            unit_price: stored_line.unit_price,
            discount: Some(stored_line.discount),
        };
        // The check that matters: the shelf is re-read now, not trusted from scan time,
        // because another till may have sold this stock while the draft waited. Also
        // rejects the sale if the item was soft-deleted in the meantime.
        let (_, available) = guard_line_sellable(&txn, &line).await?;
        let net = stored_line.unit_price * stored_line.quantity - stored_line.discount;

        let line_row = if net == stored_line.line_total {
            stored_line.clone()
        } else {
            // Persist the rebuilt figure, so the stored row matches the money actually
            // charged rather than the provisional one written at draft time.
            let mut am: sale_detail::ActiveModel = stored_line.clone().into();
            am.line_total = Set(net);
            am.update(&txn).await?
        };
        lines.push(line_row);

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