//! A paid repair job. The charge is stored; what is still due is derived
//! (`servicing_charge - paid_amount`), never stored.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "servicings")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub customer_id: i32,
    pub product_name: String,
    pub product_model: Option<String>,
    pub problem_description: Option<String>,
    pub receiving_date: chrono::NaiveDateTime,
    pub delivery_date: Option<chrono::NaiveDateTime>,
    pub servicing_charge: Decimal,
    pub paid_amount: Decimal,
    /// `Received`, `InRepair`, `Ready` or `Delivered` — see
    /// `commands::SERVICING_STATUSES`. Closed here; the reference leaves it free
    /// text, which splits one state across spellings in every report.
    pub current_status: String,
    pub technician_id: Option<i32>,
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
