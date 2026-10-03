//! Database access layer.
//!
//! Backend-agnostic on purpose: the URL determines the driver, so the same
//! build runs against SQLite, Postgres, or MySQL.
//!
//! **Postgres is the default backend.** SQLite remains fully supported and is
//! selected by giving a `sqlite://` URL — it is the right choice when the app has
//! to run on a shop counter with no database server installed.
//!
//! Resolution order for the connection URL:
//!
//!   1. `REPOS_DATABASE_URL` — always wins, whatever it contains.
//!   2. [`DEFAULT_POSTGRES_URL`] — the default backend.
//!   3. A `sqlite://` URL to opt into the embedded file database.
//!
//!   postgres://user:pass@host:5432/repos
//!   mysql://user:pass@host:3306/repos
//!   sqlite://<path>/repos.db?mode=rwc

use std::path::PathBuf;
use std::time::Duration;

use sea_orm::sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use sea_orm_migration::MigratorTrait;
use tokio::sync::OnceCell;

use crate::migration::Migrator;

/// Process-wide handle. Tauri's `setup` runs once, so every command can assume
/// this is already populated rather than threading a connection through `State`.
static DB: OnceCell<DatabaseConnection> = OnceCell::const_new();

/// The backend used when `REPOS_DATABASE_URL` is unset.
///
/// Override it per-install rather than editing this: set `REPOS_DATABASE_URL` in the
/// environment before launching the app. Credentials belong in that variable, never
/// in source.
pub const DEFAULT_POSTGRES_URL: &str = "postgres://postgres:postgres@localhost:5432/repos";

/// Where the SQLite file lives. Only used when a `sqlite://` URL is chosen.
fn default_sqlite_path(app_data_dir: &std::path::Path) -> PathBuf {
    app_data_dir.join("repos.db")
}

fn is_sqlite(url: &str) -> bool {
    url.starts_with("sqlite:")
}

/// Pool sizing, plus the SQLite-only settings that have no URL equivalent.
///
/// SQLite is single-writer, so the pool is capped at 1. That cap is also what makes
/// the rest of this safe: `foreign_keys` is a *per-connection* pragma, so it has to
/// be applied to every connection sqlx opens. `map_sqlx_sqlite_opts` mutates the
/// driver options before the pool is built, so no connection can escape it.
///
/// - `foreign_keys` — **off by default in SQLite.** Without this every
///   `ON DELETE CASCADE` / `SET NULL` in the migrations is silently a no-op.
/// - `journal_mode = WAL` — survives a process kill mid-write, where the default
///   rollback journal can leave the file inconsistent. This is the single most
///   important setting for a POS that holds sales data.
/// - `synchronous = NORMAL` — the standard companion to WAL: durable across
///   application crashes, only at risk from an OS-level power loss.
/// - `busy_timeout` — wait rather than immediately returning SQLITE_BUSY when the
///   UI thread and a background write overlap.
fn connect_options(url: &str) -> ConnectOptions {
    let sqlite = is_sqlite(url);
    let mut opts = ConnectOptions::new(url.to_owned());
    opts.sqlx_logging(false);
    opts.max_connections(if sqlite { 1 } else { 10 });

    if sqlite {
        opts.map_sqlx_sqlite_opts(|o| {
            o.foreign_keys(true)
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Normal)
                .busy_timeout(Duration::from_secs(5))
        });
    }

    opts
}

/// Opens the connection, runs pending migrations, and publishes it to [`db`].
///
/// `url_override` comes from the `REPOS_DATABASE_URL` environment variable. Without
/// it the app targets [`DEFAULT_POSTGRES_URL`].
///
/// The `app_data_dir` argument is only consulted for `sqlite://` URLs, where it
/// supplies the directory the database file lives in.
pub async fn init(app_data_dir: &std::path::Path, url_override: Option<&str>) -> Result<(), DbErr> {
    let url = match url_override {
        Some(u) if !u.trim().is_empty() => u.trim().to_owned(),
        _ => DEFAULT_POSTGRES_URL.to_owned(),
    };

    // SQLite needs its parent directory to exist and `mode=rwc` to create the file;
    // neither applies to a server-backed URL.
    if is_sqlite(&url) {
        let path = default_sqlite_path(app_data_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DbErr::Custom(format!("creating data dir: {e}")))?;
        }
    }

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
/// on the migration table. This is the fast inner loop; use [`connect_to`] when a
/// test must exercise a server-backed backend too.
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

/// Connect to an arbitrary backend and run migrations. Test-only.
///
/// The portability claim in `AGENTS.md` — one schema, three backends — is only
/// worth anything if a migration is actually executed against a server backend.
/// `cargo check` cannot catch a column type that Postgres rejects, so tests that
/// matter use this.
///
/// Skips (returns `None`) when the backend isn't reachable, so a developer without
/// a local Postgres isn't blocked from running the suite.
#[cfg(test)]
pub async fn connect_to(url: &str) -> Option<DatabaseConnection> {
    let conn = match Database::connect(connect_options(url)).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipping {url}: {e}");
            return None;
        }
    };
    if let Err(e) = Migrator::up(&conn, None).await {
        panic!("migrations failed on {url}: {e}");
    }
    Some(conn)
}
