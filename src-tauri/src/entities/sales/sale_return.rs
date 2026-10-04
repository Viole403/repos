//! Goods handed back against a sale. A sale is never edited to undo one of these.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "sale_returns")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub sale_id: i32,
    /// Human-facing reference, e.g. `RET-000001`. Derived from `id`.
    pub return_no: String,
    pub reason: String,
    pub refunded_total: Decimal,
    /// The account that authorised it, for the audit trail.
    pub returned_by: Option<i32>,
    pub note: Option<String>,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::sale::Entity",
        from = "Column::SaleId",
        to = "super::sale::Column::Id"
    )]
    Sale,
    #[sea_orm(has_many = "super::sale_return_detail::Entity")]
    SaleReturnDetail,
}

impl Related<super::sale_return_detail::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SaleReturnDetail.def()
    }
}

impl Related<super::sale::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Sale.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
