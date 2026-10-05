//! The cash book: income, expense, the owner's deposits and withdrawals, and the
//! account balances they all add up to.
//!
//! Same split as every other command module — the `#[tauri::command]` shell pulls
//! the process-wide connection that only exists inside the Tauri window, while the
//! logic takes one as an argument so it works against a throwaway database in tests.
//!
//! **Every balance here is derived, never stored.** What an account holds is the
//! signed sum over the rows that moved it, for the same reason stock is a ledger and
//! customer balances are derived: a stored figure is a read-modify-write, and two
//! concurrent takings both read it and one of them vanishes.
//!
//! Seven sources move money, and a balance reads all of them. Reading only the three
//! tables this module owns would report a drawer that had emptied itself during
//! trading.

use std::collections::{HashMap, HashSet};

use sea_orm::prelude::Decimal;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, TransactionTrait, TransactionSession,
};
use serde::{Deserialize, Serialize};

use crate::commands::{
    CmdError, CmdResult, LIVE, MONEY_SCALE, Page, PageQuery, contains_ci, like_term, text,
};
use crate::commands_auth::require_permission;
use crate::db::db;
use crate::entities::accounting::deposit_withdraw::DepositKind;
use crate::entities::accounting::expense_recurring::Rotation;
use crate::entities::accounting::{
    deposit_withdraw, expense, expense_category, expense_recurring, income, income_category,
};
use crate::entities::auth::users;
use crate::entities::sales::{sale, sale_payment};
use crate::entities::trade::{
    customer_receive, payment_method, purchase, purchase_return, supplier, supplier_payment,
};

const SALE_STATUS_COMPLETED: &str = "Completed";
const DELETED: &str = "Deleted";

// ---------------------------------------------------------------------------
// Shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryInput {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryView {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryInput {
    pub category_id: i32,
    /// The account the money arrived in, or left. Required: an entry that does not
    /// say which account is money that is in the books and nowhere else.
    pub payment_method_id: i32,
    /// Always positive. Which way it moved is what the row is.
    pub amount: Decimal,
    pub occurred_at: chrono::NaiveDate,
    #[serde(default)]
    pub employee_id: Option<i32>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    pub id: i32,
    pub reference_no: String,
    pub category_id: i32,
    pub category_name: String,
    pub payment_method_id: i32,
    pub payment_method_name: String,
    pub employee_id: Option<i32>,
    pub amount: Decimal,
    pub occurred_at: chrono::NaiveDate,
    pub note: Option<String>,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositWithdrawInput {
    pub kind: DepositKind,
    pub payment_method_id: i32,
    pub amount: Decimal,
    pub occurred_at: chrono::NaiveDate,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositWithdrawView {
    pub id: i32,
    pub reference_no: String,
    /// `Deposit` or `Withdraw`, closed so a report groups without matching strings.
    pub kind: String,
    pub payment_method_id: i32,
    pub payment_method_name: String,
    pub amount: Decimal,
    pub occurred_at: chrono::NaiveDate,
    pub note: Option<String>,
    pub created_at: chrono::NaiveDateTime,
}

/// One movement as a reader sees it. `signed` is already signed, so summing the column
/// answers "how much" without asking which way anything went.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CashBookLine {
    pub reference_no: String,
    pub kind: String,
    pub category: Option<String>,
    pub payment_method_id: i32,
    pub payment_method_name: String,
    pub amount: Decimal,
    pub signed: Decimal,
    pub occurred_at: chrono::NaiveDate,
    pub note: Option<String>,
}

/// One account's position. Derived as the sum of its lines, never a column.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountBalance {
    pub payment_method_id: i32,
    pub payment_method_name: String,
    /// `Cash` moves through the drawer, so the drawer figure is a subset of this.
    pub kind: String,
    pub balance: Decimal,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn required_name(raw: &str) -> CmdResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CmdError::Validation("name is required".into()));
    }
    Ok(trimmed.to_owned())
}

fn positive(amount: Decimal, label: &str) -> CmdResult<Decimal> {
    let rounded = amount.round_dp(MONEY_SCALE);
    if rounded <= Decimal::ZERO {
        return Err(CmdError::Validation(format!("{label} must be more than zero")));
    }
    Ok(rounded)
}

async fn account_is_live<C: ConnectionTrait>(conn: &C, id: i32) -> CmdResult<()> {
    payment_method::Entity::find_by_id(id)
        .filter(payment_method::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound(format!("payment method {id}")))?;
    Ok(())
}

async fn account_names<C: ConnectionTrait>(conn: &C, ids: &[i32]) -> CmdResult<HashMap<i32, String>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(payment_method::Entity::find()
        .filter(payment_method::Column::Id.is_in(ids.to_vec()))
        .all(conn)
        .await?
        .into_iter()
        .map(|m| (m.id, m.name))
        .collect())
}

/// Inclusive day bounds for a calendar date, because `paid_at` and `created_at` are
/// timestamps and the caller asked about a day.
fn day_start(day: chrono::NaiveDate) -> chrono::NaiveDateTime {
    day.and_hms_opt(0, 0, 0).expect("midnight is a valid time")
}

fn day_end(day: chrono::NaiveDate) -> chrono::NaiveDateTime {
    day.and_hms_opt(23, 59, 59).expect("the last second is a valid time")
}

fn account_name(names: &HashMap<i32, String>, id: i32) -> String {
    names.get(&id).cloned().unwrap_or_else(|| format!("#{id}"))
}

// ---------------------------------------------------------------------------
// Income categories
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_income_categories(query: PageQuery) -> CmdResult<Page<CategoryView>> {
    require_permission(db(), "income-list").await?;
    list_income_categories_in(db(), query).await
}

pub(crate) async fn list_income_categories_in<C: ConnectionTrait>(conn: &C, query: PageQuery) -> CmdResult<Page<CategoryView>> {

    let mut q = income_category::Entity::find()
        .filter(income_category::Column::DelStatus.eq(LIVE));
    if let Some(term) = query.term() {
        q = q.filter(contains_ci(income_category::Column::Name, &like_term(&term)));
    }
    let total = q.clone().count(conn).await?;
    let rows = q
        .order_by_asc(income_category::Column::Name)
        .order_by_asc(income_category::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;
    let views = rows
        .into_iter()
        .map(|r| CategoryView {
            id: r.id,
            name: r.name,
            description: r.description,
            created_at: r.created_at,
        })
        .collect();
    Ok(Page::new(views, total, &query))
}

#[tauri::command]
pub async fn create_income_category(input: CategoryInput) -> CmdResult<CategoryView> {
    require_permission(db(), "income-create").await?;
    create_income_category_in(db(), input).await
}

pub(crate) async fn create_income_category_in<C: ConnectionTrait>(conn: &C, input: CategoryInput) -> CmdResult<CategoryView> {

    let row = income_category::ActiveModel {
        name: Set(required_name(&input.name)?),
        description: Set(text(input.description)),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(crate::migration::now()),
        ..Default::default()
    }
    .insert(conn)
    .await?;
    Ok(CategoryView {
        id: row.id,
        name: row.name,
        description: row.description,
        created_at: row.created_at,
    })
}

#[tauri::command]
pub async fn delete_income_category(id: i32) -> CmdResult<()> {
    require_permission(db(), "income-create").await?;
    delete_income_category_in(db(), id).await
}

pub(crate) async fn delete_income_category_in<C: ConnectionTrait>(
    conn: &C,
    id: i32,
) -> CmdResult<()> {
    let used = income::Entity::find()
        .filter(income::Column::IncomeCategoryId.eq(id))
        .count(conn)
        .await?;
    if used > 0 {
        return Err(CmdError::Conflict(format!(
            "{used} income entr{} already use this category",
            if used == 1 { "y" } else { "ies" }
        )));
    }
    // A soft delete, like every other master row: an income names its category, and a
    // category that vanished under a posted entry would break the audit trail.
    let mut row: income_category::ActiveModel = income_category::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("income category".into()))?
        .into();
    row.del_status = Set(DELETED.to_owned());
    row.update(conn).await?;
    Ok(())
}

