//! The owner moving money into or out of a tender: a float top-up, cash taken to
//! the bank, the owner's own capital.
//!
//! Separate from income and expense on purpose. An income is the shop earning and an
//! expense is the shop spending; a deposit is a transfer between the owner's pocket
//! and a tender, so it changes what the drawer holds without changing what the shop
//! earned. Folding them together would make a float top-up look like revenue and
//! every profit figure wrong.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Which way the money moved. A closed vocabulary, so a report can group on it
/// without matching strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum DepositKind {
    /// Into the tender. A float top-up, or the owner's capital going in.
    Deposit,
    /// Out of the tender. Cash taken to the bank, or the owner taking change home.
    Withdraw,
}

impl DepositKind {
    /// The column value for this variant, and the contract for the `kind` column —
    /// the same shape as `PaymentKind`.
    pub fn as_str(&self) -> &'static str {
        match self {
            DepositKind::Deposit => "Deposit",
            DepositKind::Withdraw => "Withdraw",
        }
    }

    /// The direction it moves the tender's balance, as a multiplier.
    pub fn sign(&self) -> Decimal {
        match self {
            DepositKind::Deposit => Decimal::ONE,
            DepositKind::Withdraw => Decimal::NEGATIVE_ONE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "deposit_withdraws")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// `DW-{id:06}`, derived from the primary key rather than `MAX + 1`.
    pub reference_no: String,
    /// Which tender the money moved through. A cash withdrawal is about the drawer,
    /// so this is not optional: the point of the row is that *this* tender moved.
    pub payment_method_id: i32,
    pub kind: String,
    /// Always positive; `kind` says the direction. A negative amount under
    /// `Deposit` is a figure two sources disagree about.
    pub amount: Decimal,
    /// A calendar day, not a moment. Matching `purchased_at`.
    pub occurred_at: chrono::NaiveDate,
    pub note: Option<String>,
    pub created_by: Option<i32>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::entities::trade::payment_method::Entity",
        from = "Column::PaymentMethodId",
        to = "crate::entities::trade::payment_method::Column::Id"
    )]
    PaymentMethod,
}

impl Related<crate::entities::trade::payment_method::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PaymentMethod.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
