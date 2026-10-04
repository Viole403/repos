//! A price offer to a customer. Moves no stock, takes no payment.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "quotations")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub customer_id: i32,
    pub quotation_no: String,
    pub quoted_at: chrono::NaiveDateTime,
    pub reference_no: Option<String>,
    pub subtotal: Decimal,
    pub discount_total: Decimal,
    pub grand_total: Decimal,
    pub created_by: Option<i32>,
    pub note: Option<String>,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::entities::trade::customer::Entity",
        from = "Column::CustomerId",
        to = "crate::entities::trade::customer::Column::Id"
    )]
    Customer,
    #[sea_orm(has_many = "super::quotation_detail::Entity")]
    QuotationDetail,
}

impl Related<super::quotation_detail::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::QuotationDetail.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
