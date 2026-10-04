//! Login, session, and permission checks.
//!
//! Every command follows the same split as `commands.rs`: the `#[tauri::command]`
//! shell pulls the process-wide connection that only exists inside the Tauri
//! window, while the logic takes one as an argument so it works against a
//! throwaway in-memory database in tests.

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder,
};
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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
// Install
// ---------------------------------------------------------------------------

/// Whether this install still needs an owner.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallStatus {
    /// No live account exists, so nobody can sign in. The frontend branches on this
    /// to choose between the first-run wizard and the login screen.
    pub needs_setup: bool,
    /// Not a secret: anyone holding the binary can read the database file.
    pub account_count: u64,
}

/// Unauthenticated on purpose — it has to answer before anyone can sign in. It
/// reports a count and nothing else, so the one pre-session command discloses as
/// little as possible.
#[tauri::command]
pub async fn install_status() -> CmdResult<InstallStatus> {
    let conn = db();
    install_status_in(conn).await
}

pub async fn install_status_in<C: sea_orm::ConnectionTrait>(conn: &C) -> CmdResult<InstallStatus> {
    let account_count = live_user_count(conn).await?;
    Ok(InstallStatus {
        needs_setup: account_count == 0,
        account_count,
    })
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

/// Guard an account read or write. The session id is an argument for the same
/// reason it is on [`require`]; nothing from the wire supplies it.
async fn require_account<C: sea_orm::ConnectionTrait>(
    conn: &C,
    user_id: Option<i32>,
    permission: &str,
) -> CmdResult<()> {
    match user_id {
        Some(id) => require(conn, id, permission).await,
        None => Err(CmdError::Forbidden("you are not signed in".into())),
    }
}

/// Guard for creating an account — the one command reachable without a session,
/// and only while nobody can sign in. The exception closes on the first live
/// account, so it cannot mint extra accounts on a claimed install.
pub async fn require_account_create<C: sea_orm::ConnectionTrait>(
    conn: &C,
    user_id: Option<i32>,
) -> CmdResult<()> {
    if live_user_count(conn).await? == 0 {
        return Ok(());
    }
    require_account(conn, user_id, "user-create").await
}

#[tauri::command]
pub async fn list_users() -> CmdResult<Vec<UserView>> {
    let conn = db();
    require_account(conn, auth::current_user_id(), "user-list").await?;
    list_users_in(conn).await
}

pub async fn list_users_in<C: sea_orm::ConnectionTrait>(conn: &C) -> CmdResult<Vec<UserView>> {
    let rows = users::Entity::find()
        .filter(users::Column::DelStatus.eq("Live"))
        .order_by_asc(users::Column::Name)
        .all(conn)
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

    let now = crate::migration::now();
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

    // Resolved before the insert: refusing afterwards would leave an account row with
    // no role attached, which is an account that can sign in and then do nothing.
    let (label, role_type) = resolve_role_label(conn, input.role.as_deref()).await?;

    // The unique index is the real guard; this turns it into a readable message
    // instead of a raw database error surfacing in a toast.
    let row = match model.clone().insert(conn).await {
        Ok(row) => row,
        Err(DbErr::Query(err)) if is_unique_violation(&err) => {
            return Err(CmdError::Conflict(format!(
                "an account with the email {email} already exists"
            )));
        }
        Err(err) => return Err(err.into()),
    };

    // A user row on its own grants nothing: permissions resolve through the
    // `user_roles` pivot, so without this the account authenticates and then every
    // guarded command rejects it. The first account is made owner-level because a
    // fresh install has no users, and so no other way to create one that could reach
    // the screen needed to create the first account.
    attach_role(conn, row.id, &label, role_type).await?;

    Ok(UserView {
        id: row.id,
        name: row.name,
        email: row.email,
        phone: row.phone,
        role: Some(label),
        photo: row.photo,
    })
}

/// Role of the first account. `Master` is owner level and bypasses the
/// `role_permissions` pivot entirely.
const FIRST_ACCOUNT_ROLE: &str = "Super Admin";
const STAFF_ROLE_NAME: &str = "Staff";
const MASTER_ROLE_TYPE: &str = "Master";
const STAFF_ROLE_TYPE: &str = "Staff";

/// Accounts that can actually sign in, which is what "does this install have an
/// owner" means. Counting soft-deleted rows instead made an owner who deleted
/// their own account leave an install that could never be signed into again.
async fn live_user_count<C: sea_orm::ConnectionTrait>(conn: &C) -> CmdResult<u64> {
    Ok(users::Entity::find()
        .filter(users::Column::DelStatus.eq("Live"))
        .count(conn)
        .await?)
}

/// Decide which role a new account gets, or refuse the request.
///
/// The `role` field on a user is free text, so an unknown name would otherwise
/// produce an account with a label and no permissions at all. Resolving it against a
/// real role type is what makes the label mean something.
async fn resolve_role_label<C: sea_orm::ConnectionTrait>(
    conn: &C,
    label: Option<&str>,
) -> CmdResult<(String, &'static str)> {
    // Counted before this account's own row exists, so zero means this is the first.
    let first = live_user_count(conn).await? == 0;
    let requested = label
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(str::to_owned);

    if !first && requested.as_deref() == Some(FIRST_ACCOUNT_ROLE) {
        // Refused rather than quietly downgraded: the owner role belongs to the first
        // account, and naming it must not become a way to reach it.
        return Err(CmdError::Validation(format!(
            "{FIRST_ACCOUNT_ROLE} is the first account's role and cannot be taken by another"
        )));
    }

    Ok(match requested {
        Some(name) => (name, if first { MASTER_ROLE_TYPE } else { STAFF_ROLE_TYPE }),
        None if first => (FIRST_ACCOUNT_ROLE.to_owned(), MASTER_ROLE_TYPE),
        None => (STAFF_ROLE_NAME.to_owned(), STAFF_ROLE_TYPE),
    })
}

/// Attach the resolved role to the account, creating the role when it does not exist.
async fn attach_role<C: sea_orm::ConnectionTrait>(
    conn: &C,
    user_id: i32,
    label: &str,
    role_type: &str,
) -> CmdResult<()> {
    // Matched on the type as well as the name. Matching the name alone would let a
    // later account attach to the `Master` row the first one created, and every
    // operator after the owner would silently hold owner permissions.
    let role_id = match roles::Entity::find()
        .filter(roles::Column::Name.eq(label))
        .filter(roles::Column::RoleType.eq(role_type))
        .filter(roles::Column::DelStatus.eq("Live"))
        .one(conn)
        .await?
    {
        Some(existing) => existing.id,
        None => roles::ActiveModel {
            name: Set(label.to_owned()),
            guard_name: Set(label.to_owned()),
            role_type: Set(role_type.to_owned()),
            del_status: Set("Live".into()),
            created_at: Set(crate::migration::now()),
            updated_at: Set(crate::migration::now()),
            ..Default::default()
        }
        .insert(conn)
        .await?
        .id,
    };

    let already = user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(user_id))
        .filter(user_roles::Column::RoleId.eq(role_id))
        .one(conn)
        .await?;
    if already.is_none() {
        user_roles::ActiveModel {
            user_id: Set(user_id),
            role_id: Set(role_id),
            ..Default::default()
        }
        .insert(conn)
        .await?;
    }

    Ok(())
}

