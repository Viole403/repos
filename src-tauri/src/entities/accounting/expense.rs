//! Money going out that is not a purchase: rent, utilities, a subscription, a fine.
//!
//! Not editable, not deletable — a posted cash-book line, same as an income. The
//! correction path is a reversing entry.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "expenses")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// `EXP-{id:06}`, derived from the primary key rather than `MAX + 1`.
    pub reference_no: String,
    pub expense_category_id: i32,
    /// Which tender the money left as — the cash book is per tender, so this is the
    /// column the balance is grouped by.
    pub payment_method_id: i32,
    /// Who paid, when that is not the account posting it. Null is the common case.
    pub employee_id: Option<i32>,
    /// Always positive. Direction is what the row *is*.
    pub amount: Decimal,
    /// A calendar day, not a moment. Matching `purchased_at`.
    pub occurred_at: chrono::NaiveDate,
    pub note: Option<String>,
    /// The schedule this was posted from, when it came from one. SetNull: a schedule
    /// can be stopped, and stopping it must not take posted history with it.
    pub recurring_expense_id: Option<i32>,
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
        belongs_to = "super::expense_recurring::Entity",
        from = "Column::RecurringExpenseId",
        to = "super::expense_recurring::Column::Id"
    )]
    RecurringExpense,
    #[sea_orm(
        belongs_to = "crate::entities::trade::payment_method::Entity",
        from = "Column::PaymentMethodId",
        to = "crate::entities::trade::payment_method::Column::Id"
    )]
    PaymentMethod,
    #[sea_orm(
        belongs_to = "crate::entities::auth::users::Entity",
        from = "Column::EmployeeId",
        to = "crate::entities::auth::users::Column::Id"
    )]
    Employee,
}

impl Related<super::expense_category::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ExpenseCategory.def()
    }
}

impl Related<super::expense_recurring::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RecurringExpense.def()
    }
}

impl Related<crate::entities::trade::payment_method::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PaymentMethod.def()
    }
}

impl Related<crate::entities::auth::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Employee.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
