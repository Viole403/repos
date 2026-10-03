//! Unit of measure (piece, kg, box, …).

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "units")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub unit_name: String,
    pub description: Option<String>,
    pub del_status: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::item::Entity")]
    Item,
}



impl ActiveModelBehavior for ActiveModel {}
