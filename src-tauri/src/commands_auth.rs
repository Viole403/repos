//! Login, session, and permission checks.
//!
//! Every command follows the same split as `commands.rs`: the `#[tauri::command]`
//! shell pulls the process-wide connection that only exists inside the Tauri
//! window, while the logic takes one as an argument so it works against a
//! throwaway in-memory database in tests.

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DbErr, EntityTrait, QueryFilter,
    QueryOrder,
};
use serde::Deserialize;

use crate::auth;
use crate::commands::CmdError;
use crate::db::db;
use crate::entities::auth::{permissions, role_permissions, roles, user_roles, users};
use crate::entities::auth::users::UserView;

pub type CmdResult<T> = Result<T, CmdError>;

/// One message for both "no such account" and "wrong password". Distinguishing
/// them tells an attacker which addresses are registered, and anyone holding the
/// binary can open this screen.
const REJECTED: &str = "email or password is incorrect";

fn required(raw: &str, label: &str) -> CmdResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CmdError::Validation(format!("{label} is required")));
    }
    Ok(trimmed.to_owned())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginInput {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInput {
    pub name: String,
    pub email: String,
    pub password: String,
    pub phone: Option<String>,
    pub role: Option<String>,
}

// ---------------------------------------------------------------------------
// Login and session
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn login(input: LoginInput) -> CmdResult<UserView> {
    let conn = db();
    let view = login_in(conn, input).await?;
    auth::sign_in(view.id);
    Ok(view)
}

/// Authenticate by email and password.
///
/// The row is looked up case-insensitively because addresses are typed by hand at
/// a till, and the password check is the only thing that decides — a `role` or
/// `del_status` shortcut before it would leak account state.
pub async fn login_in<C: sea_orm::ConnectionTrait>(conn: &C, input: LoginInput) -> CmdResult<UserView>
{
    let email = required(&input.email, "email")?.to_lowercase();
    let password = required(&input.password, "password")?;

    let found = users::Entity::find()
        .filter(users::Column::Email.eq(&email))
        .filter(users::Column::DelStatus.eq("Live"))
        .one(conn)
        .await?;

    let Some(row) = found else {
        return Err(CmdError::Validation(REJECTED.into()));
    };

    // Argon2 is deliberately slow and memory-hard, so it must not run on an async
    // worker. `auth.rs` owns that decision; here it is just a blocking hop.
    let stored = row.password_hash.clone();
    let plaintext = password.clone();
    let matches = tokio::task::spawn_blocking(move || auth::verify_password(&plaintext, &stored))
        .await
        .unwrap_or(false);

    if !matches {
        return Err(CmdError::Validation(REJECTED.into()));
    }

    Ok(UserView::from(row))
}

#[tauri::command]
pub async fn logout() -> CmdResult<()> {
    auth::sign_out();
    Ok(())
}

/// The signed-in user.
///
/// Re-read from the database rather than reusing the view built at login: an
/// account renamed or deleted mid-session must stop working without a restart.
#[tauri::command]
pub async fn current_user() -> CmdResult<UserView> {
    let Some(id) = auth::current_user_id() else {
        return Err(CmdError::NotFound("session".into()));
    };

    let row = users::Entity::find_by_id(id)
        .filter(users::Column::DelStatus.eq("Live"))
        .one(db())
        .await?
        .ok_or_else(|| CmdError::NotFound("session".into()))?;

    Ok(UserView::from(row))
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_users() -> CmdResult<Vec<UserView>> {
    let rows = users::Entity::find()
        .filter(users::Column::DelStatus.eq("Live"))
        .order_by_asc(users::Column::Name)
        .all(db())
        .await?;
    Ok(rows.into_iter().map(UserView::from).collect())
}

/// Create an account, hashing the password before it reaches the database.
pub async fn create_user_in<C: sea_orm::ConnectionTrait>(conn: &C, input: UserInput) -> CmdResult<UserView>
{
    let name = required(&input.name, "name")?;
    let email = required(&input.email, "email")?.to_lowercase();

    if input.password.chars().count() < 8 {
        return Err(CmdError::Validation(
            "password must be at least 8 characters".into(),
        ));
    }

    let plaintext = input.password.clone();
    let password_hash = tokio::task::spawn_blocking(move || auth::hash_password(&plaintext))
        .await
        .map_err(|err| CmdError::Validation(format!("could not hash the password: {err}")))?
        .map_err(CmdError::Validation)?;

    let now = chrono::Utc::now();
    let model = users::ActiveModel {
        name: Set(name),
        email: Set(email.clone()),
        password_hash: Set(password_hash),
        phone: Set(input.phone.clone()),
        role: Set(input.role.clone()),
        photo: Set(None),
        del_status: Set("Live".into()),
        two_factor_enabled: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    // The unique index is the real guard; this turns it into a readable message
    // instead of a raw database error surfacing in a toast.
    match model.insert(conn).await {
        Ok(row) => Ok(UserView::from(row)),
        Err(DbErr::Query(err)) if is_unique_violation(&err) => Err(CmdError::Conflict(format!(
            "an account with the email {email} already exists"
        ))),
        Err(err) => Err(err.into()),
    }
}

#[tauri::command]
pub async fn create_user(input: UserInput) -> CmdResult<UserView> {
    let conn = db();
    create_user_in(conn, input).await
}

/// Soft-delete, matching the catalog convention — the audit trail wants the row.
///
/// Deleting the account you are signed in as clears the session, or it would keep
/// a session pointing at a row that no longer passes the `Live` filter.
#[tauri::command]
pub async fn delete_user(id: i32) -> CmdResult<()> {
    let row = users::Entity::find_by_id(id)
        .one(db())
        .await?
        .ok_or_else(|| CmdError::NotFound("user".into()))?;

    let mut am: users::ActiveModel = row.into();
    am.del_status = Set("Deleted".into());
    am.updated_at = Set(chrono::Utc::now());
    am.update(db()).await?;

    if auth::current_user_id() == Some(id) {
        auth::sign_out();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

/// Every permission name the user holds, through any of their roles.
///
/// A user with no roles is a legitimate state and grants nothing. `role_type =
/// 'Master'` is the owner-level role and bypasses the pivot entirely, so an owner
/// is never locked out by a missing grant.
pub async fn permission_names_in<C: sea_orm::ConnectionTrait>(conn: &C, user_id: i32) -> CmdResult<Vec<String>>
{
    let role_ids: Vec<i32> = user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(user_id))
        .all(conn)
        .await?
        .into_iter()
        .map(|r| r.role_id)
        .collect();

    if role_ids.is_empty() {
        return Ok(Vec::new());
    }

    let owns = roles::Entity::find()
        .filter(roles::Column::Id.is_in(role_ids.clone()))
        .filter(roles::Column::RoleType.eq("Master"))
        .filter(roles::Column::DelStatus.eq("Live"))
        .one(conn)
        .await?
        .is_some();
    if owns {
        return Ok(permissions::Entity::find()
            .filter(permissions::Column::DelStatus.eq("Live"))
            .all(conn)
            .await?
            .into_iter()
            .map(|p| p.name)
            .collect());
    }

    let held: Vec<i32> = role_permissions::Entity::find()
        .filter(role_permissions::Column::RoleId.is_in(role_ids))
        .all(conn)
        .await?
        .into_iter()
        .map(|rp| rp.permission_id)
        .collect();

    if held.is_empty() {
        return Ok(Vec::new());
    }

    Ok(permissions::Entity::find()
        .filter(permissions::Column::Id.is_in(held))
        .filter(permissions::Column::DelStatus.eq("Live"))
        .order_by_asc(permissions::Column::Name)
        .all(conn)
        .await?
        .into_iter()
        .map(|p| p.name)
        .collect())
}

/// Does the user hold `permission`?
///
/// The reference guards each route with `middleware('permission:...')`. There is
/// no router here to hang middleware on, so the equivalent is a check inside the
/// command — which is why this must exist before later stages add data.
pub async fn has_permission_in<C: sea_orm::ConnectionTrait>(conn: &C, user_id: i32, wanted: &str) -> CmdResult<bool>
{
    Ok(permission_names_in(conn, user_id)
        .await?
        .iter()
        .any(|name| name == wanted))
}

#[tauri::command]
pub async fn has_permission(user_id: i32, permission: String) -> CmdResult<bool> {
    let conn = db();
    has_permission_in(conn, user_id, &permission).await
}

/// The signed-in user's permissions. The frontend uses this to hide what the
/// operator cannot do, rather than letting a command reject at the last step.
#[tauri::command]
pub async fn my_permissions() -> CmdResult<Vec<String>> {
    let Some(id) = auth::current_user_id() else {
        return Err(CmdError::NotFound("session".into()));
    };
    let conn = db();
    permission_names_in(conn, id).await
}

/// SQLite and Postgres phrase a unique violation differently, and neither uses a
/// code this crate exposes portably, so both messages are matched by substring.
fn is_unique_violation(err: &sea_orm::RuntimeErr) -> bool {
    let text = err.to_string().to_lowercase();
    text.contains("unique constraint")
        || text.contains("duplicate key")
        || text.contains("unique violation")
}