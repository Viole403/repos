//! A credit sale: one item handed over now, the balance split into dated schedule
//! rows. Paid, due and status are derived (`down_payment + SUM(paid_amount)`),
//! never stored.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "installment_sales")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Human-facing reference, e.g. `INST-000001`. Derived from `id` like the
    /// sale invoice number, so it cannot collide.
    pub reference_no: String,
    pub customer_id: i32,
    pub item_id: i32,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    pub discount_amount: Decimal,
    pub interest_percent: Decimal,
    pub interest_amount: Decimal,
    pub other_charges: Decimal,
    /// `(quantity * unit_price - discount_amount) + interest_amount + other_charges`.
    pub total: Decimal,
    /// Paid up front. Counts toward the derived paid figure like a schedule row.
    pub down_payment: Decimal,
    pub down_payment_method: Option<String>,
    pub number_of_installments: i32,
    /// Days between dues. The reference calls this `installment_type`.
    pub interval_days: i32,
    pub created_by: Option<i32>,
    pub note: Option<String>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::installment_sale_detail::Entity")]
    InstallmentSaleDetail,
}

impl Related<super::installment_sale_detail::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::InstallmentSaleDetail.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
