//! A login identity. `password_hash` never leaves the backend.


// Schema is in place ahead of the commands that use it; until then the
// derive output looks unused to rustc.
#![allow(dead_code)]
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub email: String,
    pub password_hash: String,
    pub phone: Option<String>,
    pub role: Option<String>,
    pub photo: Option<String>,
    pub del_status: String,
    pub two_factor_enabled: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

/// Safe projection for the frontend. Omits `password_hash` so it cannot leak
/// through a command return value by accident.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserView {
    pub id: i32,
    pub name: String,
    pub email: String,
    pub phone: Option<String>,
    pub role: Option<String>,
    pub photo: Option<String>,
}

impl From<Model> for UserView {
    fn from(m: Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            email: m.email,
            phone: m.phone,
            role: m.role,
            photo: m.photo,
        }
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::user_roles::Entity")]
    UserRoles,
}



impl ActiveModelBehavior for ActiveModel {}
