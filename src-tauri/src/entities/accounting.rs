//! Money that is not a sale or a purchase: what comes in, what goes out, and what
//! the owner moves in or out of the till.
//!
//! One entity per leaf module, same as `auth` and `trade`. `payment_methods` already
//! lives in `trade` and is shared — a tender is the same vocabulary whether it is
//! settling a supplier or paying the rent.

pub mod deposit_withdraw;
pub mod expense;
pub mod expense_category;
pub mod income;
pub mod income_category;
