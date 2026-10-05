//! Money a customer paid. Append-only; the balance is the sum over these.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "customer_receives")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub customer_id: i32,
    pub amount: Decimal,
    /// Nullable because nothing recorded the tender before Stage 7, so no historical
    /// receipt can say which one it was. New receipts always name one.
    pub payment_method_id: Option<i32>,
    pub reference: Option<String>,
    /// When the money arrived, which is not when the row was written.
    pub paid_at: chrono::NaiveDateTime,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::customer::Entity",
        from = "Column::CustomerId",
        to = "super::customer::Column::Id"
    )]
    Customer,
    #[sea_orm(
        belongs_to = "crate::entities::trade::payment_method::Entity",
        from = "Column::PaymentMethodId",
        to = "crate::entities::trade::payment_method::Column::Id"
    )]
    PaymentMethod,
}

impl Related<super::customer::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Customer.def()
    }
}

impl Related<crate::entities::trade::payment_method::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PaymentMethod.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
