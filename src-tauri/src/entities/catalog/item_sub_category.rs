//! Second-level catalog grouping: a category's children.
//!
//! The reference's `ItemSubCategory` belongs to one `ItemCategory` and owns items
//! through `sub_category_id`. Both links are `SetNull`-style here — deleting a
//! grouping must not delete the items in it.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "item_sub_categories")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub category_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub sort_id: i32,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::item_category::Entity",
        from = "Column::CategoryId",
        to = "super::item_category::Column::Id"
    )]
    Category,
    #[sea_orm(has_many = "super::item::Entity")]
    Item,
}

impl Related<super::item_category::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Category.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
