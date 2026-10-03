//! One cart line of a sale.
//!
//! `item_name` is a snapshot taken from `items.name` at sale time: renaming the
//! item later must not rewrite what the customer was charged for. `unit_price` is
//! likewise the price at the moment of sale, not the current catalog price.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "sale_details")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub sale_id: i32,
    pub item_id: i32,
    pub item_name: String,
    pub unit_price: Decimal,
    pub quantity: Decimal,
    pub discount: Decimal,
    /// `unit_price * quantity - discount`. Per-line net, before any order-level
    /// discount, which is only reflected on the sale header.
    pub line_total: Decimal,
    pub tax_amount: Decimal,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::sale::Entity",
        from = "Column::SaleId",
        to = "super::sale::Column::Id"
    )]
    Sale,
    #[sea_orm(
        belongs_to = "crate::entities::catalog::item::Entity",
        from = "Column::ItemId",
        to = "crate::entities::catalog::item::Column::Id"
    )]
    Item,
}

// Inverse of the `has_many` on sale. Declared on the many side, per the
// catalog entities' convention.
impl Related<super::sale::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Sale.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
