//! The append-only stock ledger.
//!
//! Every event that moves stock — sale, return, goods receipt, adjustment,
//! transfer, opening balance — writes one immutable row here. **On-hand quantity
//! is derived as `SUM(quantity)`**, which is why `items` has no quantity column
//! at all: there is nothing to fall out of sync with the ledger.
//!
//! Two conventions make the ledger auditable:
//!
//! - `quantity` is **signed**. Negative leaves the shelf, positive arrives. One
//!   row shape covers both directions, so a reader never special-cases a sign to
//!   work out what happened.
//! - `balance_after` is the running on-hand immediately *after* this row. A stock
//!   count that disagrees with the derived total can then be pointed at a specific
//!   movement instead of at a number that drifted.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "stock_movements")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub item_id: i32,
    /// Null once the causing sale is hard-deleted; the movement outlives it.
    pub sale_id: Option<i32>,
    /// [`MovementType`] serialized as its string form, so the ledger stays
    /// readable in raw SQL.
    pub movement_type: String,
    /// Signed: negative for stock leaving, positive for stock entering.
    pub quantity: Decimal,
    /// Free text: receipt number, adjustment reason code, transfer note.
    pub reference: Option<String>,
    /// On-hand immediately after this row landed.
    pub balance_after: Decimal,
    pub created_at: chrono::NaiveDateTime,
}

/// Why a ledger row exists. A closed vocabulary rather than free text, so a
/// report can group movements without string matching.
///
/// Stored as its string form via [`MovementType::as_str`] / [`MovementType::parse`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum MovementType {
    /// Stock left because it was sold.
    Sale,
    /// Stock came back because the sale was returned.
    SaleReturn,
    /// Stock arrived from a supplier / purchase.
    GoodsReceipt,
    /// Manual correction, e.g. after a stock count. `reference` carries the reason.
    Adjustment,
    /// Stock left this location for another one.
    TransferOut,
    /// Stock arrived from another location.
    TransferIn,
    /// First ever count for an item, e.g. migrating in opening stock.
    OpeningBalance,
}

impl MovementType {
    /// The column value for this variant, and the contract for the `movement_type`
    /// column. `Deserialize` reads the same words back, so the wire form and the
    /// stored form are one vocabulary.
    pub fn as_str(&self) -> &'static str {
        match self {
            MovementType::Sale => "Sale",
            MovementType::SaleReturn => "SaleReturn",
            MovementType::GoodsReceipt => "GoodsReceipt",
            MovementType::Adjustment => "Adjustment",
            MovementType::TransferOut => "TransferOut",
            MovementType::TransferIn => "TransferIn",
            MovementType::OpeningBalance => "OpeningBalance",
        }
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::entities::catalog::item::Entity",
        from = "Column::ItemId",
        to = "crate::entities::catalog::item::Column::Id"
    )]
    Item,
    #[sea_orm(
        belongs_to = "super::sale::Entity",
        from = "Column::SaleId",
        to = "super::sale::Column::Id"
    )]
    Sale,
}

// Inverse of the `has_many` on sale. Declared on the many side, per the
// catalog entities' convention.
impl Related<super::sale::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Sale.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
