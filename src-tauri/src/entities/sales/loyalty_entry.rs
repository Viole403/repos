//! Loyalty points as a ledger: immutable Earn/Redeem/Void rows. The balance is
//! `SUM(points)` — never the `loyalty_points` column next door, which stopped
//! being written when this table arrived.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "loyalty_entries")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub customer_id: i32,
    /// The till sale this movement belongs to, if any.
    pub sale_id: Option<i32>,
    /// `Earn`, `Redeem` or `Void` — see `commands::LOYALTY_KINDS`.
    pub kind: String,
    /// Signed: earn is positive, redeem and void negative.
    pub points: i64,
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
