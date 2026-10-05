use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// `name` + `description` and a soft delete — the shape `MasterDataScreen` already
/// drives for units, brands and categories, so income and expense categories are two
/// more configs rather than two more screens.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "income_categories")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::income::Entity")]
    Income,
}

impl Related<super::income::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Income.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
