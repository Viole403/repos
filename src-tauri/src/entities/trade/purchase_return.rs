//! Goods sent back to a supplier — the correction path for a purchase.
//!
//! Like a sale return, this is its own document rather than an edit of the
//! purchase. "Already returned" is summed over the return rows at read time
//! rather than stored, so two partial returns of one line cannot exceed what was
//! received.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "purchase_returns")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// `PRT-{id:06}`, derived from the primary key like every other reference here.
    pub reference_no: String,
    pub purchase_id: i32,
    /// Denormalized from the purchase: a returns list should not need a join to
    /// name the supplier.
    pub supplier_id: i32,
    pub returned_at: chrono::NaiveDate,
    pub total_amount: Decimal,
    pub note: Option<String>,
    pub created_by: Option<i32>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
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
        belongs_to = "crate::entities::trade::supplier::Entity",
        from = "Column::SupplierId",
        to = "crate::entities::trade::supplier::Column::Id"
    )]
    Supplier,
    #[sea_orm(has_many = "super::purchase_return_detail::Entity")]
    PurchaseReturnDetail,
}

impl Related<super::purchase::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Purchase.def()
    }
}

impl Related<crate::entities::trade::supplier::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Supplier.def()
    }
}

impl Related<super::purchase_return_detail::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PurchaseReturnDetail.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
