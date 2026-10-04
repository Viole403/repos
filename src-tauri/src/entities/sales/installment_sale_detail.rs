//! One due of an installment sale. `paid_amount` accumulates here; the remaining
//! figure and the paid status (`Unpaid` / `Partial` / `Paid`) are derived from
//! `amount - paid_amount`, never stored.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "installment_sale_details")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub installment_sale_id: i32,
    /// Midnight on the due day.
    pub due_date: chrono::NaiveDateTime,
    pub amount: Decimal,
    pub paid_amount: Decimal,
    /// Set from the payment when the due is fully paid.
    pub paid_date: Option<chrono::NaiveDateTime>,
    /// Method of the latest payment against this due.
    pub payment_method: Option<String>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::installment_sale::Entity",
        from = "Column::InstallmentSaleId",
        to = "super::installment_sale::Column::Id"
    )]
    InstallmentSale,
}

impl Related<super::installment_sale::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::InstallmentSale.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
