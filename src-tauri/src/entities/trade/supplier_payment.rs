//! Money paid to a supplier. Append-only; the balance is the sum over these.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "supplier_payments")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub supplier_id: i32,
    /// Which purchase this settles, when the payment is against one. Null for a
    /// payment on account, which reduces the balance without settling a document.
    pub purchase_id: Option<i32>,
    /// How it was paid. Null on payments recorded before tenders were tracked.
    pub payment_method_id: Option<i32>,
    pub amount: Decimal,
    pub reference: Option<String>,
    pub paid_at: chrono::NaiveDateTime,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::supplier::Entity",
        from = "Column::SupplierId",
        to = "super::supplier::Column::Id"
    )]
    Supplier,
    #[sea_orm(
        belongs_to = "super::purchase::Entity",
        from = "Column::PurchaseId",
        to = "super::purchase::Column::Id"
    )]
    Purchase,
    #[sea_orm(
        belongs_to = "super::payment_method::Entity",
        from = "Column::PaymentMethodId",
        to = "super::payment_method::Column::Id"
    )]
    PaymentMethod,
}

impl Related<super::purchase::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Purchase.def()
    }
}

impl Related<super::payment_method::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PaymentMethod.def()
    }
}

impl Related<super::supplier::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Supplier.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
