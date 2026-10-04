//! Sales: transaction headers, their lines, and the stock ledger.
//!
//! Same one-entity-per-module constraint as `catalog` — see that module's doc
//! comment.

pub mod sale;
pub mod sale_detail;
pub mod sale_payment;
pub mod stock_movement;
