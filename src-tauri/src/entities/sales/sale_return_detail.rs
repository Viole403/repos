//! One line of a return. `sale_detail_id` points at the line it reverses.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "sale_return_details")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub sale_return_id: i32,
    pub sale_detail_id: i32,
    pub item_id: i32,
    /// Snapshotted from the sale line, not read from `items.name` now.
    pub item_name: String,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    /// `unit_price * quantity`, less any refund discount added later.
    pub amount: Decimal,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::sale_return::Entity",
        from = "Column::SaleReturnId",
        to = "super::sale_return::Column::Id"
    )]
    SaleReturn,
}

impl Related<super::sale_return::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SaleReturn.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
