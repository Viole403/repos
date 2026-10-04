//! Sales: transaction headers, their lines, and the stock ledger.
//!
//! Same one-entity-per-module constraint as `catalog` — see that module's doc
//! comment.

pub mod booking;
pub mod combo_item;
pub mod combo_sale;
pub mod gift_card;
pub mod gift_card_transaction;
pub mod installment_sale;
pub mod installment_sale_detail;
pub mod promotion;
pub mod quotation;
pub mod quotation_detail;
pub mod register;
pub mod sale;
pub mod sale_detail;
pub mod sale_payment;
pub mod sale_return;
pub mod sale_return_detail;
pub mod servicing;
pub mod stock_movement;
pub mod warranty;
