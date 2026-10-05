//! Money coming in that is not a sale: a refund from a supplier, a loan, a grant.
//!
//! Like a purchase, this is **not** editable and **not** deletable — it is a posted
//! cash-book line, and rewriting one contradicts the rows already written around it.
//! The correction path is a reversing entry, not an edit.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "incomes")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// `INC-{id:06}`, derived from the primary key rather than `MAX + 1` so two
    /// concurrent entries cannot claim the same number.
    pub reference_no: String,
    pub income_category_id: i32,
    /// Which tender the money arrived as. The cash book's balance is per tender, so
    /// this is the column the balance is grouped by.
    pub payment_method_id: i32,
    /// Who the money belongs to, when that is not the account posting it — a
    /// cashier who paid for the delivery out of pocket, say. Null is the common
    /// case. Points at `users` because that is the only person table that exists;
    /// Stage 8's employee records supersede it.
    pub employee_id: Option<i32>,
    /// Always positive. Direction is what the row *is* — a negative income would be
    /// an expense wearing the wrong table.
    pub amount: Decimal,
    /// A calendar day, not a moment. Matching `purchased_at`, which is also a date.
    pub occurred_at: chrono::NaiveDate,
    pub note: Option<String>,
    pub created_by: Option<i32>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::income_category::Entity",
        from = "Column::IncomeCategoryId",
        to = "super::income_category::Column::Id"
    )]
    IncomeCategory,
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

impl Related<super::income_category::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::IncomeCategory.def()
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
