//! A customer appointment: who, with which staff member, when.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "bookings")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub customer_id: i32,
    pub service_seller_id: Option<i32>,
    pub created_by: Option<i32>,
    /// Booked, Waiting, Completed or Cancelled — a closed set the commands enforce.
    pub status: String,
    pub start_at: chrono::NaiveDateTime,
    pub end_at: chrono::NaiveDateTime,
    pub note: Option<String>,
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

impl ActiveModelBehavior for ActiveModel {}
