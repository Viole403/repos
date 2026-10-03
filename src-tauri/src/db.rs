//! Database access layer.
//!
//! Backend-agnostic on purpose: the URL determines the driver, so the same
//! build runs against SQLite, Postgres, or MySQL. SQLite is the default because
//! this is a desktop app and a local file needs no server.
//!
//!   sqlite://<app_data_dir>/repos.db?mode=rwc
//!   postgres://user:pass@localhost:5432/repos
//!   mysql://user:pass@localhost:3306/repos

use std::path::PathBuf;

use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use sea_orm_migration::MigratorTrait;
use tokio::sync::OnceCell;

use crate::migration::Migrator;

/// Process-wide handle. Tauri's `setup` runs once, so every command can assume
/// this is already populated rather than threading a connection through `State`.
static DB: OnceCell<DatabaseConnection> = OnceCell::const_new();

/// Where the SQLite file lives. Ignored when `REPOS_DATABASE_URL` overrides it.
fn default_sqlite_path(app_data_dir: &std::path::Path) -> PathBuf {
    app_data_dir.join("repos.db")
}

fn is_sqlite(url: &str) -> bool {
    url.starts_with("sqlite:")
}

/// SQLite is single-writer. SeaORM forces `max_connections(1)` for it, but being
/// explicit documents why and keeps the two backends' pools from drifting.
fn connect_options(url: &str) -> ConnectOptions {
    let mut opts = ConnectOptions::new(url.to_owned());
    opts.sqlx_logging(false);
    opts.max_connections(if is_sqlite(url) { 1 } else { 10 });
    opts
}

/// Opens the connection, runs pending migrations, and publishes it to [`db`].
///
/// `url_override` comes from the `REPOS_DATABASE_URL` environment variable.
pub async fn init(app_data_dir: &std::path::Path, url_override: Option<&str>) -> Result<(), DbErr> {
    let url = match url_override {
        Some(u) if !u.trim().is_empty() => u.trim().to_owned(),
        _ => {
            let path = default_sqlite_path(app_data_dir);
            if let Some(parent) = path.parent() {
                // mode=rwc creates the file on first launch.
                std::fs::create_dir_all(parent)
                    .map_err(|e| DbErr::Custom(format!("creating data dir: {e}")))?;
            }
            format!("sqlite://{}?mode=rwc", path.display())
        }
    };

    let conn = Database::connect(connect_options(&url)).await?;
    Migrator::up(&conn, None).await?;
    DB.set(conn)
        .map_err(|_| DbErr::Custom("database already initialized".into()))
}

/// The shared connection.
///
/// # Panics
/// Panics if called before [`init`]. That is a programming error, not a runtime
/// condition: `setup` always runs `init` before the window opens.
pub fn db() -> &'static DatabaseConnection {
    DB.get().expect("database not initialized; init() must run during setup")
}

/// In-memory database with migrations applied. Test-only helper.
///
/// Each call gets a fresh isolated database, so tests don't share state or race
/// on the migration table.
#[cfg(test)]
pub async fn init_for_tests() -> DatabaseConnection {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("sqlx=info"))
        .with_test_writer()
        .try_init();
    let mut o = connect_options("sqlite::memory:");
    o.sqlx_logging(true);
    o.sqlx_logging_level(log::LevelFilter::Info);
    let conn = Database::connect(o)
        .await
        .expect("connect in-memory sqlite");
    Migrator::up(&conn, None)
        .await
        .expect("apply migrations");
    conn
}
