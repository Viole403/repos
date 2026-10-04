//! Store credit issued to a customer, usually from a return. The amount is a
//! promise against future sales, tracked as it is spent down by `applied_total`.
//! Spending itself is a receipt in the same `customer_receives` table
//! the balance math already counts — no new money table.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "credit_notes")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Human-facing reference, e.g. `CN-000001`. Derived from `id` like the
    /// sale invoice number, so it cannot collide.
    pub credit_no: String,
    pub customer_id: i32,
    /// The return this credit came from, when there is one.
    pub sale_return_id: Option<i32>,
    pub amount: Decimal,
    /// How much of the amount has been spent. Derived from the negative
    /// receipts, kept on the row so a page of notes does not cost a page of
    /// aggregates — updated in the same transaction that writes the receipt.
    pub applied_total: Decimal,
    pub created_by: Option<i32>,
    pub note: Option<String>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::entities::trade::customer::Entity",
        from = "Column::CustomerId",
        to = "crate::entities::trade::customer::Column::Id"
    )]
    Customer,
}

impl Related<crate::entities::trade::customer::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Customer.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
