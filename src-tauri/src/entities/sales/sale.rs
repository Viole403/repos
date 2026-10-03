//! A sale header — one row per transaction.
//!
//! `status` is `Draft` or `Completed`. Checkout writes the draft first and
//! promotes it on payment, so an app death mid-sale leaves a recoverable draft
//! rather than a half-written sale.
//!
//! There is deliberately no `del_status` here. A sale is financial history, not
//! catalog data: it is voided or refunded, never soft-deleted out of existence.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "sales")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Human-facing reference, e.g. `INV-000001`. Derived from `id` so it cannot
    /// collide — see `commands::checkout`.
    pub invoice_no: String,
    pub status: String,
    /// Sum of `unit_price * quantity` across the lines, before any discount.
    pub subtotal: Decimal,
    pub discount_total: Decimal,
    pub tax_total: Decimal,
    /// `subtotal - discount_total + tax_total`.
    pub grand_total: Decimal,
    /// Below `grand_total` is a part-paid / credit sale; above it is change given
    /// back. Both legal, negative is not.
    pub paid_total: Decimal,
    pub payment_method: String,
    /// No foreign key yet — the customers table arrives with the customer stage.
    pub customer_id: Option<i32>,
    pub note: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::sale_detail::Entity")]
    SaleDetail,
    #[sea_orm(has_many = "super::stock_movement::Entity")]
    StockMovement,
}

impl ActiveModelBehavior for ActiveModel {}
