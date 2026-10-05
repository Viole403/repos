//! A schedule that proposes recurring expenses — rent, a subscription, a salary.
//!
//! **A schedule is not a ledger row.** Posting one writes a real [`expense`] row naming
//! this, so the cash book needs no special case and a posted expense is financial
//! history in the same sense any other expense is. That is also why this table is
//! editable and `expenses` is not: a schedule is a plan, and a plan changes.
//!
//! `next_due_on` is always set while the row is live, so null means one thing only:
//! nothing is scheduled. Stopping a schedule soft-deletes it rather than clearing the
//! date, because a schedule with a null date and no way to say so is the shape of bug
//! this stage has already collected twice.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// How often it repeats. Closed, so a report can group without matching strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum Rotation {
    Daily,
    Weekly,
    BiWeekly,
    Monthly,
    Quarterly,
    Yearly,
}

impl Rotation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Rotation::Daily => "Daily",
            Rotation::Weekly => "Weekly",
            Rotation::BiWeekly => "BiWeekly",
            Rotation::Monthly => "Monthly",
            Rotation::Quarterly => "Quarterly",
            Rotation::Yearly => "Yearly",
        }
    }

    /// The date the next occurrence falls on, given the one before it.
    ///
    /// Month arithmetic keeps the day of month and clamps to the last day when that day
    /// does not exist: the 31st goes to the 28th of a short February and back to the
    /// 31st in March. A schedule that fires on the 31st does not silently become the 3rd,
    /// which is what a naive `checked_add_months` gives.
    pub fn advance(&self, from: chrono::NaiveDate) -> Option<chrono::NaiveDate> {
        use chrono::{Datelike, NaiveDate};
        let months = |n: u32| {
            let total = from.year() as i64 * 12 + from.month0() as i64 + n as i64;
            let year = total.div_euclid(12) as i32;
            let month = total.rem_euclid(12) as u32 + 1;
            let day = from.day().min(days_in_month(year, month));
            NaiveDate::from_ymd_opt(year, month, day)
        };
        match self {
            Rotation::Daily => from.succ_opt(),
            Rotation::Weekly => from.checked_add_signed(chrono::Duration::weeks(1)),
            Rotation::BiWeekly => from.checked_add_signed(chrono::Duration::weeks(2)),
            Rotation::Monthly => months(1),
            Rotation::Quarterly => months(3),
            Rotation::Yearly => months(12),
        }
    }
}

/// The length of a month, found by stepping to the first of the next one and going back
/// a day rather than by remembering February's length.
fn days_in_month(year: i32, month: u32) -> u32 {
    use chrono::{Datelike, NaiveDate};
    let (year, month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(year, month, 1)
        .and_then(|first| first.pred_opt())
        .map(|last| last.day())
        .unwrap_or(28)
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "expense_recurrings")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub expense_category_id: i32,
    /// What it is for, in words. Free text is right here — this is a description, not a
    /// key anything joins on.
    pub name: String,
    pub amount: Decimal,
    /// The account the money will leave. A reference, not a name: the whole point of a
    /// recurring schedule is that it posts without being asked again, and a name it has
    /// to look up is a name that can stop resolving.
    pub payment_method_id: i32,
    pub rotation: String,
    /// When the schedule starts. The first posting is on or after this.
    pub starts_on: chrono::NaiveDate,
    /// Always set while live. Null means nothing is scheduled.
    pub next_due_on: Option<chrono::NaiveDate>,
    /// Null means open-ended — rent has no last month. A date means it stops itself.
    pub ends_on: Option<chrono::NaiveDate>,
    pub note: Option<String>,
    pub created_by: Option<i32>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::expense_category::Entity",
        from = "Column::ExpenseCategoryId",
        to = "super::expense_category::Column::Id"
    )]
    ExpenseCategory,
    #[sea_orm(
        belongs_to = "crate::entities::trade::payment_method::Entity",
        from = "Column::PaymentMethodId",
        to = "crate::entities::trade::payment_method::Column::Id"
    )]
    PaymentMethod,
    #[sea_orm(has_many = "super::expense::Entity")]
    Expense,
}

impl Related<super::expense_category::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ExpenseCategory.def()
    }
}

impl Related<crate::entities::trade::payment_method::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PaymentMethod.def()
    }
}

impl Related<super::expense::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Expense.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
