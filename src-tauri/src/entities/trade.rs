//! Trade parties: customers and suppliers, and the money that moves against them.
//!
//! Same one-entity-per-module constraint as `auth` and `catalog`.

pub mod customer;
pub mod customer_receive;
pub mod supplier;
pub mod supplier_payment;
