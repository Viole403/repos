//! One line of a purchase return: what went back, how many, at what price.
//!
//! `unit_price` is taken from the purchase line rather than re-agreed, so the
//! credit the supplier owes is the same figure that was originally invoiced.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "purchase_return_details")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub purchase_return_id: i32,
    /// The purchase line being corrected. Naming the line rather than the item is
    /// what stops a return drawing down the wrong delivery when the same item was
    /// bought more than once.
    pub purchase_detail_id: i32,
    pub item_id: i32,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    pub total: Decimal,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::purchase_return::Entity",
        from = "Column::PurchaseReturnId",
        to = "super::purchase_return::Column::Id"
    )]
    PurchaseReturn,
    #[sea_orm(
        belongs_to = "super::purchase_detail::Entity",
        from = "Column::PurchaseDetailId",
        to = "super::purchase_detail::Column::Id"
    )]
    PurchaseDetail,
    #[sea_orm(
        belongs_to = "crate::entities::catalog::item::Entity",
        from = "Column::ItemId",
        to = "crate::entities::catalog::item::Column::Id"
    )]
    Item,
}

impl Related<super::purchase_detail::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PurchaseDetail.def()
    }
}

impl Related<super::purchase_return::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PurchaseReturn.def()
    }
}

impl Related<crate::entities::catalog::item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Item.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
