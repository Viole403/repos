//! One append-only row of fixed-asset history: an asset arriving or leaving.
//!
//! `quantity` is signed — positive when the asset arrives, negative when it leaves
//! — so on-hand for an asset is `SUM(quantity)` over its rows, never a stored
//! column. `amount` is derived from `quantity` and `unit_price` at write time and
//! kept, because a valuation wants the figures as they stood when the asset moved,
//! not recomputed from today's prices.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "fixed_asset_movements")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub asset_item_id: i32,
    /// [`AssetMovementKind`] as its string form, so the ledger reads in raw SQL.
    pub movement_kind: String,
    /// Signed: positive on arrival, negative on departure.
    pub quantity: Decimal,
    /// What it went at, per movement rather than per item.
    pub unit_price: Decimal,
    /// `quantity * unit_price`, frozen at write time.
    pub amount: Decimal,
    /// Invoice, receipt, or disposal reference.
    pub reference_no: Option<String>,
    pub note: Option<String>,
    pub created_at: chrono::NaiveDateTime,
}

/// Why an asset arrived or left. A closed vocabulary so a disposal report groups
/// without string matching — a write-off and a sale are both "Out" but are not the
/// same event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum AssetMovementKind {
    /// Bought, donated, or found.
    In,
    /// Sold, scrapped, or written off.
    Out,
}

impl AssetMovementKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetMovementKind::In => "In",
            AssetMovementKind::Out => "Out",
        }
    }

    /// The sign this kind contributes to the derived on-hand total.
    pub fn sign(&self) -> Decimal {
        match self {
            AssetMovementKind::In => Decimal::ONE,
            AssetMovementKind::Out => Decimal::NEGATIVE_ONE,
        }
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::entities::catalog::fixed_asset_item::Entity",
        from = "Column::AssetItemId",
        to = "super::fixed_asset_item::Column::Id"
    )]
    AssetItem,
}

impl Related<crate::entities::catalog::fixed_asset_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AssetItem.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