#[tauri::command]
pub async fn create_user(input: UserInput) -> CmdResult<UserView> {
    let conn = db();
    require_account_create(conn, auth::current_user_id()).await?;
    create_user_in(conn, input).await
}

/// Soft-delete, matching the catalog convention — the audit trail wants the row.
///
/// Deleting the account you are signed in as clears the session, or it would keep
/// a session pointing at a row that no longer passes the `Live` filter.
#[tauri::command]
pub async fn delete_user(id: i32) -> CmdResult<()> {
    let conn = db();
    require_account(conn, auth::current_user_id(), "user-destroy").await?;
    delete_user_in(conn, id).await
}

pub async fn delete_user_in<C: sea_orm::ConnectionTrait>(conn: &C, id: i32) -> CmdResult<()> {
    let row = users::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("user".into()))?;

    let mut am: users::ActiveModel = row.into();
    am.del_status = Set("Deleted".into());
    am.updated_at = Set(crate::migration::now());
    am.update(conn).await?;

    if auth::current_user_id() == Some(id) {
        auth::sign_out();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Roles
// ---------------------------------------------------------------------------

/// A role with the permissions it grants, for the role picker in settings.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleView {
    pub id: i32,
    pub name: String,
    /// `"Master"` bypasses the pivot, so it lists every permission.
    pub role_type: String,
    pub permissions: Vec<String>,
}

#[tauri::command]
pub async fn list_roles() -> CmdResult<Vec<RoleView>> {
    let conn = db();
    require_account(conn, auth::current_user_id(), "role-list").await?;
    list_roles_in(conn).await
}

pub async fn list_roles_in<C: sea_orm::ConnectionTrait>(conn: &C) -> CmdResult<Vec<RoleView>> {
    let live_roles = roles::Entity::find()
        .filter(roles::Column::DelStatus.eq("Live"))
        .order_by_asc(roles::Column::Name)
        .all(conn)
        .await?;

    let live_permissions: Vec<permissions::Model> = permissions::Entity::find()
        .filter(permissions::Column::DelStatus.eq("Live"))
        .order_by_asc(permissions::Column::Name)
        .all(conn)
        .await?;

    let held: Vec<i32> = role_permissions::Entity::find()
        .all(conn)
        .await?
        .into_iter()
        .map(|rp| rp.permission_id)
        .collect();

    let views = live_roles
        .into_iter()
        .map(|role| {
            // A Master role is never granted through the pivot, so it would report
            // no permissions at all if the pivot were the only source here.
            let master = role.role_type == MASTER_ROLE_TYPE;
            let names = live_permissions
                .iter()
                .filter(|p| master || held.contains(&p.id))
                .map(|p| p.name.clone())
                .collect();
            RoleView {
                id: role.id,
                name: role.name,
                role_type: role.role_type,
                permissions: names,
            }
        })
        .collect();

    Ok(views)
}

#[tauri::command]
pub async fn set_user_role(user_id: i32, role_id: i32) -> CmdResult<()> {
    let conn = db();
    require_account(conn, auth::current_user_id(), "user-edit").await?;
    set_user_role_in(conn, user_id, role_id).await
}

/// Live accounts holding a `Master` role.
async fn owner_count<C: sea_orm::ConnectionTrait>(conn: &C) -> CmdResult<u64> {
    let owner_roles: Vec<i32> = roles::Entity::find()
        .filter(roles::Column::RoleType.eq(MASTER_ROLE_TYPE))
        .filter(roles::Column::DelStatus.eq("Live"))
        .all(conn)
        .await?
        .into_iter()
        .map(|r| r.id)
        .collect();

    let owner_ids: Vec<i32> = user_roles::Entity::find()
        .filter(user_roles::Column::RoleId.is_in(owner_roles))
        .all(conn)
        .await?
        .into_iter()
        .map(|ur| ur.user_id)
        .collect();

    live_users_among(conn, &owner_ids).await
}

async fn live_users_among<C: sea_orm::ConnectionTrait>(
    conn: &C,
    ids: &[i32],
) -> CmdResult<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    Ok(users::Entity::find()
        .filter(users::Column::Id.is_in(ids.to_vec()))
        .filter(users::Column::DelStatus.eq("Live"))
        .all(conn)
        .await?
        .len() as u64)
}

pub async fn set_user_role_in<C: sea_orm::ConnectionTrait>(
    conn: &C,
    user_id: i32,
    role_id: i32,
) -> CmdResult<()> {
    let role = roles::Entity::find_by_id(role_id)
        .filter(roles::Column::DelStatus.eq("Live"))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("role".into()))?;

    let target = users::Entity::find_by_id(user_id)
        .filter(users::Column::DelStatus.eq("Live"))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("user".into()))?;

    let held: Vec<i32> = user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(user_id))
        .all(conn)
        .await?
        .into_iter()
        .map(|ur| ur.role_id)
        .collect();

    if role.role_type != MASTER_ROLE_TYPE {
        if owner_count(conn).await? <= 1 && was_owner(&held, conn).await? {
            return Err(CmdError::Forbidden(
                "this is the only owner account, so its role cannot be changed".into(),
            ));
        }
    } else if live_user_count(conn).await? > 1 {
        // Assigning by name is what `create_user` refuses. Assigning by id is the same
        // privilege through a different door unless it is refused here too.
        return Err(CmdError::Forbidden(
            "the owner role belongs to the first account and cannot be assigned to another"
                .into(),
        ));
    }

    // The pivot is additive, so this replaces rather than appends — otherwise an
    // account assigned a second role silently keeps the first.
    user_roles::Entity::delete_many()
        .filter(user_roles::Column::UserId.eq(user_id))
        .exec(conn)
        .await?;

    user_roles::ActiveModel {
        user_id: Set(user_id),
        role_id: Set(role_id),
        ..Default::default()
    }
    .insert(conn)
    .await?;

    // `users.role` is the label the sidebar shows, so it follows the pivot.
    let mut am: users::ActiveModel = target.into();
    am.role = Set(Some(role.name));
    am.updated_at = Set(crate::migration::now());
    am.update(conn).await?;

    Ok(())
}

