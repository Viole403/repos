//! People who work in the shop, as distinct from accounts that can sign in.
//!
//! An employee is not a login. A part-time helper or a family member is on the
//! roster, appears in the salary run and answers for their attendance, and has no
//! account, no password and no permissions — so they cannot be a `users` row.
//!
//! `user_id` is nullable and links the two where they overlap: a manager who is both
//! an employee and an account. Salary needs the *person*, permissions need the
//! *account*, and conflating them is why a shop ends up either with payroll entries for
//! people who never worked there or with logins for people who did not need one.

pub mod employee;
