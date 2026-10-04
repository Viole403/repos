//! One line of a price offer. The name is snapshotted from the catalog.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "quotation_details")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub quotation_id: i32,
    pub item_id: i32,
    pub item_name: String,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    pub discount: Decimal,
    pub line_total: Decimal,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::quotation::Entity",
        from = "Column::QuotationId",
        to = "super::quotation::Column::Id"
    )]
    Quotation,
}

impl Related<super::quotation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Quotation.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
