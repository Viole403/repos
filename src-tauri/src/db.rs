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
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbErr};
#[cfg(test)]
use sea_orm::{DbBackend, Statement};
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

    let conn = match Database::connect(connect_options(&url)).await {
        Ok(conn) => conn,
        // Only a missing database is recoverable; anything else surfaces as-is.
        Err(e) => match create_missing_postgres_db(&url).await {
            Ok(true) => Database::connect(connect_options(&url))
                .await
                .map_err(|e| DbErr::Custom(format!("created the database but still could not connect: {e}")))?,
            Ok(false) => return Err(e),
            Err(cause) => return Err(DbErr::Custom(format!("could not create the database: {cause}; original error: {e}"))),
        },
    };
    Migrator::up(&conn, None).await?;
    DB.set(conn)
        .map_err(|_| DbErr::Custom("database already initialized".into()))
}

/// Creates the database named in `url` if absent, so a first run matches SQLite,
/// which creates its file. Returns whether one was created.
pub(crate) async fn create_missing_postgres_db(url: &str) -> Result<bool, DbErr> {
    if is_sqlite(url) {
        return Ok(false);
    }
    let Some(name) = database_name(url) else {
        return Ok(false);
    };
    // Interpolated into DDL, so allow nothing that could close the quoted identifier.
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Ok(false);
    }

    let maintenance = url_with_database(url, "postgres");
    let admin = Database::connect(connect_options(&maintenance)).await?;

    match admin.execute_unprepared(&format!("CREATE DATABASE \"{name}\"")).await {
        Ok(_) => Ok(true),
        // Already there — which is the outcome we wanted. Matched on the message
        // rather than SQLSTATE 42P04, which only appears in the Debug form.
        Err(e) if format!("{e}").contains("already exists") => Ok(false),
        Err(e) => Err(e),
    }
}

/// The database name from a URL path, e.g. `postgres://host:5432/repos` -> `repos`.
fn database_name(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let path = after_scheme.split_once('/')?.1;
    let name = path.split(['?', '#']).next().unwrap_or("");
    (!name.is_empty()).then(|| name.to_owned())
}

/// The same URL pointed at a different database, keeping credentials and host.
fn url_with_database(url: &str, name: &str) -> String {
    let (prefix, rest) = url.split_once("://").expect("checked by database_name");
    let (_, tail) = match rest.split_once('/') {
        Some(parts) => parts,
        None => return format!("{prefix}://{rest}/{name}"),
    };
    let query = tail.find(['?', '#']).map(|i| &tail[i..]).unwrap_or("");
    format!("{prefix}://{}/{}{}", rest.split_once('/').map(|(a, _)| a).unwrap_or(rest), name, query)
}

/// The shared connection.
///
/// # Panics
/// Panics if called before [`init`]. That is a programming error, not a runtime
/// condition: `setup` always runs `init` before the window opens.
pub fn db() -> &'static DatabaseConnection {
    DB.get().expect("database not initialized; init() must run during setup")
}

/// A fresh, isolated, migrated database for one test. Test-only helper.
///
/// Every test calls this, and the backend is chosen by `REPOS_TEST_BACKEND`
/// (`sqlite` by default, `postgres`, or `mysql`). That is the whole reason the
/// suite can claim all three backends: the 180-odd call sites are unchanged, so
/// running `REPOS_TEST_BACKEND=postgres cargo test` executes *the same business
/// logic* against Postgres rather than a schema-only smoke test.
///
/// Isolation differs per backend because the cheap option differs:
///
/// - **SQLite** — `sqlite::memory:`, a private database per call for free.
/// - **Postgres** — a uniquely named **schema** with `search_path` pointed at it.
///   A schema is a namespace, so the 29 migrations land inside it and nothing
///   leaks between tests without creating or dropping a database.
/// - **MySQL** — a uniquely named **database**, because MySQL has no nested
///   namespace; a "schema" there *is* a database.
///
/// Server-backed leftovers are swept at the start of each run, so an interrupted
/// run cannot poison the next one. Tests run in parallel by default, which is why
/// the name is unique rather than a single shared scratch schema.
#[cfg(test)]
pub async fn init_for_tests() -> DatabaseConnection {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("sqlx=info"))
        .with_test_writer()
        .try_init();

    match std::env::var("REPOS_TEST_BACKEND").as_deref() {
        Ok("postgres") | Ok("postgresql") => isolated_postgres().await,
        Ok("mysql") | Ok("mariadb") => isolated_mysql().await,
        Ok("sqlite") | Err(_) => {
            let mut o = connect_options("sqlite::memory:");
            o.sqlx_logging(true);
            o.sqlx_logging_level(log::LevelFilter::Info);
            let conn = Database::connect(o).await.expect("connect in-memory sqlite");
            Migrator::up(&conn, None).await.expect("apply migrations");
            conn
        }
        Ok(other) => panic!("REPOS_TEST_BACKEND must be sqlite, postgres, or mysql — got {other:?}"),
    }
}

