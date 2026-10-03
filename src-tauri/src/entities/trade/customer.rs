//! A customer. Balance is derived from sales and receipts, never stored.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "customers")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub code: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub zip: Option<String>,
    pub tax_number: Option<String>,
    pub credit_limit: Decimal,
    pub loyalty_points: Decimal,
    pub note: Option<String>,
    pub photo: Option<String>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::customer_receive::Entity")]
    CustomerReceive,
}

impl Related<super::customer_receive::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CustomerReceive.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
