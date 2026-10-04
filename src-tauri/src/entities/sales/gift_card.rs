//! Stored value. The card number is operator-supplied (printed on the physical
//! card); the balance is `SUM(amount)` over the transactions, never a column.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "gift_cards")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub card_no: String,
    /// Optional PIN, verified on redeem when set.
    pub pin: Option<String>,
    pub created_by: Option<i32>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::gift_card_transaction::Entity")]
    GiftCardTransaction,
}

impl Related<super::gift_card_transaction::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GiftCardTransaction.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
