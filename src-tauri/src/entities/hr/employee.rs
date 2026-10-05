//! Someone on the roster.
//!
//! `base_salary` lives here rather than on the salary run, because a salary run that
//! makes the operator re-key everyone's monthly rate every month is not a salary
//! module — it is a form that gets typed. One source, maintained once.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "employees")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    /// The login this employee also has, when they have one. Null is normal and is not
    /// an incomplete record: most staff at a counter do not need an account.
    pub user_id: Option<i32>,
    /// The monthly figure a salary run starts from. A run may override it for one
    /// month — a raise, a short month — without rewriting history.
    pub base_salary: Decimal,
    pub hire_date: Option<chrono::NaiveDate>,
    /// Null means not set. A separate column would be a second thing that can be empty.
    pub terminated_on: Option<chrono::NaiveDate>,
    pub note: Option<String>,
    pub created_by: Option<i32>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::entities::auth::users::Entity",
        from = "Column::UserId",
        to = "crate::entities::auth::users::Column::Id"
    )]
    User,
}

impl Related<crate::entities::auth::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
