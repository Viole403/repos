//! How money is tendered. Shared by purchase payments today and by Stage 7's
//! income, expense and deposit/withdraw rows, so those are one vocabulary
//! rather than three free-text columns.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Whether the tender physically moves through the till.
///
/// A register close needs to know what is in the drawer, and only `Cash` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum PaymentKind {
    Cash,
    Transfer,
    Card,
    Qris,
    EWallet,
    Credit,
}

impl PaymentKind {
    /// The column value for this variant, and the contract for the `kind`
    /// column — the same shape as `MovementType`.
    pub fn as_str(&self) -> &'static str {
        match self {
            PaymentKind::Cash => "Cash",
            PaymentKind::Transfer => "Transfer",
            PaymentKind::Card => "Card",
            PaymentKind::Qris => "Qris",
            PaymentKind::EWallet => "EWallet",
            PaymentKind::Credit => "Credit",
        }
    }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "payment_methods")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub kind: String,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::entities::trade::supplier_payment::Entity",
        from = "Column::Id",
        to = "crate::entities::trade::supplier_payment::Column::PaymentMethodId"
    )]
    SupplierPayment,
    #[sea_orm(has_many = "crate::entities::accounting::income::Entity")]
    Income,
    #[sea_orm(has_many = "crate::entities::accounting::expense::Entity")]
    Expense,
    #[sea_orm(has_many = "crate::entities::accounting::deposit_withdraw::Entity")]
    DepositWithdraw,
}

impl Related<crate::entities::trade::supplier_payment::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SupplierPayment.def()
    }
}

impl Related<crate::entities::accounting::income::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Income.def()
    }
}

impl Related<crate::entities::accounting::expense::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Expense.def()
    }
}

impl Related<crate::entities::accounting::deposit_withdraw::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DepositWithdraw.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
