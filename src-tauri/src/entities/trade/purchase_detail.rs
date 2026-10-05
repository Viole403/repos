//! One line of a purchase: what arrived, how many, and at what price.
//!
//! `total` is written rather than derived, because a purchase is immutable — a
//! receipt states the price agreed at the time, so re-deriving it from a later
//! catalog edit would restate history.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "purchase_details")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub purchase_id: i32,
    pub item_id: i32,
    /// Null for goods with no expiry, which is most of a shop.
    pub batch_id: Option<i32>,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    pub total: Decimal,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::purchase::Entity",
        from = "Column::PurchaseId",
        to = "super::purchase::Column::Id"
    )]
    Purchase,
    #[sea_orm(
        belongs_to = "crate::entities::catalog::item::Entity",
        from = "Column::ItemId",
        to = "crate::entities::catalog::item::Column::Id"
    )]
    Item,
    #[sea_orm(
        belongs_to = "crate::entities::catalog::item_batch::Entity",
        from = "Column::BatchId",
        to = "crate::entities::catalog::item_batch::Column::Id"
    )]
    ItemBatch,
}

impl Related<super::purchase::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Purchase.def()
    }
}

impl Related<crate::entities::catalog::item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Item.def()
    }
}

impl Related<crate::entities::catalog::item_batch::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ItemBatch.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
