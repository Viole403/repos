//! Password hashing and session state.
//!
//! Split out of `commands.rs` because hashing is CPU- and memory-hard by design:
//! Argon2 at its default parameters takes tens of milliseconds and ~19 MiB per call.
//! That is fine on a blocking thread and catastrophic on an async runtime's worker,
//! so every hash and verify runs through `spawn_blocking`.

use std::sync::RwLock;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand_core::OsRng;

/// Hash a plaintext password into a PHC string (`$argon2id$v=19$...`).
///
/// The salt is random per call and the parameters are baked into the string, so
/// verification reads them back rather than assuming today's config — which means
/// the cost can be raised later without invalidating existing hashes.
pub fn hash_password(plaintext: &str) -> Result<String, String> {
    // `hash_password` takes the salt as an argument and generates one only in the
    // `getrandom` feature's convenience wrapper, so the salt is supplied here.
    let salt = SaltString::generate(OsRng);
    Argon2::default()
        .hash_password(plaintext.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| format!("could not hash the password: {err}"))
}

/// Check a plaintext against a stored PHC string.
///
/// Returns `false` for a malformed hash rather than an error: a corrupt row should
/// fail the login, not surface a parse message to the cashier.
pub fn verify_password(plaintext: &str, stored: &str) -> bool {
    match PasswordHash::new(stored) {
        Ok(parsed) => Argon2::default()
            .verify_password(plaintext.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Who is signed in. A desktop app serves one operator per launch, so a process-wide
/// cell is the honest model — there is no request context to thread a session through.
static CURRENT_USER: RwLock<Option<i32>> = RwLock::new(None);

pub fn sign_in(user_id: i32) {
    if let Ok(mut slot) = CURRENT_USER.write() {
        *slot = Some(user_id);
    }
}

pub fn sign_out() {
    if let Ok(mut slot) = CURRENT_USER.write() {
        *slot = None;
    }
}

pub fn current_user_id() -> Option<i32> {
    CURRENT_USER
        .read()
        .ok()
        .and_then(|slot| *slot)
}