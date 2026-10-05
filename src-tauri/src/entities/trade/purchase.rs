//! Goods received from a supplier.
//!
//! A purchase is **not** editable and **not** deletable. It has already written a
//! `GoodsReceipt` row per line into the stock ledger and changed what the shop
//! owes the supplier, so rewriting it would contradict records that are already
//! written. The correction path is a purchase return, the same shape as a sale and
//! its returns.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "purchases")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// `PUR-{id:06}`, derived from the primary key rather than `MAX + 1` so two
    /// concurrent purchases cannot claim the same number.
    pub reference_no: String,
    pub supplier_id: i32,
    /// The supplier's own invoice number. Null is normal.
    pub supplier_invoice_no: Option<String>,
    pub purchased_at: chrono::NaiveDate,
    pub subtotal: Decimal,
    /// Always an **amount**, never a percentage. The input accepts `"10%"` and the
    /// server resolves it before it is stored, because a column that means
    /// different things depending on its text is not a number.
    pub discount: Decimal,
    /// `subtotal - discount`, computed server-side from the lines.
    pub grand_total: Decimal,
    pub note: Option<String>,
    pub created_by: Option<i32>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::entities::trade::supplier::Entity",
        from = "Column::SupplierId",
        to = "crate::entities::trade::supplier::Column::Id"
    )]
    Supplier,
    #[sea_orm(has_many = "super::purchase_detail::Entity")]
    PurchaseDetail,
    #[sea_orm(has_many = "super::purchase_return::Entity")]
    PurchaseReturn,
}

impl Related<crate::entities::trade::supplier::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Supplier.def()
    }
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

impl ActiveModelBehavior for ActiveModel {}
