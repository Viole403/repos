//! A fixed asset: the fridge, the forklift, the till itself.
//!
//! Deliberately **not** a row in `items`. An asset is tracked but never sold off
//! the shelf and never decrements on a sale, so making it catalog stock would mean
//! either a flag that every stock query has to remember or a second meaning for
//! `stock_movements`.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "fixed_asset_items")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    /// What the shop paid for one, as a standing figure. Individual purchases are
    /// recorded per movement, because the same model is bought at different prices
    /// over the years.
    pub purchase_price: Decimal,
    /// What the shop would get for one now.
    pub sale_price: Decimal,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "crate::entities::catalog::fixed_asset_movement::Entity")]
    Movement,
}

impl Related<crate::entities::catalog::fixed_asset_movement::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Movement.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
