//! A repair ticket for a sold product moving through the pipeline — received from
//! the customer, sent to the vendor, received back, delivered. The product is
//! named as free text: the unit on the bench is not necessarily a catalog row.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "warranties")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub customer_id: i32,
    pub product_name: String,
    pub product_serial_no: Option<String>,
    pub description: Option<String>,
    pub receiving_date: chrono::NaiveDateTime,
    pub delivery_date: Option<chrono::NaiveDateTime>,
    /// `R_F_C`, `S_T_V`, `R_T_V` or `D_T_C` — see `commands::WARRANTY_STATUSES`.
    pub current_status: String,
    pub technician_id: Option<i32>,
    pub present_location: Option<String>,
    pub sender_service_center: Option<String>,
    pub receiver_service_center: Option<String>,
    pub created_by: Option<i32>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::entities::trade::customer::Entity",
        from = "Column::CustomerId",
        to = "crate::entities::trade::customer::Column::Id"
    )]
    Customer,
}

impl Related<crate::entities::trade::customer::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Customer.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
