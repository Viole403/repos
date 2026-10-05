//! One tender against a sale. A split payment writes one row per method.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "sale_payments")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub sale_id: i32,
    /// What the till displayed. Kept for the receipt and for rows written before the
    /// tender became a reference — `payment_method_id` is the source, and a row whose
    /// text matched no live tender keeps its text with a null id.
    pub method: String,
    /// Nullable because a pre-existing row may match no live tender. New rows
    /// always resolve one, so a sale cannot split one tender across spellings.
    pub payment_method_id: Option<i32>,
    pub amount: Decimal,
    /// Gateway reference, receipt number, or whatever the tender produces.
    pub reference: Option<String>,
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
    #[sea_orm(
        belongs_to = "crate::entities::trade::payment_method::Entity",
        from = "Column::PaymentMethodId",
        to = "crate::entities::trade::payment_method::Column::Id"
    )]
    PaymentMethod,
}

impl Related<super::sale::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Sale.def()
    }
}

impl Related<crate::entities::trade::payment_method::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PaymentMethod.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
