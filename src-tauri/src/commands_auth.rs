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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_for_tests;
    use sea_orm::DatabaseConnection;

    /// Create an account without going through `create_user_in`, so tests that
    /// need a *known* password do not pay Argon2's cost twice or depend on the
    /// validator's rules.
    async fn seed_user(conn: &DatabaseConnection, email: &str) -> i32 {
        let now = chrono::Utc::now();
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
        let now = chrono::Utc::now();
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

    async fn seed_permission(conn: &DatabaseConnection, name: &str) -> i32 {
        let now = chrono::Utc::now();
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
}