//! The audit of one bundle explosion: which components one sold line moved.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "combo_sales")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub sale_id: i32,
    pub sale_detail_id: i32,
    pub combo_item_id: i32,
    pub item_id: i32,
    pub quantity: Decimal,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::sale::Entity",
        from = "Column::SaleId",
        to = "super::sale::Column::Id"
    )]
    Sale,
}

impl Related<super::sale::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Sale.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
