//! Users, roles, and permissions.
//!
//! Each entity lives in its own leaf module. SeaORM's `DeriveEntityModel` expands
//! `Model` / `Column` / `Entity` / `PrimaryKey` / `Relation` into the enclosing
//! module, so two entities in one file collide on those names and fail to compile.

pub mod permissions;
pub mod role_permissions;
pub mod roles;
pub mod user_roles;
pub mod users;
