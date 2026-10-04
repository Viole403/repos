//! One stored-value movement. Signed `amount` (in is positive), with
//! `balance_after` like the stock ledger so a discrepancy points at one row.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "gift_card_transactions")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub gift_card_id: i32,
    /// The till sale this paid for, if the row is a redemption.
    pub sale_id: Option<i32>,
    /// `Sell`, `Reload` or `Redeem` — see `commands::GIFT_CARD_KINDS`.
    pub kind: String,
    pub amount: Decimal,
    pub balance_after: Decimal,
    /// Tender method on Sell/Reload rows, so the register close can count cash
    /// taken for stored value later.
    pub payment_method: Option<String>,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::gift_card::Entity",
        from = "Column::GiftCardId",
        to = "super::gift_card::Column::Id"
    )]
    GiftCard,
}

impl Related<super::gift_card::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GiftCard.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