async fn was_owner<C: sea_orm::ConnectionTrait>(held: &[i32], conn: &C) -> CmdResult<bool> {
    Ok(!roles::Entity::find()
        .filter(roles::Column::Id.is_in(held.to_vec()))
        .filter(roles::Column::RoleType.eq(MASTER_ROLE_TYPE))
        .all(conn)
        .await?
        .is_empty())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleInput {
    pub name: String,
}

#[tauri::command]
pub async fn create_role(input: RoleInput) -> CmdResult<RoleView> {
    let conn = db();
    require_account(conn, auth::current_user_id(), "role-create").await?;
    create_role_in(conn, input).await
}

pub async fn create_role_in<C: sea_orm::ConnectionTrait>(conn: &C, input: RoleInput) -> CmdResult<RoleView> {
    let name = required(&input.name, "role name")?;

    if roles::Entity::find()
        .filter(roles::Column::Name.eq(&name))
        .filter(roles::Column::DelStatus.eq("Live"))
        .one(conn)
        .await?
        .is_some()
    {
        return Err(CmdError::Conflict(format!("a role named {name} already exists")));
    }

    // Always Staff. The owner role is not creatable here — it belongs to the first
    // account, and a command that mints one on demand is the door `create_user`
    // and `set_user_role` both refuse to open.
    let now = crate::migration::now();
    let row = roles::ActiveModel {
        name: Set(name.clone()),
        guard_name: Set(name.clone()),
        role_type: Set(STAFF_ROLE_TYPE.to_owned()),
        del_status: Set("Live".into()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(conn)
    .await?;

    Ok(RoleView { id: row.id, name, role_type: row.role_type, permissions: Vec::new() })
}

#[tauri::command]
pub async fn set_role_permissions(role_id: i32, permissions: Vec<String>) -> CmdResult<()> {
    let conn = db();
    require_account(conn, auth::current_user_id(), "role-edit").await?;
    set_role_permissions_in(conn, role_id, permissions).await
}

pub async fn set_role_permissions_in<C: sea_orm::ConnectionTrait>(
    conn: &C,
    role_id: i32,
    wanted: Vec<String>,
) -> CmdResult<()> {
    let role = roles::Entity::find_by_id(role_id)
        .filter(roles::Column::DelStatus.eq("Live"))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("role".into()))?;

    if role.role_type == MASTER_ROLE_TYPE {
        // The owner bypasses the pivot rather than being granted through it, so
        // editing this list would look like it narrowed the owner when it did not.
        return Err(CmdError::Validation(
            "the owner role holds every permission and cannot be edited".into(),
        ));
    }

    let known: HashMap<String, i32> = permissions::Entity::find()
        .filter(permissions::Column::DelStatus.eq("Live"))
        .all(conn)
        .await?
        .into_iter()
        .map(|p| (p.name, p.id))
        .collect();

    if let Some(unknown) = wanted.iter().find(|name| !known.contains_key(*name)) {
        return Err(CmdError::Validation(format!("no permission named {unknown}")));
    }

    role_permissions::Entity::delete_many()
        .filter(role_permissions::Column::RoleId.eq(role_id))
        .exec(conn)
        .await?;

    for name in wanted {
        role_permissions::ActiveModel {
            role_id: Set(role_id),
            permission_id: Set(known[&name]),
            ..Default::default()
        }
        .insert(conn)
        .await?;
    }

    Ok(())
}

#[tauri::command]
pub async fn delete_role(id: i32) -> CmdResult<()> {
    let conn = db();
    require_account(conn, auth::current_user_id(), "role-destroy").await?;
    delete_role_in(conn, id).await
}

pub async fn delete_role_in<C: sea_orm::ConnectionTrait>(conn: &C, id: i32) -> CmdResult<()> {
    let role = roles::Entity::find_by_id(id)
        .filter(roles::Column::DelStatus.eq("Live"))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("role".into()))?;

    if role.role_type == MASTER_ROLE_TYPE {
        return Err(CmdError::Forbidden(
            "the owner role cannot be deleted, since deleting it would lock every command"
                .into(),
        ));
    }

    let holders = user_roles::Entity::find()
        .filter(user_roles::Column::RoleId.eq(id))
        .all(conn)
        .await?;
    if !holders.is_empty() {
        return Err(CmdError::Conflict(
            "accounts still hold this role — move them first".into(),
        ));
    }

    let mut am: roles::ActiveModel = role.into();
    am.del_status = Set("Deleted".into());
    am.updated_at = Set(crate::migration::now());
    am.update(conn).await?;

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
    // Deleting an account is a soft-delete, so without this its grants still resolve —
    // the same hole the role filter below closes one level up.
    if !users::Entity::find_by_id(user_id)
        .filter(users::Column::DelStatus.eq("Live"))
        .one(conn)
        .await?
        .is_some()
    {
        return Ok(Vec::new());
    }

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

    // Resolve the roles down to the live ones first. Revoking a role is a
    // soft-delete, so without this the pivot below would keep handing out
    // everything a deleted role still pointed at.
    let live_roles = roles::Entity::find()
        .filter(roles::Column::Id.is_in(role_ids))
        .filter(roles::Column::DelStatus.eq("Live"))
        .all(conn)
        .await?;

    if live_roles.is_empty() {
        return Ok(Vec::new());
    }

    if live_roles.iter().any(|r| r.role_type == "Master") {
        return Ok(permissions::Entity::find()
            .filter(permissions::Column::DelStatus.eq("Live"))
            .order_by_asc(permissions::Column::Name)
            .all(conn)
            .await?
            .into_iter()
            .map(|p| p.name)
            .collect());
    }

    let live_role_ids: Vec<i32> = live_roles.into_iter().map(|r| r.id).collect();

    let held: Vec<i32> = role_permissions::Entity::find()
        .filter(role_permissions::Column::RoleId.is_in(live_role_ids))
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

/// Reject unless `user_id` holds `permission`.
///
/// This is the boundary the reference expresses as `middleware('permission:…')`.
/// Tauri has no router to hang middleware on, so each command calls this first.
///
/// The identity is an argument rather than read from the session cell on purpose.
/// The session lives in a process-wide `static`, and `cargo test` runs every test
/// in one process — a guard that read the static would have each test's session
/// depend on which other tests happened to run first. `require_permission`
/// supplies the signed-in id; nothing reachable from the wire can, so a caller
/// cannot nominate a more privileged account.
pub async fn require<C: sea_orm::ConnectionTrait>(
    conn: &C,
    user_id: i32,
    permission: &str,
) -> CmdResult<()>
{
    if has_permission_in(conn, user_id, permission).await? {
        return Ok(());
    }

    Err(CmdError::Forbidden(format!(
        "your account does not have the {permission} permission"
    )))
}

/// The session-bound form, for a `#[tauri::command]` to call first.
///
/// Anonymous is its own message rather than a bare "lacks the permission", so a
/// frontend whose session expired can tell "sign in again" from "ask your manager
/// for the permission".
pub async fn require_permission<C: sea_orm::ConnectionTrait>(conn: &C, permission: &str) -> CmdResult<()>
{
    let Some(user_id) = auth::current_user_id() else {
        return Err(CmdError::Forbidden("you are not signed in".into()));
    };

    require(conn, user_id, permission).await
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

/// Set a user's approval PIN. An operator may set their own; anyone else's needs
/// `user-edit` — otherwise any cashier could arm their own second pair of eyes.
#[tauri::command]
pub async fn set_user_pin(user_id: i32, pin: String) -> CmdResult<()> {
    let conn = db();
    let me = auth::current_user_id().ok_or_else(|| CmdError::Forbidden("you are not signed in".into()))?;
    if me != user_id {
        require(conn, me, "user-edit").await?;
    }
    set_user_pin_in(conn, user_id, &pin).await
}

pub async fn set_user_pin_in<C: sea_orm::ConnectionTrait>(
    conn: &C,
    user_id: i32,
    pin: &str,
) -> CmdResult<()> {
    let pin = pin.trim();
    if pin.len() < 4 || !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(CmdError::Validation("PIN must be at least 4 digits".into()));
    }
    let row = users::Entity::find_by_id(user_id)
        .filter(users::Column::DelStatus.eq("Live"))
        .one(conn)
        .await?
        .ok_or_else(|| CmdError::NotFound("user".into()))?;

    let owned = pin.to_owned();
    let pin_hash = tokio::task::spawn_blocking(move || auth::hash_password(&owned))
        .await
        .map_err(|err| CmdError::Validation(format!("could not hash the PIN: {err}")))?
        .map_err(CmdError::Validation)?;

    let mut am: users::ActiveModel = row.into();
    am.pin_hash = Set(Some(pin_hash));
    am.update(conn).await?;
    Ok(())
}

/// Check a PIN against every live account and return the one it belongs to —
/// but only if that account may approve. A correct PIN on an account without
/// `sale-approve` is the same answer as a wrong PIN: "no".
#[tauri::command]
pub async fn verify_approval_pin(pin: String) -> CmdResult<UserView> {
    let conn = db();
    verify_approval_pin_in(conn, &pin).await
}

pub async fn verify_approval_pin_in<C: sea_orm::ConnectionTrait>(
    conn: &C,
    pin: &str,
) -> CmdResult<UserView> {
    let pin = pin.trim();
    if pin.is_empty() {
        return Err(CmdError::Validation("PIN is required".into()));
    }
    let candidates = users::Entity::find()
        .filter(users::Column::DelStatus.eq("Live"))
        .filter(users::Column::PinHash.is_not_null())
        .all(conn)
        .await?;

    // Argon2 off the async worker, one candidate at a time — there are at most a
    // handful of PIN-holding accounts per install, so no batching needed.
    for row in candidates {
        let stored = row.pin_hash.clone().expect("filtered not-null");
        let attempt = pin.to_owned();
        let matches = tokio::task::spawn_blocking(move || auth::verify_password(&attempt, &stored))
            .await
            .unwrap_or(false);
        if matches && has_permission_in(conn, row.id, "sale-approve").await? {
            return Ok(UserView::from(row));
        }
    }
    Err(CmdError::Validation("PIN is incorrect".into()))
}

/// Backends phrase unique violations differently, so match by substring.
fn is_unique_violation(err: &sea_orm::RuntimeErr) -> bool {
    let text = err.to_string().to_lowercase();
    text.contains("unique constraint")
        || text.contains("duplicate key")
        || text.contains("unique violation")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_for_tests;
    use sea_orm::DatabaseConnection;

    /// Create an account without going through `create_user_in`, so tests that
    /// need a *known* password do not pay Argon2's cost twice or depend on the
    /// validator's rules.
    async fn seed_user(conn: &DatabaseConnection, email: &str) -> i32 {
        let now = crate::migration::now();
        let model = users::ActiveModel {
            name: Set("Cashier".into()),
            email: Set(email.into()),
            password_hash: Set(auth::hash_password("correct-horse").expect("hash")),
            del_status: Set("Live".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        model.insert(conn).await.expect("seed user").id
    }

    async fn seed_role(conn: &DatabaseConnection, name: &str, role_type: &str) -> i32 {
        let now = crate::migration::now();
        let model = roles::ActiveModel {
            name: Set(name.into()),
            guard_name: Set(name.into()),
            role_type: Set(role_type.into()),
            del_status: Set("Live".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        model.insert(conn).await.expect("seed role").id
    }

    async fn role_id_of(conn: &DatabaseConnection, name: &str) -> i32 {
        roles::Entity::find()
            .filter(roles::Column::Name.eq(name))
            .one(conn)
            .await
            .expect("query")
            .unwrap_or_else(|| panic!("no role named {name}"))
            .id
    }

    async fn role_permissions_for(conn: &DatabaseConnection, role_id: i32) -> Vec<String> {
        let ids: Vec<i32> = role_permissions::Entity::find()
            .filter(role_permissions::Column::RoleId.eq(role_id))
            .all(conn)
            .await
            .unwrap()
            .into_iter()
            .map(|rp| rp.permission_id)
            .collect();
        let mut names: Vec<String> = permissions::Entity::find()
            .filter(permissions::Column::Id.is_in(ids))
            .all(conn)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        names.sort();
        names
    }

    async fn seed_permission(conn: &DatabaseConnection, name: &str) -> i32 {
        let now = crate::migration::now();
        let model = permissions::ActiveModel {
            name: Set(name.into()),
            group_name: Set("sales".into()),
            guard_name: Set(name.into()),
            del_status: Set("Live".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        model.insert(conn).await.expect("seed permission").id
    }

    #[test]
    fn argon2_round_trips_and_salts() {
        let first = auth::hash_password("correct-horse").expect("hash");
        let second = auth::hash_password("correct-horse").expect("hash");

        assert!(auth::verify_password("correct-horse", &first));
        assert!(auth::verify_password("correct-horse", &second));
        assert!(!auth::verify_password("wrong-horse", &first));

        // Same input must not produce the same digest, or the column would leak
        // "these two accounts chose the same password".
        assert_ne!(first, second);
        assert!(first.starts_with("$argon2id$"), "not a PHC string: {first}");
    }

    #[tokio::test]
    async fn login_accepts_the_right_password_and_is_case_insensitive() {
        let conn = init_for_tests().await;
        seed_user(&conn, "till@example.com").await;

        let view = login_in(
            &conn,
            LoginInput { email: "TILL@Example.com".into(), password: "correct-horse".into() },
        )
        .await
        .expect("login");

        assert_eq!(view.email, "till@example.com");
    }

    #[tokio::test]
    async fn login_rejects_a_wrong_password() {
        let conn = init_for_tests().await;
        seed_user(&conn, "till@example.com").await;

        let err = login_in(
            &conn,
            LoginInput { email: "till@example.com".into(), password: "nope".into() },
        )
        .await
        .expect_err("must reject");

        assert_eq!(err.to_string(), REJECTED);
    }

    /// The message must be byte-identical for an unknown address and a wrong
    /// password. If these ever differ, the login form becomes an oracle for
    /// which addresses are registered.
    #[tokio::test]
    async fn login_does_not_reveal_whether_the_account_exists() {
        let conn = init_for_tests().await;
        seed_user(&conn, "real@example.com").await;

        let unknown = login_in(
            &conn,
            LoginInput { email: "ghost@example.com".into(), password: "correct-horse".into() },
        )
        .await
        .expect_err("unknown account");
        let wrong = login_in(
            &conn,
            LoginInput { email: "real@example.com".into(), password: "wrong".into() },
        )
        .await
        .expect_err("wrong password");

        assert_eq!(unknown.to_string(), wrong.to_string());
        assert_eq!(unknown.to_string(), REJECTED);
    }

    #[tokio::test]
    async fn login_ignores_a_soft_deleted_account() {
        let conn = init_for_tests().await;
        let id = seed_user(&conn, "gone@example.com").await;

        let mut am: users::ActiveModel =
            users::Entity::find_by_id(id).one(&conn).await.unwrap().unwrap().into();
        am.del_status = Set("Deleted".into());
        am.update(&conn).await.unwrap();

        let err = login_in(
            &conn,
            LoginInput { email: "gone@example.com".into(), password: "correct-horse".into() },
        )
        .await
        .expect_err("deleted account must not authenticate");

        assert_eq!(err.to_string(), REJECTED);
    }

    #[tokio::test]
    async fn create_user_hashes_the_password_and_never_stores_it_plain() {
        let conn = init_for_tests().await;

        let view = create_user_in(
            &conn,
            UserInput {
                name: "Ada".into(),
                email: "Ada@Example.com".into(),
                password: "s3cret-passphrase".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("create");

        assert_eq!(view.email, "ada@example.com", "email must be normalised");

        let row = users::Entity::find_by_id(view.id).one(&conn).await.unwrap().unwrap();
        assert_ne!(row.password_hash, "s3cret-passphrase");
        assert!(!row.password_hash.contains("s3cret-passphrase"));
        assert!(auth::verify_password("s3cret-passphrase", &row.password_hash));
    }

    #[tokio::test]
    async fn create_user_rejects_a_short_password() {
        let conn = init_for_tests().await;

        let err = create_user_in(
            &conn,
            UserInput {
                name: "Ada".into(),
                email: "ada@example.com".into(),
                password: "short".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect_err("too short");

        assert!(err.to_string().contains("8 characters"), "got: {err}");
    }

    #[tokio::test]
    async fn create_user_rejects_a_duplicate_email() {
        let conn = init_for_tests().await;
        create_user_in(
            &conn,
            UserInput {
                name: "Ada".into(),
                email: "ada@example.com".into(),
                password: "s3cret-passphrase".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first account");

        let err = create_user_in(
            &conn,
            UserInput {
                name: "Impostor".into(),
                email: "ada@example.com".into(),
                password: "s3cret-passphrase".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect_err("duplicate email");

        assert!(matches!(err, CmdError::Conflict(_)), "got: {err}");
    }

    #[tokio::test]
    async fn a_user_with_no_roles_holds_no_permissions() {
        let conn = init_for_tests().await;
        let user = seed_user(&conn, "nobody@example.com").await;

        assert!(permission_names_in(&conn, user).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_master_role_holds_every_live_permission() {
        let conn = init_for_tests().await;
        let user = seed_user(&conn, "owner@example.com").await;
        let role = seed_role(&conn, "Super Admin", "Master").await;
        seed_permission(&conn, "sales.create").await;
        seed_permission(&conn, "items.edit").await;

        user_roles::ActiveModel { role_id: Set(role), user_id: Set(user), ..Default::default() }
            .insert(&conn)
            .await
            .expect("assign role");

        // Compared against the table rather than a literal list: the catalog is
        // seeded by migration, so the owner set is however many rows exist. The
        // point of `Master` is that it bypasses the pivot entirely.
        let expected: Vec<String> = permissions::Entity::find()
            .filter(permissions::Column::DelStatus.eq("Live"))
            .order_by_asc(permissions::Column::Name)
            .all(&conn)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();

        let held = permission_names_in(&conn, user).await.unwrap();
        assert_eq!(held, expected);
        assert!(has_permission_in(&conn, user, "sales.create").await.unwrap());
        // A seeded permission too, not only the ones this test inserted.
        assert!(has_permission_in(&conn, user, "item-create").await.unwrap());
    }

    #[tokio::test]
    async fn permissions_come_through_the_role_permissions_pivot() {
        let conn = init_for_tests().await;
        let user = seed_user(&conn, "clerk@example.com").await;
        let role = seed_role(&conn, "Cashier", "Staff").await;
        let allowed = seed_permission(&conn, "sales.create").await;
        seed_permission(&conn, "settings.delete").await;

        user_roles::ActiveModel { role_id: Set(role), user_id: Set(user), ..Default::default() }
            .insert(&conn)
            .await
            .expect("assign role");
        role_permissions::ActiveModel {
            permission_id: Set(allowed),
            role_id: Set(role),
            ..Default::default()
        }
        .insert(&conn)
        .await
        .expect("grant permission");

        let held = permission_names_in(&conn, user).await.unwrap();
        assert_eq!(held, vec!["sales.create".to_string()]);
        assert!(has_permission_in(&conn, user, "sales.create").await.unwrap());
        assert!(
            !has_permission_in(&conn, user, "settings.delete").await.unwrap(),
            "an ungranted permission must not pass"
        );
    }

    #[tokio::test]
    async fn a_soft_deleted_role_grants_nothing() {
        let conn = init_for_tests().await;
        let user = seed_user(&conn, "stale@example.com").await;
        let role = seed_role(&conn, "Cashier", "Staff").await;
        let granted = seed_permission(&conn, "sales.create").await;

        user_roles::ActiveModel { role_id: Set(role), user_id: Set(user), ..Default::default() }
            .insert(&conn)
            .await
            .expect("assign role");
        role_permissions::ActiveModel {
            permission_id: Set(granted),
            role_id: Set(role),
            ..Default::default()
        }
        .insert(&conn)
        .await
        .expect("grant permission");

        let mut am: roles::ActiveModel =
            roles::Entity::find_by_id(role).one(&conn).await.unwrap().unwrap().into();
        am.del_status = Set("Deleted".into());
        am.update(&conn).await.unwrap();

        // A deleted *role* must not fall through to the Master branch either.
        assert!(permission_names_in(&conn, user).await.unwrap().is_empty());
    }
    /// Attach a role to a user, so a `require` check has something to resolve.
    async fn give_role(conn: &DatabaseConnection, user: i32, role: i32) {
        user_roles::ActiveModel { role_id: Set(role), user_id: Set(user), ..Default::default() }
            .insert(conn)
            .await
            .expect("assign role");
    }

    /// Grant a permission to a role through the pivot, bypassing `Master`.
    async fn grant(conn: &DatabaseConnection, role: i32, permission: i32) {
        role_permissions::ActiveModel {
            permission_id: Set(permission),
            role_id: Set(role),
            ..Default::default()
        }
        .insert(conn)
        .await
        .expect("grant permission");
    }

    #[tokio::test]
    async fn require_lets_a_granted_permission_through() {
        let conn = init_for_tests().await;
        let user = seed_user(&conn, "granted@example.com").await;
        let role = seed_role(&conn, "Cashier", "Staff").await;
        let permission = seed_permission(&conn, "unit-list").await;
        give_role(&conn, user, role).await;
        grant(&conn, role, permission).await;

        require(&conn, user, "unit-list").await.expect("granted");
    }

    #[tokio::test]
    async fn require_rejects_a_permission_the_role_lacks() {
        let conn = init_for_tests().await;
        let user = seed_user(&conn, "denied@example.com").await;
        let role = seed_role(&conn, "Cashier", "Staff").await;
        let granted = seed_permission(&conn, "unit-list").await;
        give_role(&conn, user, role).await;
        grant(&conn, role, granted).await;

        // Same account, a different verb. This is the case that has to fail.
        let err = require(&conn, user, "unit-create")
            .await
            .expect_err("must not be granted");
        match err {
            CmdError::Forbidden(message) => assert_eq!(
                message, "your account does not have the unit-create permission"
            ),
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn require_rejects_a_user_with_no_roles() {
        let conn = init_for_tests().await;
        let user = seed_user(&conn, "roleless@example.com").await;

        // No role at all must grant nothing, rather than falling through to Master.
        assert!(require(&conn, user, "unit-list").await.is_err());
    }

    #[tokio::test]
    async fn require_lets_a_master_role_through_without_a_pivot_row() {
        let conn = init_for_tests().await;
        let user = seed_user(&conn, "owner@example.com").await;
        let role = seed_role(&conn, "Super Admin", "Master").await;
        give_role(&conn, user, role).await;

        // No permission row is attached: Master bypasses the pivot, which is the
        // point of it. `sale-pos` is checked because it exists in the seeded
        // catalog.
        require(&conn, user, "sale-pos").await.expect("master bypasses the pivot");
    }

    #[tokio::test]
    async fn require_rejects_when_the_granting_role_is_soft_deleted() {
        let conn = init_for_tests().await;
        let user = seed_user(&conn, "stale@example.com").await;
        let role = seed_role(&conn, "Cashier", "Staff").await;
        let permission = seed_permission(&conn, "unit-list").await;
        give_role(&conn, user, role).await;
        grant(&conn, role, permission).await;

        // The grant still exists as a row; the role behind it does not.
        let mut am: roles::ActiveModel =
            roles::Entity::find_by_id(role).one(&conn).await.unwrap().unwrap().into();
        am.del_status = Set("Deleted".into());
        am.update(&conn).await.unwrap();

        assert!(require(&conn, user, "unit-list").await.is_err());
    }

    #[tokio::test]
    async fn require_rejects_an_account_that_does_not_exist() {
        let conn = init_for_tests().await;

        // No user 9999. Must be a plain rejection, never a panic or an allow.
        assert!(require(&conn, 9999, "unit-list").await.is_err());
    }
    /// A fresh install has no users. The very first account must be usable, or
    /// nobody can ever sign in and every command stays locked behind the guards.
    #[tokio::test]
    async fn the_first_account_can_actually_use_the_app() {
        let conn = init_for_tests().await;

        assert!(
            users::Entity::find().all(&conn).await.unwrap().is_empty(),
            "a migrated database starts with no accounts"
        );

        let owner = create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("the first account must be creatable");

        // The user row is not enough: permissions resolve through the pivot, and a
        // soft-deleted user must not pass either.
        assert!(
            require(&conn, owner.id, "sale-pos")
                .await
                .is_ok(),
            "the first account holds no permissions, so every guarded command rejects"
        );
    }
    /// The first account is owner-level because there is nothing to bootstrap it with.
    /// Every account after that must not be, or any operator could mint themselves an
    /// owner by typing a role name.
    #[tokio::test]
    async fn a_later_account_does_not_inherit_owner_level() {
        let conn = init_for_tests().await;
        let input = |email: &str| UserInput {
            name: "Someone".into(),
            email: email.into(),
            password: "correct-horse".into(),
            phone: None,
            role: None,
        };

        let first = create_user_in(&conn, input("first@example.com")).await.expect("first");
        require(&conn, first.id, "sale-pos").await.expect("owner");

        // The same request that just worked, on a second account.
        let second = create_user_in(&conn, input("second@example.com")).await.expect("second");

        assert!(
            require(&conn, second.id, "sale-pos").await.is_err(),
            "a second account must not be owner-level"
        );
    }

    /// Naming a role must not grant it either — the label is resolved, not trusted.
    #[tokio::test]
    async fn a_named_role_does_not_grant_itself() {
        let conn = init_for_tests().await;
        create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");

        // Asking for the owner role by name is refused outright — silently accepting
        // the request and downgrading it would tell the operator the account was set
        // up when it is not.
        let err = create_user_in(
            &conn,
            UserInput {
                name: "Wannabe".into(),
                email: "wannabe@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: Some(FIRST_ACCOUNT_ROLE.into()),
            },
        )
        .await
        .expect_err("naming the owner role must be refused");

        assert!(
            matches!(err, CmdError::Validation(_)),
            "expected a Validation refusal, got {err:?}"
        );
        assert!(
            users::Entity::find()
                .filter(users::Column::Email.eq("wannabe@example.com"))
                .all(&conn)
                .await
                .unwrap()
                .is_empty(),
            "a refused account must not be left behind"
        );
    }

    #[tokio::test]
    async fn a_fresh_install_reports_that_it_needs_an_owner() {
        let conn = init_for_tests().await;

        let fresh = install_status_in(&conn).await.expect("status");
        assert!(fresh.needs_setup, "a migrated database has no accounts");
        assert_eq!(fresh.account_count, 0);

        create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");

        let claimed = install_status_in(&conn).await.expect("status");
        assert!(!claimed.needs_setup, "an install with an owner is claimed");
        assert_eq!(claimed.account_count, 1);
    }

    #[tokio::test]
    async fn deleting_the_only_account_leaves_the_install_claimable() {
        let conn = init_for_tests().await;
        let owner = create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");

        delete_user_in(&conn, owner.id).await.expect("delete the only account");
        assert_eq!(
            live_user_count(&conn).await.unwrap(),
            0,
            "a soft-deleted account is not a usable account"
        );

        let replacement = create_user_in(
            &conn,
            UserInput {
                name: "New Owner".into(),
                email: "new@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("an install with no live accounts is unclaimed again");

        assert!(
            require(&conn, replacement.id, "sale-pos").await.is_ok(),
            "the replacement owner holds no permissions, so the install is unusable"
        );
    }

    #[tokio::test]
    async fn an_unclaimed_install_takes_an_account_without_a_session() {
        let conn = init_for_tests().await;
        assert_eq!(live_user_count(&conn).await.unwrap(), 0);

        require_account_create(&conn, None)
            .await
            .expect("the first account has to be creatable with nobody signed in");
    }

    #[tokio::test]
    async fn a_claimed_install_refuses_an_anonymous_account() {
        let conn = init_for_tests().await;
        let owner = create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");

        let err = require_account_create(&conn, None)
            .await
            .expect_err("one account exists, so the exception is closed");

        assert!(
            matches!(err, CmdError::Forbidden(_)),
            "expected Forbidden, got {err:?}"
        );
        require_account_create(&conn, Some(owner.id))
            .await
            .expect("the owner holds user-create");
    }

    #[tokio::test]
    async fn a_signed_in_account_without_the_permission_cannot_create_another() {
        let conn = init_for_tests().await;
        create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");

        let clerk = create_user_in(
            &conn,
            UserInput {
                name: "Clerk".into(),
                email: "clerk@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: Some("Cashier".into()),
            },
        )
        .await
        .expect("second");

        let err = require_account_create(&conn, Some(clerk.id))
            .await
            .expect_err("a Staff role with no grants must not create accounts");

        assert!(
            matches!(err, CmdError::Forbidden(ref m) if m.contains("user-create")),
            "expected a refusal naming the permission, got {err:?}"
        );
    }

    #[tokio::test]
    async fn listing_accounts_needs_the_list_permission() {
        let conn = init_for_tests().await;
        let owner = create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");

        assert!(
            require_account(&conn, None, "user-list").await.is_err(),
            "an anonymous caller must not read the account list"
        );
        require_account(&conn, Some(owner.id), "user-list")
            .await
            .expect("the owner holds user-list");
    }

    #[tokio::test]
    async fn a_master_role_cannot_be_assigned_to_a_second_account() {
        let conn = init_for_tests().await;
        let owner = create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");

        let clerk = create_user_in(
            &conn,
            UserInput {
                name: "Clerk".into(),
                email: "clerk@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: Some("Cashier".into()),
            },
        )
        .await
        .expect("second");

        let owner_role = roles::Entity::find()
            .filter(roles::Column::RoleType.eq(MASTER_ROLE_TYPE))
            .one(&conn)
            .await
            .unwrap()
            .expect("the first account's role");

        // Assigning by name is what `resolve_role_label` refuses. Assigning by id is
        // the same privilege reached through a different door unless it is refused
        // here too.
        let err = set_user_role_in(&conn, clerk.id, owner_role.id)
            .await
            .expect_err("the owner role must not be assignable");

        assert!(
            matches!(err, CmdError::Forbidden(_)),
            "expected Forbidden, got {err:?}"
        );
        assert!(
            !has_permission_in(&conn, clerk.id, "sale-pos").await.unwrap(),
            "the clerk gained owner permissions"
        );
        // The owner's own assignment is untouched.
        assert!(has_permission_in(&conn, owner.id, "sale-pos").await.unwrap());
    }

    #[tokio::test]
    async fn a_created_role_grants_exactly_the_permissions_it_was_given() {
        let conn = init_for_tests().await;
        create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");
        let clerk = create_user_in(
            &conn,
            UserInput {
                name: "Clerk".into(),
                email: "clerk@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: Some("Cashier".into()),
            },
        )
        .await
        .expect("second");

        let cashier = role_id_of(&conn, "Cashier").await;

        // A role created by typing its name holds nothing until it is given
        // permissions, so an account added this way can sign in and do nothing.
        assert!(
            !has_permission_in(&conn, clerk.id, "sale-pos").await.unwrap(),
            "a role with an empty pivot already grants a permission"
        );

        set_role_permissions_in(&conn, cashier, vec!["sale-pos".into(), "item-list".into()])
            .await
            .expect("grant");

        assert!(has_permission_in(&conn, clerk.id, "sale-pos").await.unwrap());
        assert!(has_permission_in(&conn, clerk.id, "item-list").await.unwrap());
        assert!(
            !has_permission_in(&conn, clerk.id, "item-destroy").await.unwrap(),
            "a permission that was not granted came through anyway"
        );
    }

    #[tokio::test]
    async fn setting_permissions_replaces_rather_than_accumulates() {
        let conn = init_for_tests().await;
        create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");

        let role = create_role_in(&conn, RoleInput { name: "Cashier".into() })
            .await
            .expect("create");

        set_role_permissions_in(&conn, role.id, vec!["sale-pos".into(), "item-list".into()])
            .await
            .expect("grant two");
        set_role_permissions_in(&conn, role.id, vec!["item-list".into()])
            .await
            .expect("narrow to one");

        let names = role_permissions_for(&conn, role.id).await;
        assert_eq!(
            names,
            vec!["item-list".to_owned()],
            "the pivot is additive, so narrowing a role keeps the old grants"
        );
    }

    #[tokio::test]
    async fn a_role_still_held_by_an_account_cannot_be_deleted() {
        let conn = init_for_tests().await;
        create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");
        create_user_in(
            &conn,
            UserInput {
                name: "Clerk".into(),
                email: "clerk@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: Some("Cashier".into()),
            },
        )
        .await
        .expect("second — and it holds Cashier");

        let role_id = role_id_of(&conn, "Cashier").await;

        assert!(
            matches!(
                delete_role_in(&conn, role_id).await,
                Err(CmdError::Conflict(_))
            ),
            "deleting an assigned role leaves the account holding a row nothing resolves"
        );

        // An unheld role goes through.
        let spare = create_role_in(&conn, RoleInput { name: "Spare".into() })
            .await
            .expect("create");
        delete_role_in(&conn, spare.id).await.expect("an unheld role deletes");
        assert!(
            list_roles_in(&conn)
                .await
                .unwrap()
                .iter()
                .all(|r| r.name != "Spare"),
            "a deleted role still appears in the picker"
        );
    }

    #[tokio::test]
    async fn a_created_role_is_never_owner_level() {
        let conn = init_for_tests().await;
        create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");

        let role = create_role_in(&conn, RoleInput { name: "Owner".into() })
            .await
            .expect("a role may be *named* Owner");

        assert_eq!(
            role.role_type, STAFF_ROLE_TYPE,
            "a role named after the owner must not also be the owner type"
        );
    }

    #[tokio::test]
    async fn granting_a_permission_that_does_not_exist_is_refused() {
        let conn = init_for_tests().await;
        create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");
        let role = create_role_in(&conn, RoleInput { name: "Cashier".into() })
            .await
            .expect("create");

        assert!(
            set_role_permissions_in(&conn, role.id, vec!["sale-pos".into(), "wat".into()])
                .await
                .is_err(),
            "a typo'd permission name is accepted and silently grants nothing"
        );
        assert_eq!(
            role_permissions_for(&conn, role.id).await,
            Vec::<String>::new(),
            "a refused grant left part of the list applied"
        );
    }

    #[tokio::test]
    async fn the_owner_role_cannot_be_narrowed_or_deleted() {
        let conn = init_for_tests().await;
        create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");

        let owner_role = roles::Entity::find()
            .filter(roles::Column::RoleType.eq(MASTER_ROLE_TYPE))
            .one(&conn)
            .await
            .unwrap()
            .expect("owner role");

        assert!(
            set_role_permissions_in(&conn, owner_role.id, vec!["item-list".into()])
                .await
                .is_err(),
            "narrowing the owner pivot would look like it revoked the owner's access"
        );
        assert!(
            delete_role_in(&conn, owner_role.id).await.is_err(),
            "deleting the owner role leaves every command unreachable"
        );
    }

    #[tokio::test]
    async fn the_only_owner_cannot_demote_itself() {
        let conn = init_for_tests().await;
        let owner = create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");

        seed_role(&conn, "Cashier", "Staff").await;
        let cashier = roles::Entity::find()
            .filter(roles::Column::Name.eq("Cashier"))
            .one(&conn)
            .await
            .unwrap()
            .expect("cashier role");

        assert!(
            set_user_role_in(&conn, owner.id, cashier.id).await.is_err(),
            "the last owner can demote itself, leaving nobody able to undo it"
        );
        assert!(
            has_permission_in(&conn, owner.id, "sale-pos").await.unwrap(),
            "the refused demotion took effect anyway"
        );
    }

    #[tokio::test]
    async fn assigning_a_role_replaces_rather_than_accumulates() {
        let conn = init_for_tests().await;
        create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");
        let clerk = create_user_in(
            &conn,
            UserInput {
                name: "Clerk".into(),
                email: "clerk@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: Some("Cashier".into()),
            },
        )
        .await
        .expect("second");

        let cashier = role_id_of(&conn, "Cashier").await;
        seed_role(&conn, "Manager", "Staff").await;
        let manager = role_id_of(&conn, "Manager").await;

        set_user_role_in(&conn, clerk.id, manager).await.expect("assign");

        let held: Vec<i32> = user_roles::Entity::find()
            .filter(user_roles::Column::UserId.eq(clerk.id))
            .all(&conn)
            .await
            .unwrap()
            .into_iter()
            .map(|ur| ur.role_id)
            .collect();

        assert_eq!(
            held,
            vec![manager],
            "the pivot is additive, so assigning leaves the account holding both roles"
        );
        assert_ne!(cashier, manager);
    }

    #[tokio::test]
    async fn a_soft_deleted_role_cannot_be_assigned() {
        let conn = init_for_tests().await;
        create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");
        let clerk = create_user_in(
            &conn,
            UserInput {
                name: "Clerk".into(),
                email: "clerk@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: Some("Cashier".into()),
            },
        )
        .await
        .expect("second");

        let cashier = roles::Entity::find()
            .filter(roles::Column::Name.eq("Cashier"))
            .one(&conn)
            .await
            .unwrap()
            .expect("cashier role");
        let mut am: roles::ActiveModel = cashier.clone().into();
        am.del_status = Set("Deleted".into());
        am.update(&conn).await.unwrap();

        assert!(
            set_user_role_in(&conn, clerk.id, cashier.id).await.is_err(),
            "a deleted role still grants nothing, so assigning it must not look like success"
        );
    }

    #[tokio::test]
    async fn a_soft_deleted_account_does_not_pass_a_guard() {
        let conn = init_for_tests().await;
        let owner = create_user_in(
            &conn,
            UserInput {
                name: "Owner".into(),
                email: "owner@example.com".into(),
                password: "correct-horse".into(),
                phone: None,
                role: None,
            },
        )
        .await
        .expect("first");

        delete_user_in(&conn, owner.id).await.expect("delete");

        assert!(
            require_account(&conn, Some(owner.id), "user-list")
                .await
                .is_err(),
            "a deleted owner still passes the guard, so deleting an account removes nothing"
        );
    }
}