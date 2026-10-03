//! role_id <-> permission_id pivot.


// Schema is in place ahead of the commands that use it; until then the
// derive output looks unused to rustc.
#![allow(dead_code)]
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "role_permissions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub permission_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub role_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::permissions::Entity",
        from = "Column::PermissionId",
        to = "super::permissions::Column::Id"
    )]
    Permissions,
    #[sea_orm(
        belongs_to = "super::roles::Entity",
        from = "Column::RoleId",
        to = "super::roles::Column::Id"
    )]
    Roles,
}


// Inverse of the `has_many` on roles.
impl Related<super::roles::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Roles.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
