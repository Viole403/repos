//! A discount rule the till applies by itself. Four kinds share one table; the
//! columns a kind does not use stay null.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "promotions")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub title: String,
    /// ItemPercent, ItemFixed, OrderPercent, OrderFixed or BuyGet.
    pub kind: String,
    pub target_item_id: Option<i32>,
    pub reward_item_id: Option<i32>,
    pub percent: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub min_total: Option<Decimal>,
    pub buy_qty: Option<Decimal>,
    pub get_qty: Option<Decimal>,
    pub start_at: chrono::NaiveDateTime,
    pub end_at: chrono::NaiveDateTime,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