/// A per-run counter making each test's schema or database name unique.
#[cfg(test)]
fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!(
        "t{}_{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

/// Drops scratch schemas left behind by an interrupted run.
///
/// Run **once per process**, before any test creates its own schema. Sweeping per
/// test instead lets one thread drop a sibling's live schema, which surfaces as
/// `no schema has been selected to create in` — the sibling's `search_path` still
/// names a schema that no longer exists.
#[cfg(test)]
async fn sweep_postgres_schemas(admin: &DatabaseConnection) -> () {
    let rows = admin
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT nspname FROM pg_namespace WHERE nspname LIKE 'repos\\_test\\_%'",
        ))
        .await
        .expect("list scratch schemas");
    for row in rows {
        let name: String = row.try_get("", "nspname").expect("schema name");
        let _ = admin
            .execute_unprepared(&format!("DROP SCHEMA IF EXISTS \"{name}\" CASCADE"))
            .await;
    }
}

/// Drops scratch databases left behind by an interrupted run. Once per process,
/// for the same reason as the Postgres sweep above.
#[cfg(test)]
async fn sweep_mysql_databases(admin: &DatabaseConnection) -> () {
    let rows = admin
        .query_all_raw(Statement::from_string(
            DbBackend::MySql,
            "SELECT schema_name FROM information_schema.schemata WHERE schema_name LIKE 'repos\\_test\\_%'",
        ))
        .await
        .expect("list scratch databases");
    for row in rows {
        let name: String = row.try_get("", "schema_name").expect("database name");
        let _ = admin
            .execute_unprepared(&format!("DROP DATABASE IF EXISTS `{name}`"))
            .await;
    }
}

#[cfg(test)]
async fn isolated_postgres() -> DatabaseConnection {
    let base = std::env::var("REPOS_TEST_POSTGRES_URL")
        .expect("REPOS_TEST_BACKEND=postgres needs REPOS_TEST_POSTGRES_URL");
    let admin = Database::connect(connect_options(&base)).await.expect("postgres admin");

    static SWEEP: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    SWEEP
        .get_or_init(|| sweep_postgres_schemas(&admin))
        .await;

    let schema = format!("repos_test_{}", unique_suffix());
    admin
        .execute_unprepared(&format!("CREATE SCHEMA \"{schema}\""))
        .await
        .expect("create scratch schema");

    let mut o = connect_options(&base);
    o.set_schema_search_path(schema);
    let conn = Database::connect(o).await.expect("connect postgres scratch schema");
    Migrator::up(&conn, None).await.expect("apply migrations on postgres");
    conn
}

#[cfg(test)]
async fn isolated_mysql() -> DatabaseConnection {
    let base = std::env::var("REPOS_TEST_MYSQL_URL")
        .expect("REPOS_TEST_BACKEND=mysql needs REPOS_TEST_MYSQL_URL");
    let admin = Database::connect(connect_options(&base)).await.expect("mysql admin");

    static SWEEP: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    SWEEP
        .get_or_init(|| sweep_mysql_databases(&admin))
        .await;

    let name = format!("repos_test_{}", unique_suffix());
    admin
        .execute_unprepared(&format!("CREATE DATABASE `{name}`"))
        .await
        .expect("create scratch database");

    let conn = Database::connect(connect_options(&url_with_database(&base, &name)))
        .await
        .expect("connect mysql scratch database");
    Migrator::up(&conn, None).await.expect("apply migrations on mysql");
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Drops a database by name through the maintenance database.
    async fn drop_database(base_url: &str, name: &str) {
        let admin = Database::connect(connect_options(&url_with_database(base_url, "postgres")))
            .await
            .expect("admin connection");
        admin
            .execute_unprepared(&format!("DROP DATABASE IF EXISTS \"{name}\""))
            .await
            .expect("drop probe database");
    }

    /// A first run against a fresh install must not fail just because the database
    /// does not exist yet — SQLite creates its file, so Postgres should too.
    #[tokio::test]
    async fn creates_a_missing_postgres_database() {
        let Ok(base) = std::env::var("REPOS_TEST_POSTGRES_URL") else {
            eprintln!("skipping: REPOS_TEST_POSTGRES_URL not set");
            return;
        };
        let name = format!("{}_autocreate_probe", database_name(&base).unwrap_or_default());
        let url = url_with_database(&base, &name);

        // Self-cleaning, so a failed run does not poison the next one.
        drop_database(&base, &name).await;
        assert!(create_missing_postgres_db(&url).await.expect("probe is created"), "a missing database should be created");
        assert!(
            !create_missing_postgres_db(&url).await.expect("second call is a no-op"),
            "an existing database must not be recreated"
        );
        drop_database(&base, &name).await;
    }

    /// A SQLite URL has no server to create anything, and must be left alone.
    #[tokio::test]
    async fn leaves_sqlite_alone() {
        assert!(!create_missing_postgres_db("sqlite::memory:").await.expect("sqlite is not created"));
    }

    #[test]
    fn extracts_the_database_name() {
        assert_eq!(database_name("postgres://u:p@h:5432/repos").as_deref(), Some("repos"));
        assert_eq!(database_name("postgres://u:p@h/repos?sslmode=require").as_deref(), Some("repos"));
        assert_eq!(database_name("postgres://u:p@h:5432/"), None);
        assert_eq!(database_name("nonsense"), None);
    }

    #[test]
    fn keeps_credentials_and_query_when_repointing() {
        assert_eq!(
            url_with_database("postgres://u:p@h:5432/repos", "postgres"),
            "postgres://u:p@h:5432/postgres"
        );
        assert_eq!(
            url_with_database("postgres://u:p@h:5432/repos?sslmode=require", "postgres"),
            "postgres://u:p@h:5432/postgres?sslmode=require"
        );
    }
}
