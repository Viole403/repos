//! Catalog: units, brands, item categories, items.
//!
//! Same one-entity-per-module constraint as `auth` — see that module's doc comment.

pub mod brand;
pub mod fixed_asset_item;
pub mod fixed_asset_movement;
pub mod item;
pub mod item_batch;
pub mod item_category;
pub mod item_sub_category;
pub mod unit;
