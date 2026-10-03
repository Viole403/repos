//! SeaORM entities. Import the leaf modules (`catalog::item`, `auth::users`, …)
//! directly — the re-exports that existed here collided with the derive output.

pub mod auth;
pub mod catalog;
pub mod sales;
