//! A lot of a perishable. Carries dates and identity only — **never a quantity**.
//!
//! On-hand for a batch is `SUM(quantity)` over the `stock_movements` rows naming
//! it, the same derivation as item-level on-hand. A quantity column here would be
//! a second figure that can disagree with the ledger, which is the whole reason
//! stock is a ledger in the first place.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "item_batches")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub item_id: i32,
    /// Manufacturer's lot or batch number. Unique per item, not globally.
    pub batch_no: String,
    /// A calendar date, not a timestamp — an expiry has no time component, and
    /// giving it one only invites timezone arguments at the till.
    pub expiry_date: Option<chrono::NaiveDate>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::item::Entity",
        from = "Column::ItemId",
        to = "super::item::Column::Id"
    )]
    Item,
}

impl Related<super::item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Item.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