#[tauri::command]
pub async fn delete_expense_category(id: i32) -> CmdResult<()> {
    require_permission(db(), "expense-create").await?;
    let used = expense::Entity::find()
        .filter(expense::Column::ExpenseCategoryId.eq(id))
        .count(db())
        .await?;
    if used > 0 {
        return Err(CmdError::Conflict(format!(
            "{used} expense entr{} already use this category",
            if used == 1 { "y" } else { "ies" }
        )));
    }
    let mut row: expense_category::ActiveModel = expense_category::Entity::find_by_id(id)
        .one(db())
        .await?
        .ok_or_else(|| CmdError::NotFound("expense category".into()))?
        .into();
    row.del_status = Set(DELETED.to_owned());
    row.update(db()).await?;
    Ok(())
}

#[tauri::command]
pub async fn list_expense_categories(query: PageQuery) -> CmdResult<Page<CategoryView>> {
    require_permission(db(), "expense-list").await?;
    list_expense_categories_in(db(), query).await
}

pub(crate) async fn list_expense_categories_in<C: ConnectionTrait>(conn: &C, query: PageQuery) -> CmdResult<Page<CategoryView>> {

    let mut q = expense_category::Entity::find()
        .filter(expense_category::Column::DelStatus.eq(LIVE));
    if let Some(term) = query.term() {
        q = q.filter(contains_ci(expense_category::Column::Name, &like_term(&term)));
    }
    let total = q.clone().count(conn).await?;
    let rows = q
        .order_by_asc(expense_category::Column::Name)
        .order_by_asc(expense_category::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;
    let views = rows
        .into_iter()
        .map(|r| CategoryView {
            id: r.id,
            name: r.name,
            description: r.description,
            created_at: r.created_at,
        })
        .collect();
    Ok(Page::new(views, total, &query))
}

#[tauri::command]
pub async fn create_expense_category(input: CategoryInput) -> CmdResult<CategoryView> {
    require_permission(db(), "expense-create").await?;
    create_expense_category_in(db(), input).await
}

pub(crate) async fn create_expense_category_in<C: ConnectionTrait>(conn: &C, input: CategoryInput) -> CmdResult<CategoryView> {

    let row = expense_category::ActiveModel {
        name: Set(required_name(&input.name)?),
        description: Set(text(input.description)),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(crate::migration::now()),
        ..Default::default()
    }
    .insert(conn)
    .await?;
    Ok(CategoryView {
        id: row.id,
        name: row.name,
        description: row.description,
        created_at: row.created_at,
    })
}

// ---------------------------------------------------------------------------
// Incomes and expenses
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_incomes(query: PageQuery) -> CmdResult<Page<EntryView>> {
    require_permission(db(), "income-list").await?;
    list_incomes_in(db(), query).await
}

pub(crate) async fn list_incomes_in<C: ConnectionTrait>(conn: &C, query: PageQuery) -> CmdResult<Page<EntryView>> {

    let mut q = income::Entity::find().filter(income::Column::DelStatus.eq(LIVE));
    if let Some(term) = query.term() {
        q = q.filter(contains_ci(income::Column::ReferenceNo, &like_term(&term)));
    }
    let total = q.clone().count(conn).await?;
    let rows = q
        .order_by_desc(income::Column::OccurredAt)
        .order_by_desc(income::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;

    let accounts = account_names(conn, &rows.iter().map(|r| r.payment_method_id).collect::<Vec<_>>()).await?;
    let category_ids: Vec<i32> = rows.iter().map(|r| r.income_category_id).collect();
    let categories: HashMap<i32, String> = income_category::Entity::find()
        .filter(income_category::Column::Id.is_in(category_ids))
        .all(conn)
        .await?
        .into_iter()
        .map(|c| (c.id, c.name))
        .collect();

    let views = rows
        .into_iter()
        .map(|r| EntryView {
            category_name: categories.get(&r.income_category_id).cloned().unwrap_or_default(),
            payment_method_name: account_name(&accounts, r.payment_method_id),
            id: r.id,
            reference_no: r.reference_no,
            category_id: r.income_category_id,
            payment_method_id: r.payment_method_id,
            employee_id: r.employee_id,
            amount: r.amount,
            occurred_at: r.occurred_at,
            note: r.note,
            created_at: r.created_at,
        })
        .collect();
    Ok(Page::new(views, total, &query))
}

#[tauri::command]
pub async fn create_income(input: EntryInput) -> CmdResult<EntryView> {
    require_permission(db(), "income-create").await?;
    create_income_in(db(), input).await
}

pub(crate) async fn create_income_in<C: ConnectionTrait + TransactionTrait>(conn: &C, input: EntryInput) -> CmdResult<EntryView> {

    let amount = positive(input.amount, "amount")?;

    // Guards inside the transaction, so the category and the account cannot be retired
    // between the check and the insert. SQLite is opened with `max_connections(1)`, so
    // anything reading the pool while the transaction holds the only connection waits
    // for itself.
    let txn = conn.begin().await?;
    account_is_live(&txn, input.payment_method_id).await?;
    let category = income_category::Entity::find_by_id(input.category_id)
        .filter(income_category::Column::DelStatus.eq(LIVE))
        .one(&txn)
        .await?
        .ok_or_else(|| CmdError::NotFound("income category".into()))?;
    if let Some(employee) = input.employee_id {
        users::Entity::find_by_id(employee)
            .filter(users::Column::DelStatus.eq(LIVE))
            .one(&txn)
            .await?
            .ok_or_else(|| CmdError::NotFound(format!("user {employee}")))?;
    }

    let now = crate::migration::now();
    let inserted = income::ActiveModel {
        reference_no: Set(String::new()),
        income_category_id: Set(input.category_id),
        payment_method_id: Set(input.payment_method_id),
        employee_id: Set(input.employee_id),
        amount: Set(amount),
        occurred_at: Set(input.occurred_at),
        note: Set(text(input.note)),
        created_by: Set(crate::auth::current_user_id()),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;

    // From the primary key, not MAX(reference_no) + 1: two entries committed at once
    // read the same MAX.
    let reference_no = format!("INC-{:06}", inserted.id);
    let mut header: income::ActiveModel = inserted.into();
    header.reference_no = Set(reference_no.clone());
    let row = header.update(&txn).await?;
    txn.commit().await?;

    Ok(EntryView {
        id: row.id,
        reference_no,
        category_id: row.income_category_id,
        category_name: category.name,
        payment_method_id: row.payment_method_id,
        payment_method_name: String::new(),
        employee_id: row.employee_id,
        amount: row.amount,
        occurred_at: row.occurred_at,
        note: row.note,
        created_at: row.created_at,
    })
}

#[tauri::command]
pub async fn list_expenses(query: PageQuery) -> CmdResult<Page<EntryView>> {
    require_permission(db(), "expense-list").await?;
    list_expenses_in(db(), query).await
}

pub(crate) async fn list_expenses_in<C: ConnectionTrait>(conn: &C, query: PageQuery) -> CmdResult<Page<EntryView>> {

    let mut q = expense::Entity::find().filter(expense::Column::DelStatus.eq(LIVE));
    if let Some(term) = query.term() {
        q = q.filter(contains_ci(expense::Column::ReferenceNo, &like_term(&term)));
    }
    let total = q.clone().count(conn).await?;
    let rows = q
        .order_by_desc(expense::Column::OccurredAt)
        .order_by_desc(expense::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;

    let accounts = account_names(conn, &rows.iter().map(|r| r.payment_method_id).collect::<Vec<_>>()).await?;
    let category_ids: Vec<i32> = rows.iter().map(|r| r.expense_category_id).collect();
    let categories: HashMap<i32, String> = expense_category::Entity::find()
        .filter(expense_category::Column::Id.is_in(category_ids))
        .all(conn)
        .await?
        .into_iter()
        .map(|c| (c.id, c.name))
        .collect();

    let views = rows
        .into_iter()
        .map(|r| EntryView {
            category_name: categories.get(&r.expense_category_id).cloned().unwrap_or_default(),
            payment_method_name: account_name(&accounts, r.payment_method_id),
            id: r.id,
            reference_no: r.reference_no,
            category_id: r.expense_category_id,
            payment_method_id: r.payment_method_id,
            employee_id: r.employee_id,
            amount: r.amount,
            occurred_at: r.occurred_at,
            note: r.note,
            created_at: r.created_at,
        })
        .collect();
    Ok(Page::new(views, total, &query))
}

#[tauri::command]
pub async fn create_expense(input: EntryInput) -> CmdResult<EntryView> {
    require_permission(db(), "expense-create").await?;
    create_expense_in(db(), input).await
}

pub(crate) async fn create_expense_in<C: ConnectionTrait + TransactionTrait>(conn: &C, input: EntryInput) -> CmdResult<EntryView> {

    let amount = positive(input.amount, "amount")?;

    let txn = conn.begin().await?;
    account_is_live(&txn, input.payment_method_id).await?;
    let category = expense_category::Entity::find_by_id(input.category_id)
        .filter(expense_category::Column::DelStatus.eq(LIVE))
        .one(&txn)
        .await?
        .ok_or_else(|| CmdError::NotFound("expense category".into()))?;
    if let Some(employee) = input.employee_id {
        users::Entity::find_by_id(employee)
            .filter(users::Column::DelStatus.eq(LIVE))
            .one(&txn)
            .await?
            .ok_or_else(|| CmdError::NotFound(format!("user {employee}")))?;
    }

    let now = crate::migration::now();
    let inserted = expense::ActiveModel {
        reference_no: Set(String::new()),
        expense_category_id: Set(input.category_id),
        payment_method_id: Set(input.payment_method_id),
        employee_id: Set(input.employee_id),
        amount: Set(amount),
        occurred_at: Set(input.occurred_at),
        note: Set(text(input.note)),
        created_by: Set(crate::auth::current_user_id()),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;

    let reference_no = format!("EXP-{:06}", inserted.id);
    let mut header: expense::ActiveModel = inserted.into();
    header.reference_no = Set(reference_no.clone());
    let row = header.update(&txn).await?;
    txn.commit().await?;

    Ok(EntryView {
        id: row.id,
        reference_no,
        category_id: row.expense_category_id,
        category_name: category.name,
        payment_method_id: row.payment_method_id,
        payment_method_name: String::new(),
        employee_id: row.employee_id,
        amount: row.amount,
        occurred_at: row.occurred_at,
        note: row.note,
        created_at: row.created_at,
    })
}

// ---------------------------------------------------------------------------
// Deposits and withdrawals
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn create_deposit_withdraw(input: DepositWithdrawInput) -> CmdResult<DepositWithdrawView> {
    require_permission(db(), "deposit-withdraw-create").await?;
    create_deposit_withdraw_in(db(), input).await
}

pub(crate) async fn create_deposit_withdraw_in<C: ConnectionTrait + TransactionTrait>(conn: &C, input: DepositWithdrawInput) -> CmdResult<DepositWithdrawView> {

    let amount = positive(input.amount, "amount")?;

    let txn = conn.begin().await?;
    account_is_live(&txn, input.payment_method_id).await?;

    let now = crate::migration::now();
    let inserted = deposit_withdraw::ActiveModel {
        reference_no: Set(String::new()),
        payment_method_id: Set(input.payment_method_id),
        kind: Set(input.kind.as_str().to_owned()),
        amount: Set(amount),
        occurred_at: Set(input.occurred_at),
        note: Set(text(input.note)),
        created_by: Set(crate::auth::current_user_id()),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;

    let reference_no = format!("DW-{:06}", inserted.id);
    let mut header: deposit_withdraw::ActiveModel = inserted.into();
    header.reference_no = Set(reference_no.clone());
    let row = header.update(&txn).await?;
    txn.commit().await?;

    Ok(DepositWithdrawView {
        id: row.id,
        reference_no,
        kind: row.kind,
        payment_method_id: row.payment_method_id,
        payment_method_name: String::new(),
        amount: row.amount,
        occurred_at: row.occurred_at,
        note: row.note,
        created_at: row.created_at,
    })
}

#[tauri::command]
pub async fn list_deposit_withdraws(query: PageQuery) -> CmdResult<Page<DepositWithdrawView>> {
    require_permission(db(), "deposit-withdraw-list").await?;
    list_deposit_withdraws_in(db(), query).await
}

pub(crate) async fn list_deposit_withdraws_in<C: ConnectionTrait>(conn: &C, query: PageQuery) -> CmdResult<Page<DepositWithdrawView>> {

    let mut q =
        deposit_withdraw::Entity::find().filter(deposit_withdraw::Column::DelStatus.eq(LIVE));
    if let Some(term) = query.term() {
        q = q.filter(contains_ci(deposit_withdraw::Column::ReferenceNo, &like_term(&term)));
    }
    let total = q.clone().count(conn).await?;
    let rows = q
        .order_by_desc(deposit_withdraw::Column::OccurredAt)
        .order_by_desc(deposit_withdraw::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;

    let accounts = account_names(conn, &rows.iter().map(|r| r.payment_method_id).collect::<Vec<_>>()).await?;
    let views = rows
        .into_iter()
        .map(|r| DepositWithdrawView {
            payment_method_name: account_name(&accounts, r.payment_method_id),
            id: r.id,
            reference_no: r.reference_no,
            kind: r.kind,
            payment_method_id: r.payment_method_id,
            amount: r.amount,
            occurred_at: r.occurred_at,
            note: r.note,
            created_at: r.created_at,
        })
        .collect();
    Ok(Page::new(views, total, &query))
}

// ---------------------------------------------------------------------------
// The cash book
// ---------------------------------------------------------------------------

/// Every movement, oldest first, with the sign already applied.
///
/// Seven sources, and all of them are read: a single-tender sale, a split sale's
/// tender rows, a customer receipt, a supplier payment, income, expense, and the
/// owner's own deposits and withdrawals. Reading fewer would balance a drawer that had
/// emptied itself during trading.
async fn lines_in<C: ConnectionTrait>(
    conn: &C,
    account: Option<i32>,
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
) -> CmdResult<Vec<CashBookLine>> {
    let mut out: Vec<CashBookLine> = Vec::new();
    let mut account_ids: Vec<i32> = Vec::new();

    fn push(
        out: &mut Vec<CashBookLine>,
        seen: &mut Vec<i32>,
        only: Option<i32>,
        line: CashBookLine,
    ) {
        if only.is_none_or(|a| a == line.payment_method_id) {
            seen.push(line.payment_method_id);
            out.push(line);
        }
    }

    // Income.
    let mut q = income::Entity::find().filter(income::Column::DelStatus.eq(LIVE));
    if let Some(d) = from {
        q = q.filter(income::Column::OccurredAt.gte(d));
    }
    if let Some(d) = to {
        q = q.filter(income::Column::OccurredAt.lte(d));
    }
    let income_rows = q.all(conn).await?;
    let income_categories: HashMap<i32, String> = income_category::Entity::find()
        .all(conn)
        .await?
        .into_iter()
        .map(|c| (c.id, c.name))
        .collect();
    for r in income_rows {
        push(
            &mut out,
            &mut account_ids,
            account,
            CashBookLine {
                reference_no: r.reference_no,
                kind: "Income".into(),
                category: income_categories.get(&r.income_category_id).cloned(),
                payment_method_id: r.payment_method_id,
                payment_method_name: String::new(),
                amount: r.amount,
                signed: r.amount,
                occurred_at: r.occurred_at,
                note: r.note,
            },
        );
    }

    // Expense.
    let mut q = expense::Entity::find().filter(expense::Column::DelStatus.eq(LIVE));
    if let Some(d) = from {
        q = q.filter(expense::Column::OccurredAt.gte(d));
    }
    if let Some(d) = to {
        q = q.filter(expense::Column::OccurredAt.lte(d));
    }
    let expense_rows = q.all(conn).await?;
    let expense_categories: HashMap<i32, String> = expense_category::Entity::find()
        .all(conn)
        .await?
        .into_iter()
        .map(|c| (c.id, c.name))
        .collect();
    for r in expense_rows {
        push(
            &mut out,
            &mut account_ids,
            account,
            CashBookLine {
                reference_no: r.reference_no,
                kind: "Expense".into(),
                category: expense_categories.get(&r.expense_category_id).cloned(),
                payment_method_id: r.payment_method_id,
                payment_method_name: String::new(),
                amount: r.amount,
                signed: -r.amount,
                occurred_at: r.occurred_at,
                note: r.note,
            },
        );
    }

    // Deposits and withdrawals.
    let mut q =
        deposit_withdraw::Entity::find().filter(deposit_withdraw::Column::DelStatus.eq(LIVE));
    if let Some(d) = from {
        q = q.filter(deposit_withdraw::Column::OccurredAt.gte(d));
    }
    if let Some(d) = to {
        q = q.filter(deposit_withdraw::Column::OccurredAt.lte(d));
    }
    for r in q.all(conn).await? {
        // The row's own kind, never an assumed one. Hardcoding `Deposit` here read
        // every withdrawal as an inflow, so a float top-up and cash taken to the bank
        // both added — the balance said 70 where the drawer held 30.
        let sign = DepositKind::parse(&r.kind)?.sign();
        push(
            &mut out,
            &mut account_ids,
            account,
            CashBookLine {
                reference_no: r.reference_no,
                kind: r.kind.clone(),
                category: None,
                payment_method_id: r.payment_method_id,
                payment_method_name: String::new(),
                amount: r.amount,
                signed: sign * r.amount,
                occurred_at: r.occurred_at,
                note: r.note,
            },
        );
    }

    // Customer receipts.
    let mut q = customer_receive::Entity::find();
    if let Some(d) = from {
        q = q.filter(customer_receive::Column::PaidAt.gte(day_start(d)));
    }
    if let Some(d) = to {
        q = q.filter(customer_receive::Column::PaidAt.lte(day_end(d)));
    }
    for r in q.all(conn).await? {
        let Some(id) = r.payment_method_id else { continue };
        push(
            &mut out,
            &mut account_ids,
            account,
            CashBookLine {
                reference_no: format!("RCP-{:06}", r.id),
                kind: "Receipt".into(),
                category: None,
                payment_method_id: id,
                payment_method_name: String::new(),
                amount: r.amount,
                signed: r.amount,
                occurred_at: r.paid_at.date(),
                note: r.reference,
            },
        );
    }

    // Supplier payments, money leaving.
    let mut q = supplier_payment::Entity::find();
    if let Some(d) = from {
        q = q.filter(supplier_payment::Column::PaidAt.gte(day_start(d)));
    }
    if let Some(d) = to {
        q = q.filter(supplier_payment::Column::PaidAt.lte(day_end(d)));
    }
    for r in q.all(conn).await? {
        let Some(id) = r.payment_method_id else { continue };
        push(
            &mut out,
            &mut account_ids,
            account,
            CashBookLine {
                reference_no: format!("PAY-{:06}", r.id),
                kind: "Supplier payment".into(),
                category: None,
                payment_method_id: id,
                payment_method_name: String::new(),
                amount: r.amount,
                signed: -r.amount,
                occurred_at: r.paid_at.date(),
                note: r.reference,
            },
        );
    }

    // Sales. A split sale's tenders are rows; a single-tender sale is the header, and
    // the two are disjoint by construction — a sale has one or the other — so a tender
    // is never counted twice. This is the same union the register close already reads.
    let mut completed = sale::Entity::find()
        .filter(sale::Column::Status.eq(SALE_STATUS_COMPLETED));
    if let Some(d) = from {
        completed = completed.filter(sale::Column::CreatedAt.gte(day_start(d)));
    }
    if let Some(d) = to {
        completed = completed.filter(sale::Column::CreatedAt.lte(day_end(d)));
    }
    let sales: Vec<(i32, String, chrono::NaiveDateTime, Decimal, String, Option<i32>)> =
        completed
            .select_only()
            .column(sale::Column::Id)
            .column(sale::Column::InvoiceNo)
            .column(sale::Column::CreatedAt)
            .column(sale::Column::PaidTotal)
            .column(sale::Column::PaymentMethod)
            .column(sale::Column::PaymentMethodId)
            .into_tuple()
            .all(conn)
            .await?;

    let sale_ids: Vec<i32> = sales.iter().map(|(id, ..)| *id).collect();
    let mut tendered: HashSet<i32> = HashSet::new();

    if !sale_ids.is_empty() {
        let tenders: Vec<(i32, String, Decimal, Option<i32>)> = sale_payment::Entity::find()
            .select_only()
            .column(sale_payment::Column::SaleId)
            .column(sale_payment::Column::Method)
            .column(sale_payment::Column::Amount)
            .column(sale_payment::Column::PaymentMethodId)
            .filter(sale_payment::Column::SaleId.is_in(sale_ids.clone()))
            .into_tuple()
            .all(conn)
            .await?;
        let headers: HashMap<i32, (String, chrono::NaiveDateTime)> = sales
            .iter()
            .map(|(id, inv, at, ..)| (*id, (inv.clone(), *at)))
            .collect();
        for (sale_id, method, amount, method_id) in tenders {
            tendered.insert(sale_id);
            let (invoice, when) = headers
                .get(&sale_id)
                .cloned()
                .unwrap_or_else(|| (String::new(), chrono::NaiveDateTime::default()));
            // A null id is a tender whose text matched no live account, or a
            // redemption. Neither is money entering an account.
            let Some(id) = method_id else { continue };
            push(
                &mut out,
                &mut account_ids,
                account,
                CashBookLine {
                    reference_no: invoice.clone(),
                    kind: format!("Sale ({method})"),
                    category: None,
                    payment_method_id: id,
                    payment_method_name: String::new(),
                    amount,
                    signed: amount,
                    occurred_at: when.date(),
                    note: None,
                },
            );
        }
    }

    for (id, invoice, when, paid, method, method_id) in sales {
        if tendered.contains(&id) {
            continue;
        }
        let Some(account_id) = method_id else { continue };
        push(
            &mut out,
            &mut account_ids,
            account,
            CashBookLine {
                reference_no: invoice,
                kind: format!("Sale ({method})"),
                category: None,
                payment_method_id: account_id,
                payment_method_name: String::new(),
                amount: paid,
                signed: paid,
                occurred_at: when.date(),
                note: None,
            },
        );
    }

    let names = account_names(conn, &account_ids).await?;
    for line in &mut out {
        line.payment_method_name = account_name(&names, line.payment_method_id);
    }
    out.sort_by(|a, b| a.occurred_at.cmp(&b.occurred_at).then(a.reference_no.cmp(&b.reference_no)));
    Ok(out)
}

#[tauri::command]
pub async fn account_balances() -> CmdResult<Vec<AccountBalance>> {
    require_permission(db(), "accounting-balance").await?;
    account_balances_in(db()).await
}

pub(crate) async fn account_balances_in<C: ConnectionTrait>(conn: &C) -> CmdResult<Vec<AccountBalance>> {
    let lines = lines_in(conn, None, None, None).await?;
    let mut totals: HashMap<i32, Decimal> = HashMap::new();
    for line in &lines {
        *totals.entry(line.payment_method_id).or_insert(Decimal::ZERO) += line.signed;
    }

    let methods = payment_method::Entity::find()
        .filter(payment_method::Column::DelStatus.eq(LIVE))
        .all(conn)
        .await?;

    // Every live account appears, including one that has never moved — an operator
    // adding a tender wants to see its balance as zero, not as absent.
    let mut out: Vec<AccountBalance> = methods
        .into_iter()
        .map(|m| AccountBalance {
            payment_method_id: m.id,
            payment_method_name: m.name,
            kind: m.kind,
            balance: totals.remove(&m.id).unwrap_or(Decimal::ZERO).round_dp(MONEY_SCALE),
        })
        .collect();
    out.sort_by(|a, b| a.payment_method_name.cmp(&b.payment_method_name));
    Ok(out)
}

#[tauri::command]
pub async fn account_statement(
    payment_method_id: i32,
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
) -> CmdResult<Vec<CashBookLine>> {
    require_permission(db(), "accounting-balance").await?;
    account_statement_in(db(), payment_method_id, from, to).await
}

pub(crate) async fn account_statement_in<C: ConnectionTrait>(
    conn: &C,
    payment_method_id: i32,
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
) -> CmdResult<Vec<CashBookLine>> {
    account_is_live(conn, payment_method_id).await?;
    lines_in(conn, Some(payment_method_id), from, to).await
}

#[tauri::command]
pub async fn cash_book_history(
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
) -> CmdResult<Vec<CashBookLine>> {
    require_permission(db(), "accounting-balance").await?;
    cash_book_history_in(db(), from, to).await
}

pub(crate) async fn cash_book_history_in<C: ConnectionTrait>(
    conn: &C,
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
) -> CmdResult<Vec<CashBookLine>> {
    lines_in(conn, None, from, to).await
}

// ---------------------------------------------------------------------------
// Recurring expenses — a schedule proposes, the expense posts
// ---------------------------------------------------------------------------

/// Dates a rotation arithmetic gets wrong, in the order they bite.
fn rotation_of(raw: &str) -> CmdResult<Rotation> {
    match raw {
        "Daily" => Ok(Rotation::Daily),
        "Weekly" => Ok(Rotation::Weekly),
        "BiWeekly" => Ok(Rotation::BiWeekly),
        "Monthly" => Ok(Rotation::Monthly),
        "Quarterly" => Ok(Rotation::Quarterly),
        "Yearly" => Ok(Rotation::Yearly),
        other => Err(CmdError::Validation(format!(
            "{other:?} is not a rotation — daily, weekly, bi-weekly, monthly, quarterly or yearly"
        ))),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurringInput {
    pub expense_category_id: i32,
    pub name: String,
    pub amount: Decimal,
    pub payment_method_id: i32,
    pub rotation: String,
    pub starts_on: chrono::NaiveDate,
    #[serde(default)]
    pub ends_on: Option<chrono::NaiveDate>,
    #[serde(default)]
    pub note: Option<String>,
    /// Post the first entry now rather than waiting for the due date.
    #[serde(default)]
    pub post_now: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurringView {
    pub id: i32,
    pub expense_category_id: i32,
    pub expense_category_name: String,
    pub name: String,
    pub amount: Decimal,
    pub payment_method_id: i32,
    pub payment_method_name: String,
    pub rotation: String,
    pub starts_on: chrono::NaiveDate,
    pub next_due_on: Option<chrono::NaiveDate>,
    pub ends_on: Option<chrono::NaiveDate>,
    pub note: Option<String>,
    /// How many times it has actually posted, so a screen can show a schedule at work.
    pub posted_count: u64,
    pub created_at: chrono::NaiveDateTime,
}

/// What one posting run did, so an operator can see what was written rather than
/// guessing from a date.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostingRun {
    pub posted: u64,
    /// Schedule names that were due and could not be written. A partial run is not a
    /// silent one — an expense that failed must be visible, not rolled back into a
    /// count that looks smaller.
    pub failed: Vec<String>,
}

#[tauri::command]
pub async fn list_recurring_expenses(query: PageQuery) -> CmdResult<Page<RecurringView>> {
    require_permission(db(), "recurring-expense-list").await?;
    list_recurring_expenses_in(db(), query).await
}

pub(crate) async fn list_recurring_expenses_in<C: ConnectionTrait>(
    conn: &C,
    query: PageQuery,
) -> CmdResult<Page<RecurringView>> {
    let mut q = expense_recurring::Entity::find()
        .filter(expense_recurring::Column::DelStatus.eq(LIVE));
    if let Some(term) = query.term() {
        let like = like_term(&term);
        q = q.filter(
            sea_orm::sea_query::Condition::any()
                .add(contains_ci(expense_recurring::Column::Name, &like))
                .add(contains_ci(expense_recurring::Column::Rotation, &like)),
        );
    }
    let total = q.clone().count(conn).await?;
    let rows = q
        .order_by_asc(expense_recurring::Column::Name)
        .order_by_asc(expense_recurring::Column::Id)
        .offset(query.offset())
        .limit(query.per_page())
        .all(conn)
        .await?;

    let accounts = account_names(conn, &rows.iter().map(|r| r.payment_method_id).collect::<Vec<_>>()).await?;
    let category_ids: Vec<i32> = rows.iter().map(|r| r.expense_category_id).collect();
    let categories: HashMap<i32, String> = expense_category::Entity::find()
        .filter(expense_category::Column::Id.is_in(category_ids))
        .all(conn)
        .await?
        .into_iter()
        .map(|c| (c.id, c.name))
        .collect();

    // One grouped count rather than one query per row.
    let schedule_ids: Vec<i32> = rows.iter().map(|r| r.id).collect();
    let mut posted: HashMap<i32, i64> = HashMap::new();
    if !schedule_ids.is_empty() {
        let counts: Vec<(Option<i32>, i64)> = expense::Entity::find()
            .select_only()
            .column(expense::Column::RecurringExpenseId)
            .column_as(expense::Column::Id.count(), "total")
            .filter(expense::Column::RecurringExpenseId.is_in(schedule_ids))
            .group_by(expense::Column::RecurringExpenseId)
            .into_tuple()
            .all(conn)
            .await?;
        for (id, total) in counts {
            if let Some(id) = id {
                posted.insert(id, total);
            }
        }
    }

    let views = rows
        .into_iter()
        .map(|r| RecurringView {
            expense_category_name: categories.get(&r.expense_category_id).cloned().unwrap_or_default(),
            payment_method_name: account_name(&accounts, r.payment_method_id),
            posted_count: posted.get(&r.id).copied().unwrap_or(0) as u64,
            id: r.id,
            expense_category_id: r.expense_category_id,
            name: r.name,
            amount: r.amount,
            payment_method_id: r.payment_method_id,
            rotation: r.rotation,
            starts_on: r.starts_on,
            next_due_on: r.next_due_on,
            ends_on: r.ends_on,
            note: r.note,
            created_at: r.created_at,
        })
        .collect();
    Ok(Page::new(views, total, &query))
}

/// Writes the expense rows a run is owed, without moving any schedule's clock. Kept
/// separate from the advance so a test can post a schedule without a date dependency.
async fn post_schedule_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    schedule: &expense_recurring::Model,
    dates: &[chrono::NaiveDate],
) -> CmdResult<Vec<expense::Model>> {
    let now = crate::migration::now();
    let mut written = Vec::with_capacity(dates.len());
    for occurred_at in dates {
        let inserted = expense::ActiveModel {
            reference_no: Set(String::new()),
            expense_category_id: Set(schedule.expense_category_id),
            payment_method_id: Set(schedule.payment_method_id),
            amount: Set(schedule.amount),
            occurred_at: Set(*occurred_at),
            note: Set(text(schedule.note.clone())),
            recurring_expense_id: Set(Some(schedule.id)),
            created_by: Set(crate::auth::current_user_id()),
            del_status: Set(LIVE.to_owned()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(conn)
        .await?;

        let reference_no = format!("EXP-{:06}", inserted.id);
        let mut header: expense::ActiveModel = inserted.into();
        header.reference_no = Set(reference_no);
        written.push(header.update(conn).await?);
    }
    Ok(written)
}

#[tauri::command]
pub async fn create_recurring_expense(input: RecurringInput) -> CmdResult<RecurringView> {
    require_permission(db(), "recurring-expense-create").await?;
    create_recurring_expense_in(db(), input).await
}

pub(crate) async fn create_recurring_expense_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    input: RecurringInput,
) -> CmdResult<RecurringView> {
    let amount = positive(input.amount, "amount")?;
    let rotation = rotation_of(input.rotation.trim())?;
    if let Some(ends_on) = input.ends_on {
        if ends_on < input.starts_on {
            return Err(CmdError::Validation(
                "a schedule cannot end before it starts".into(),
            ));
        }
    }

    let txn = conn.begin().await?;
    expense_category::Entity::find_by_id(input.expense_category_id)
        .filter(expense_category::Column::DelStatus.eq(LIVE))
        .one(&txn)
        .await?
        .ok_or_else(|| CmdError::NotFound("expense category".into()))?;
    account_is_live(&txn, input.payment_method_id).await?;


    let now = crate::migration::now();
    let inserted = expense_recurring::ActiveModel {
        expense_category_id: Set(input.expense_category_id),
        name: Set(required_name(&input.name)?),
        amount: Set(amount),
        payment_method_id: Set(input.payment_method_id),
        rotation: Set(rotation.as_str().to_owned()),
        starts_on: Set(input.starts_on),
        // The first due date is the start date, full stop — **not** clamped to today.
        //
        // The clamp looked right and was wrong: setting `next_due_on` to today throws
        // away the gap, so a schedule created with a start date in the past can never
        // catch up, because there is nothing left to catch up on. It also made
        // `as_at` useless for a backdated run, since the schedule was already future-dated.
        //
        // What actually prevents a year of rent on first run is that *creation posts
        // nothing*: a run is a deliberate act, it takes the date it runs as at, and it
        // reports what it posted. "Never retro-post" is a property of creating a
        // schedule, not a property of the due date.
        next_due_on: Set(Some(input.starts_on)),
        ends_on: Set(input.ends_on),
        note: Set(text(input.note)),
        created_by: Set(crate::auth::current_user_id()),
        del_status: Set(LIVE.to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;

    let today = crate::migration::now().date();
    let posted_count = if input.post_now.unwrap_or(false) {
        post_schedule_in(&txn, &inserted, &[today]).await?.len()
    } else {
        0
    };
    let mut row: expense_recurring::ActiveModel = inserted.into();
    if posted_count > 0 {
        // Advance past what was just posted, so a daily schedule created today does not
        // post again on the next run.
        row.next_due_on = Set(rotation.advance(today));
    }
    let saved = row.update(&txn).await?;
    txn.commit().await?;

    Ok(recurring_view(saved, String::new(), String::new(), posted_count as u64))
}

fn recurring_view(
    r: expense_recurring::Model,
    category_name: String,
    account: String,
    posted_count: u64,
) -> RecurringView {
    RecurringView {
        id: r.id,
        expense_category_id: r.expense_category_id,
        expense_category_name: category_name,
        name: r.name,
        amount: r.amount,
        payment_method_id: r.payment_method_id,
        payment_method_name: account,
        rotation: r.rotation,
        starts_on: r.starts_on,
        next_due_on: r.next_due_on,
        ends_on: r.ends_on,
        note: r.note,
        posted_count,
        created_at: r.created_at,
    }
}

#[tauri::command]
pub async fn update_recurring_expense(id: i32, input: RecurringInput) -> CmdResult<RecurringView> {
    require_permission(db(), "recurring-expense-edit").await?;
    update_recurring_expense_in(db(), id, input).await
}

pub(crate) async fn update_recurring_expense_in<C: ConnectionTrait>(
    conn: &C,
    id: i32,
    input: RecurringInput,
) -> CmdResult<RecurringView> {
    let amount = positive(input.amount, "amount")?;
    let rotation = rotation_of(input.rotation.trim())?;
    if let Some(ends_on) = input.ends_on {
        if ends_on < input.starts_on {
            return Err(CmdError::Validation("a schedule cannot end before it starts".into()));
        }
    }

    let existing = expense_recurring::Entity::find_by_id(id)
        .filter(expense_recurring::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("recurring expense".into()))?;

    expense_category::Entity::find_by_id(input.expense_category_id)
        .filter(expense_category::Column::DelStatus.eq(LIVE))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("expense category".into()))?;
    account_is_live(conn, input.payment_method_id).await?;

    // An already-stopped schedule stays stopped: null means "nothing is scheduled", and
    // an edit is not a request to start it again.
    let still_due = match existing.next_due_on {
        Some(due) => Some(due.max(input.starts_on)),
        None => None,
    };

    let now = crate::migration::now();
    let mut row: expense_recurring::ActiveModel = existing.into();
    row.expense_category_id = Set(input.expense_category_id);
    row.name = Set(required_name(&input.name)?);
    row.amount = Set(amount);
    row.payment_method_id = Set(input.payment_method_id);
    row.rotation = Set(rotation.as_str().to_owned());
    row.starts_on = Set(input.starts_on);
    row.ends_on = Set(input.ends_on);
    row.note = Set(text(input.note));
    row.next_due_on = Set(still_due);
    row.updated_at = Set(now);
    let saved = row.update(conn).await?;

    let categories = expense_category::Entity::find_by_id(saved.expense_category_id)
        .one(conn)
        .await?
        .map(|c| c.name)
        .unwrap_or_default();
    let accounts = account_names(conn, &[saved.payment_method_id]).await?;
    let account = account_name(&accounts, saved.payment_method_id);
    let posted = expense::Entity::find()
        .filter(expense::Column::RecurringExpenseId.eq(saved.id))
        .count(conn)
        .await?;
    Ok(recurring_view(saved, categories, account, posted))
}

#[tauri::command]
pub async fn delete_recurring_expense(id: i32) -> CmdResult<()> {
    require_permission(db(), "recurring-expense-delete").await?;
    delete_recurring_expense_in(db(), id).await
}

pub(crate) async fn delete_recurring_expense_in<C: ConnectionTrait>(
    conn: &C,
    id: i32,
) -> CmdResult<()> {
    let mut row: expense_recurring::ActiveModel = expense_recurring::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("recurring expense".into()))?
        .into();
    // A soft delete, not a hard one: the expenses it posted keep naming it, and
    // `SetNull` only helps on a delete that removes the row outright.
    row.del_status = Set(DELETED.to_owned());
    row.updated_at = Set(crate::migration::now());
    row.update(conn).await?;
    Ok(())
}

#[tauri::command]
pub async fn post_due_recurring_expenses(as_of: Option<chrono::NaiveDate>) -> CmdResult<PostingRun> {
    require_permission(db(), "recurring-expense-post").await?;
    post_due_recurring_expenses_in(db(), as_of).await
}

/// Posts every live schedule that is due on or before `as_of`.
///
/// One transaction per schedule, not one for the run: a schedule that cannot post — a
/// category deleted out from under it — must not roll back the twelve that already did.
/// The failures are returned by name rather than swallowed.
pub(crate) async fn post_due_recurring_expenses_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    as_of: Option<chrono::NaiveDate>,
) -> CmdResult<PostingRun> {
    let today = as_of.unwrap_or_else(|| crate::migration::now().date());

    let due: Vec<expense_recurring::Model> = expense_recurring::Entity::find()
        .filter(expense_recurring::Column::DelStatus.eq(LIVE))
        .filter(expense_recurring::Column::NextDueOn.lte(today))
        .order_by_asc(expense_recurring::Column::NextDueOn)
        .all(conn)
        .await?;

    let mut posted = 0u64;
    let mut failed = Vec::new();

    for schedule in due {
        let Ok(rotation) = rotation_of(&schedule.rotation) else {
            failed.push(schedule.name.clone());
            continue;
        };
        // Every date from the current due date up to the run date, so a schedule left
        // unposted for three months catches up rather than collapsing to one entry.
        let mut dates = Vec::new();
        let mut cursor = schedule.next_due_on;
        while let Some(day) = cursor {
            if day > today {
                break;
            }
            if schedule.ends_on.is_some_and(|last| day > last) {
                break;
            }
            dates.push(day);
            match rotation.advance(day) {
                Some(next) if next > day => cursor = Some(next),
                // A rotation that cannot advance has run past the end of the calendar;
                // stop it rather than spin.
                _ => {
                    cursor = None;
                    break;
                }
            }
            if dates.len() > 1000 {
                break;
            }
        }

        let txn = match conn.begin().await {
            Ok(txn) => txn,
            Err(_) => {
                failed.push(schedule.name.clone());
                continue;
            }
        };
        match post_schedule_in(&txn, &schedule, &dates).await {
            Ok(rows) => {
                let count = rows.len();
                // Advance to the first date still due, or past the run date when the
                // schedule is caught up.
                let next_due = cursor.filter(|d| *d <= today).or(if dates.is_empty() {
                    Some(today)
                } else {
                    None
                });
                let mut row: expense_recurring::ActiveModel = schedule.clone().into();
                row.next_due_on = Set(next_due);
                row.updated_at = Set(crate::migration::now());
                if row.update(&txn).await.is_err() {
                    let _ = txn.rollback().await;
                    failed.push(schedule.name.clone());
                    continue;
                }
                if txn.commit().await.is_err() {
                    failed.push(schedule.name.clone());
                    continue;
                }
                posted += count as u64;
            }
            Err(_) => {
                let _ = txn.rollback().await;
                failed.push(schedule.name.clone());
            }
        }
    }

    Ok(PostingRun { posted, failed })
}

// ---------------------------------------------------------------------------
// Trial balance and balance sheet
// ---------------------------------------------------------------------------

/// One side of the trial balance. A group rather than an account, because that is the
/// level at which a shop reads one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum LedgerGroup {
    /// Tenders: money the shop holds.
    Cash,
    /// What customers owe the shop.
    Receivables,
    /// What the shop owes suppliers.
    Payables,
    /// Money the owner moved in or out.
    OwnerEquity,
    /// What the shop took.
    Revenue,
    /// What the shop spent, other than stock.
    Expenses,
}

impl LedgerGroup {
    /// Which side this account's balance sits on when it is positive.
    ///
    /// A trial balance only means anything if each line lands on the side its account
    /// normally lives on. One global rule — positive means debit — is right for assets
    /// and exactly wrong for revenue, payables and the owner's position, and it makes
    /// the two columns unable to agree, which is the entire point of the report.
    fn normal_side(&self) -> NormalSide {
        match self {
            // Assets and expenses grow on the debit side.
            Self::Cash | Self::Receivables | Self::Expenses => NormalSide::Debit,
            // Liabilities, revenue and the owner's own money grow on the credit side.
            Self::Payables | Self::OwnerEquity | Self::Revenue => NormalSide::Credit,
        }
    }
}

/// The side an account normally sits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NormalSide {
    Debit,
    Credit,
}

/// One line: a group, and how much sits on each side of it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrialLine {
    pub group: LedgerGroup,
    pub debit: Decimal,
    pub credit: Decimal,
    /// `debit - credit`. Negative on a liability group, which is the ordinary case.
    pub net: Decimal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrialBalance {
    pub lines: Vec<TrialLine>,
    pub total_debit: Decimal,
    pub total_credit: Decimal,
    /// `total_debit - total_credit`, rounded. **Zero is the only correct answer**, and
    /// this is here so a figure that should not balance says so rather than looking
    /// like a number.
    pub difference: Decimal,
}

/// Every movement lands on exactly one debit line and one credit line, so the two
/// sides must agree. Nothing is derived from a stored balance: each side is a sum over
/// the same rows the cash book reads.
///
/// Inventory and cost of goods sold are **not** here. Valuing stock needs a cost per
/// unit sold, which lives on `purchase_details` behind a batch join, and a trial balance
/// that quietly valued stock at zero would be worse than one that admits it is not in
/// the figure. That is Stage 10's profit and loss, where it belongs.
pub(crate) async fn trial_balance_in<C: ConnectionTrait>(
    conn: &C,
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
) -> CmdResult<TrialBalance> {
    let lines = lines_in(conn, None, from, to).await?;

    // A sale is money taken (debit cash, credit revenue); a customer receipt is money
    // arriving against a debt already charged (debit cash, credit receivables); a
    // supplier payment settles a debt (debit payables, credit cash). The pair is what
    // makes the two sides agree, and it is why the customer and supplier ledgers have
    // to be counted rather than assumed.
    let mut cash = Decimal::ZERO;
    let mut revenue = Decimal::ZERO;
    let mut receivables = Decimal::ZERO;
    let mut payables = Decimal::ZERO;
    let mut owner = Decimal::ZERO;
    let mut expenses = Decimal::ZERO;

    let customer_net = net_customer_balances_in(conn).await?;
    let supplier_net = net_supplier_balances_in(conn).await?;

    for line in &lines {
        match line.kind.as_str() {
            // Cash came in: the counterpart is what earned it.
            "Income" => {
                cash += line.signed;
                revenue += line.signed;
            }
            "Receipt" => {
                cash += line.signed;
                // A receipt lowers what the customer owes.
                receivables -= line.signed;
            }
            k if k.starts_with("Sale (") => {
                cash += line.signed;
                revenue += line.signed;
            }
            "Deposit" => {
                cash += line.signed;
                owner += line.signed;
            }
            // Cash went out: the counterpart is what it was spent on.
            "Expense" => {
                cash += line.signed;
                expenses += -line.signed;
            }
            "Supplier payment" => {
                cash += line.signed;
                payables += -line.signed;
            }
            "Withdraw" => {
                cash += line.signed;
                owner += line.signed;
            }
            other => {
                // A row the vocabulary does not name would be silently dropped, which is
                // how a balance goes quietly wrong. Say so instead.
                return Err(CmdError::Validation(format!(
                    "{other:?} is a movement the trial balance does not place"
                )));
            }
        }
    }

    let mut lines_out = vec![
        TrialLine { group: LedgerGroup::Cash, debit: Decimal::ZERO, credit: Decimal::ZERO, net: cash },
        TrialLine { group: LedgerGroup::Receivables, debit: Decimal::ZERO, credit: Decimal::ZERO, net: customer_net },
        TrialLine { group: LedgerGroup::Payables, debit: Decimal::ZERO, credit: Decimal::ZERO, net: supplier_net },
        TrialLine { group: LedgerGroup::OwnerEquity, debit: Decimal::ZERO, credit: Decimal::ZERO, net: owner },
        TrialLine { group: LedgerGroup::Revenue, debit: Decimal::ZERO, credit: Decimal::ZERO, net: revenue },
        TrialLine { group: LedgerGroup::Expenses, debit: Decimal::ZERO, credit: Decimal::ZERO, net: expenses },
    ];

    // `net` is `debit − credit`, so it carries the opposite sign to the balance on a
    // credit-natured account: a liability of 150 is a credit balance and reads −150.
    // Splitting against the account's own normal side is what lets the two columns be
    // added up at all.
    for line in &mut lines_out {
        let balance = match line.group.normal_side() {
            NormalSide::Debit => line.net,
            NormalSide::Credit => -line.net,
        };
        if balance >= Decimal::ZERO {
            line.debit = balance;
        } else {
            line.credit = -balance;
        }
        // Recomputed rather than kept, so `net` is *always* `debit − credit` as
        // documented. Leaving it as the natural balance made a liability read
        // positive, and every total built on it was wrong by that much.
        line.debit = line.debit.round_dp(MONEY_SCALE);
        line.credit = line.credit.round_dp(MONEY_SCALE);
        line.net = (line.debit - line.credit).round_dp(MONEY_SCALE);
    }

    let total_debit = lines_out.iter().map(|l| l.debit).sum::<Decimal>().round_dp(MONEY_SCALE);
    let total_credit = lines_out.iter().map(|l| l.credit).sum::<Decimal>().round_dp(MONEY_SCALE);
    Ok(TrialBalance {
        difference: (total_debit - total_credit).round_dp(MONEY_SCALE),
        lines: lines_out,
        total_debit,
        total_credit,
    })
}

#[tauri::command]
pub async fn trial_balance(
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
) -> CmdResult<TrialBalance> {
    require_permission(db(), "accounting-report").await?;
    trial_balance_in(db(), from, to).await
}

/// Net owed across every customer: sales charged less receipts taken. One grouped sum
/// rather than a balance call per customer.
async fn net_customer_balances_in<C: ConnectionTrait>(conn: &C) -> CmdResult<Decimal> {
    let charged = sale::Entity::find()
        .select_only()
        .column_as(sale::Column::GrandTotal.sum(), "total")
        .filter(sale::Column::Status.eq(SALE_STATUS_COMPLETED))
        .into_tuple::<Option<Decimal>>()
        .one(conn)
        .await?
        .flatten()
        .unwrap_or(Decimal::ZERO);
    let paid = customer_receive::Entity::find()
        .select_only()
        .column_as(customer_receive::Column::Amount.sum(), "total")
        .into_tuple::<Option<Decimal>>()
        .one(conn)
        .await?
        .flatten()
        .unwrap_or(Decimal::ZERO);
    Ok((charged - paid).round_dp(MONEY_SCALE))
}

/// Net owed across every supplier, matching `supplier_balance_in`'s four terms:
/// opening + purchases − returns − payments.
async fn net_supplier_balances_in<C: ConnectionTrait>(conn: &C) -> CmdResult<Decimal> {
    let opening = supplier::Entity::find()
        .select_only()
        .column_as(supplier::Column::OpeningBalance.sum(), "total")
        .filter(supplier::Column::DelStatus.eq(LIVE))
        .into_tuple::<Option<Decimal>>()
        .one(conn)
        .await?
        .flatten()
        .unwrap_or(Decimal::ZERO);
    let purchased = purchase::Entity::find()
        .select_only()
        .column_as(purchase::Column::GrandTotal.sum(), "total")
        .filter(purchase::Column::DelStatus.eq(LIVE))
        .into_tuple::<Option<Decimal>>()
        .one(conn)
        .await?
        .flatten()
        .unwrap_or(Decimal::ZERO);
    let returned = purchase_return::Entity::find()
        .select_only()
        .column_as(purchase_return::Column::TotalAmount.sum(), "total")
        .filter(purchase_return::Column::DelStatus.eq(LIVE))
        .into_tuple::<Option<Decimal>>()
        .one(conn)
        .await?
        .flatten()
        .unwrap_or(Decimal::ZERO);
    let paid = supplier_payment::Entity::find()
        .select_only()
        .column_as(supplier_payment::Column::Amount.sum(), "total")
        .into_tuple::<Option<Decimal>>()
        .one(conn)
        .await?
        .flatten()
        .unwrap_or(Decimal::ZERO);

    Ok((opening + purchased - returned - paid).round_dp(MONEY_SCALE))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceSheet {
    pub assets: Vec<TrialLine>,
    pub liabilities: Vec<TrialLine>,
    pub equity: Vec<TrialLine>,
    pub total_assets: Decimal,
    pub total_liabilities: Decimal,
    pub total_equity: Decimal,
    /// `assets − liabilities − equity`. Zero when the books are whole.
    pub difference: Decimal,
}

/// The same figures grouped the way a balance sheet groups them. Derived from the trial
/// balance rather than computed twice, so the two can never disagree.
#[tauri::command]
pub async fn balance_sheet(
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
) -> CmdResult<BalanceSheet> {
    require_permission(db(), "accounting-report").await?;
    balance_sheet_in(db(), from, to).await
}

pub(crate) async fn balance_sheet_in<C: ConnectionTrait>(
    conn: &C,
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
) -> CmdResult<BalanceSheet> {
    let trial = trial_balance_in(conn, from, to).await?;
    let find = |group: LedgerGroup| -> TrialLine {
        trial.lines.iter().find(|l| l.group == group).cloned().unwrap_or(TrialLine {
            group,
            debit: Decimal::ZERO,
            credit: Decimal::ZERO,
            net: Decimal::ZERO,
        })
    };

    // What the shop holds, and what customers owe it.
    let mut assets = vec![find(LedgerGroup::Cash), find(LedgerGroup::Receivables)];
    assets.retain(|l| l.net != Decimal::ZERO);
    // What it owes, and the owner's own position: retained earnings plus anything they
    // have moved in or out.
    let mut liabilities = vec![find(LedgerGroup::Payables)];
    liabilities.retain(|l| l.net != Decimal::ZERO);
    let equity = vec![find(LedgerGroup::Revenue), find(LedgerGroup::Expenses), find(LedgerGroup::OwnerEquity)];

    let total = |rows: &[TrialLine]| -> Decimal { rows.iter().map(|l| l.net).sum::<Decimal>().round_dp(MONEY_SCALE) };
    let total_assets = total(&assets);
    let total_liabilities = total(&liabilities);
    let total_equity = total(&equity);

    Ok(BalanceSheet {
        // The signed sum, not `assets − liabilities − equity`. Every `net` already
        // carries its own accounting sign — a liability and revenue are negative —
        // so subtracting liabilities and equity again counted them twice, and a
        // perfectly balanced book reported a difference equal to twice its own
        // earnings.
        difference: (total_assets + total_liabilities + total_equity).round_dp(MONEY_SCALE),
        assets,
        liabilities,
        equity,
        total_assets,
        total_liabilities,
        total_equity,
    })
}
