//! A sellable item. Money is `Decimal` — never `f64` — and quantities are
//! `Decimal` because fractional quantities (2.5 kg) are real.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "items")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    /// Barcode / SKU. Unique across the table.
    pub code: String,
    pub alternative_name: Option<String>,
    pub generic_name: Option<String>,
    pub description: Option<String>,
    pub category_id: Option<i32>,
    pub brand_id: Option<i32>,
    pub purchase_unit_id: Option<i32>,
    pub sale_unit_id: Option<i32>,
    /// How many purchase units make one sale unit. Must be > 0.
    pub conversion_rate: Decimal,
    pub purchase_price: Decimal,
    pub sale_price: Decimal,
    pub whole_sale_price: Option<Decimal>,
    /// Reorder threshold; null disables low-stock alerts.
    pub alert_quantity: Option<Decimal>,
    pub loyalty_point: Decimal,
    pub photo: Option<String>,
    pub del_status: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

/// Item plus joined display names, so the list screen doesn't issue three extra
/// lookups per row.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub id: i32,
    pub name: String,
    pub code: String,
    pub alternative_name: Option<String>,
    pub generic_name: Option<String>,
    pub description: Option<String>,
    pub category_id: Option<i32>,
    pub category_name: Option<String>,
    pub brand_id: Option<i32>,
    pub brand_name: Option<String>,
    pub purchase_unit_id: Option<i32>,
    pub purchase_unit_name: Option<String>,
    pub sale_unit_id: Option<i32>,
    pub sale_unit_name: Option<String>,
    pub conversion_rate: Decimal,
    pub purchase_price: Decimal,
    pub sale_price: Decimal,
    pub whole_sale_price: Option<Decimal>,
    pub alert_quantity: Option<Decimal>,
    pub loyalty_point: Decimal,
    pub photo: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::brand::Entity",
        from = "Column::BrandId",
        to = "super::brand::Column::Id"
    )]
    Brand,
    #[sea_orm(
        belongs_to = "super::item_category::Entity",
        from = "Column::CategoryId",
        to = "super::item_category::Column::Id"
    )]
    Category,
    #[sea_orm(
        belongs_to = "super::unit::Entity",
        from = "Column::PurchaseUnitId",
        to = "super::unit::Column::Id"
    )]
    PurchaseUnit,
    #[sea_orm(
        belongs_to = "super::unit::Entity",
        from = "Column::SaleUnitId",
        to = "super::unit::Column::Id"
    )]
    SaleUnit,
}




// Inverse relations for the `has_many` declared on brand/category/unit.
impl Related<super::brand::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Brand.def()
    }
}

impl Related<super::item_category::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Category.def()
    }
}

impl Related<super::unit::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PurchaseUnit.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
