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

use serde::{Deserialize, Serialize};

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
    DB.get().expect(
        "no database is open. Every command here needs one, and this install has not \
         chosen it yet — the frontend asks database_status first and routes to the \
         wizard's database step, so reaching this means a command was invoked before \
         that step ran. Note release builds use panic = \"abort\", so this is fatal \
         rather than a rejected promise.",
    )
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

// ---------------------------------------------------------------------------
// Backend selection
//
// The app supports SQLite, Postgres, and MySQL, but it runs on **exactly one**.
// Which one is chosen once, in the first-run wizard, and persisted — the point is
// that switching is a choice rather than a rewrite, not that all three are ever
// live at once.
// ---------------------------------------------------------------------------

/// Which database engine to run on. One install uses one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Sqlite,
    Postgres,
    Mysql,
}

impl BackendKind {
    pub fn parse(key: &str) -> Result<Self, DbErr> {
        match key.trim().to_ascii_lowercase().as_str() {
            "sqlite" | "sqlite3" => Ok(Self::Sqlite),
            "postgres" | "postgresql" => Ok(Self::Postgres),
            "mysql" | "mariadb" => Ok(Self::Mysql),
            other => Err(DbErr::Custom(format!(
                "unknown database type {other:?}: choose sqlite, postgres, or mysql"
            ))),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
        }
    }

    /// The scheme this backend's URL uses, used to sanity-check a typed URL.
    fn scheme_prefixes(self) -> &'static [&'static str] {
        match self {
            Self::Sqlite => &["sqlite:"],
            Self::Postgres => &["postgres://", "postgresql://"],
            Self::Mysql => &["mysql://", "mariadb://"],
        }
    }

    fn accepts_url(self, url: &str) -> bool {
        let url = url.trim();
        self.scheme_prefixes()
            .iter()
            .any(|prefix| url.starts_with(prefix))
    }
}

/// The persisted choice: which engine, and how to reach it.
///
/// SQLite's location is not stored — it is always inside the app data directory,
/// so recording a path would only create a way for it to go stale.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseSelection {
    pub backend: BackendKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl DatabaseSelection {
    /// The URL actually used at runtime. SQLite resolves to its file path here,
    /// because that path depends on the app data directory rather than on config.
    pub fn resolved_url(&self, app_data_dir: &std::path::Path) -> String {
        match self.backend {
            BackendKind::Sqlite => format!("sqlite://{}?mode=rwc", default_sqlite_path(app_data_dir).display()),
            _ => self
                .url
                .clone()
                .unwrap_or_else(|| DEFAULT_POSTGRES_URL.to_owned()),
        }
    }

    /// A URL safe to show on screen: the password is replaced, never displayed or
    /// written to the frontend.
    pub fn redacted_url(&self, app_data_dir: &std::path::Path) -> String {
        let url = self.resolved_url(app_data_dir);
        redact_password(&url)
    }
}

/// Replaces the password in a URL with `***`, leaving everything else readable.
///
/// The URL is shown back to the operator so they can tell *which* database the app
/// is on. Printing the credential to do it would defeat the point.
pub fn redact_password(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let (authority, tail) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let Some((userinfo, host)) = authority.rsplit_once('@') else {
        return url.to_owned();
    };

    // The split has to happen in this order. `userinfo` is everything before the
    // `@`, so treating it as the user name and appending `:***` leaves the password
    // sitting in the output — `shop:hunter2` becomes `shop:hunter2:***@host`. The
    // user name is only the part before the *first* `:`; everything after it is the
    // secret.
    match userinfo.split_once(':') {
        Some((user, _secret)) => format!("{scheme}://{user}:***@{host}{tail}"),
        // A user with no password is not a secret, so it is shown as written.
        None => url.to_owned(),
    }
}

/// Where the selection is persisted. Plain JSON: one small file, read once at
/// startup, and readable by the operator who wants to fix a bad URL by hand.
fn selection_path(app_data_dir: &std::path::Path) -> PathBuf {
    app_data_dir.join("database.json")
}

pub fn load_selection(app_data_dir: &std::path::Path) -> Option<DatabaseSelection> {
    let raw = std::fs::read_to_string(selection_path(app_data_dir)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Persists the choice and, on Unix, restricts it to the owner.
///
/// It holds a database password, so it is written `0600` rather than the `0644`
/// a plain `write` would produce.
pub fn save_selection(app_data_dir: &std::path::Path, selection: &DatabaseSelection) -> Result<(), DbErr> {
    let path = selection_path(app_data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| DbErr::Custom(format!("creating data dir: {e}")))?;
    }
    let body = serde_json::to_string_pretty(selection)
        .map_err(|e| DbErr::Custom(format!("serialising database choice: {e}")))?;
    std::fs::write(&path, body).map_err(|e| DbErr::Custom(format!("writing database choice: {e}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| DbErr::Custom(format!("securing database choice: {e}")))?;
    }
    Ok(())
}

/// What `bootstrap` found, and whether the app can serve commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bootstrap {
    /// No choice recorded yet — the first-run wizard picks one.
    Unconfigured,
    /// Connected, and the connection is published to [`db`].
    Ready,
    /// A choice exists but could not be used. The wizard can replace it.
    Failed,
}

/// The startup state, for the wizard to read. No password ever crosses this.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseStatus {
    /// `unconfigured`, `ready`, or `error`.
    pub state: &'static str,
    /// Which engine this install runs on, if one has been chosen.
    pub backend: Option<&'static str>,
    /// The redacted URL, so an operator can see which database is in use.
    pub location: Option<String>,
    /// Why it failed, when `state` is `error`.
    pub message: Option<String>,
}

/// Resolves the URL this install should use, and why.
///
/// `REPOS_DATABASE_URL` wins over the persisted choice so a developer can point a
/// packaged build at a scratch database without editing the file.
pub fn resolve_url(app_data_dir: &std::path::Path) -> Option<(BackendKind, String)> {
    if let Ok(url) = std::env::var("REPOS_DATABASE_URL") {
        if !url.trim().is_empty() {
            let backend = if is_sqlite(&url) {
                BackendKind::Sqlite
            } else if url.starts_with("mysql://") || url.starts_with("mariadb://") {
                BackendKind::Mysql
            } else {
                BackendKind::Postgres
            };
            return Some((backend, url.trim().to_owned()));
        }
    }
    let selection = load_selection(app_data_dir)?;
    let url = selection.resolved_url(app_data_dir);
    Some((selection.backend, url))
}

/// Prepares the connection at startup. Never panics.
///
/// The wizard is what chooses the database, so a missing or broken one has to
/// leave the window able to open — panicking here would make the question
/// unaskable, which is how the app used to end: a Postgres that was not running
/// meant no app at all, and no way to say "use SQLite instead".
pub async fn bootstrap(app_data_dir: &std::path::Path) -> (Bootstrap, Option<DatabaseStatus>) {
    let Some((backend, url)) = resolve_url(app_data_dir) else {
        return (
            Bootstrap::Unconfigured,
            Some(DatabaseStatus {
                state: "unconfigured",
                backend: None,
                location: None,
                message: None,
            }),
        );
    };

    match init(app_data_dir, Some(&url)).await {
        Ok(()) => (
            Bootstrap::Ready,
            Some(DatabaseStatus {
                state: "ready",
                backend: Some(backend.key()),
                location: Some(redact_password(&url)),
                message: None,
            }),
        ),
        Err(e) => (
            Bootstrap::Failed,
            Some(DatabaseStatus {
                state: "error",
                backend: Some(backend.key()),
                location: Some(redact_password(&url)),
                message: Some(describe_connection_failure(&e)),
            }),
        ),
    }
}

/// Whether `db()` is published, without touching it.
///
/// The wizard uses this so it can ask the question before anything that would
/// otherwise panic on an unconfigured install.
pub fn is_ready() -> bool {
    DB.get().is_some()
}

/// A connection error phrased for whoever is looking at the screen.
///
/// The raw driver message carries the host, the port and sometimes the user, so
/// it is kept — but it is prefixed so the cause is obvious at a glance.
fn describe_connection_failure(err: &DbErr) -> String {
    format!("could not open the database: {err}")
}

/// Connects, migrating, and publishes the connection for a *new* selection.
///
/// Used by the wizard's confirm step. Refuses to replace a connection that is
/// already live: [`DB`] is a `OnceCell` set once per process, so a swap would need
/// a restart rather than pretending to have applied.
pub async fn configure(app_data_dir: &std::path::Path, selection: &DatabaseSelection) -> Result<DatabaseStatus, DbErr> {
    let url = selection.resolved_url(app_data_dir);
    if !selection.backend.accepts_url(&url) {
        return Err(DbErr::Custom(format!(
            "that is not a {}:// URL",
            selection.backend.key()
        )));
    }

    save_selection(app_data_dir, selection)?;

    if is_ready() {
        return Err(DbErr::Custom(
            "the database is already in use; restart the app to switch it".into(),
        ));
    }

    init(app_data_dir, Some(&url)).await?;

    Ok(DatabaseStatus {
        state: "ready",
        backend: Some(selection.backend.key()),
        location: Some(redact_password(&url)),
        message: None,
    })
}

/// Checks a URL without saving or migrating it, so the wizard can validate a
/// typed connection before committing the install to it.
pub async fn probe(url: &str) -> Result<(), DbErr> {
    let conn = Database::connect(connect_options(url)).await.map_err(|e| {
        DbErr::Custom(describe_connection_failure(&e))
    })?;
    conn.ping().await.map_err(|e| DbErr::Custom(describe_connection_failure(&e)))
}

/// The status the wizard shows on open: what is configured, or why nothing is.
pub fn status(app_data_dir: &std::path::Path) -> DatabaseStatus {
    if is_ready() {
        let (backend, url) = resolve_url(app_data_dir)
            .unwrap_or_else(|| (BackendKind::Postgres, DEFAULT_POSTGRES_URL.to_owned()));
        return DatabaseStatus {
            state: "ready",
            backend: Some(backend.key()),
            location: Some(redact_password(&url)),
            message: None,
        };
    }
    match load_selection(app_data_dir) {
        Some(selection) => DatabaseStatus {
            state: "error",
            backend: Some(selection.backend.key()),
            location: Some(selection.redacted_url(app_data_dir)),
            message: Some("the saved database could not be opened; choose another below".into()),
        },
        None => DatabaseStatus {
            state: "unconfigured",
            backend: None,
            location: None,
            message: None,
        },
    }
}


#[cfg(test)]
mod selection_tests {
    use super::*;

    /// A scratch directory per test, so the persisted choice cannot leak between them.
    fn scratch(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "repos_sel_{}_{}_{:?}",
            std::process::id(),
            label,
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn a_password_never_appears_in_what_the_wizard_shows() {
        let shown = redact_password("postgres://shop:hunter2@db.internal:5432/repos");
        assert!(!shown.contains("hunter2"), "leaked: {shown}");
        assert!(shown.contains("shop"), "the user is useful and safe: {shown}");
        assert!(shown.contains("db.internal:5432/repos"), "the host is what identifies it: {shown}");
    }

    #[test]
    fn a_url_with_no_credentials_is_returned_unchanged() {
        let url = "sqlite:///data/repos.db?mode=rwc";
        assert_eq!(redact_password(url), url);
    }

    #[test]
    fn the_backend_is_read_from_its_key() {
        assert_eq!(BackendKind::parse("PostgreSQL").unwrap(), BackendKind::Postgres);
        assert_eq!(BackendKind::parse(" mariadb ").unwrap(), BackendKind::Mysql);
        assert_eq!(BackendKind::parse("sqlite").unwrap(), BackendKind::Sqlite);
        assert!(BackendKind::parse("oracle").is_err());
    }

    #[test]
    fn sqlite_needs_no_url_because_its_file_lives_in_the_app_folder() {
        let dir = scratch("sqlite_url");
        let selection = DatabaseSelection { backend: BackendKind::Sqlite, url: None };
        let url = selection.resolved_url(&dir);
        assert!(url.starts_with("sqlite://"), "{url}");
        assert!(url.contains("repos.db"), "{url}");
    }

    /// The chosen database is what comes back on the next launch — that is the whole
    /// promise the wizard makes, and the reason it persists instead of guessing again.
    #[test]
    fn a_saved_choice_is_read_back_on_the_next_launch() {
        let dir = scratch("roundtrip");
        let selection = DatabaseSelection {
            backend: BackendKind::Postgres,
            url: Some("postgres://shop:pw@localhost:5432/repos".into()),
        };
        save_selection(&dir, &selection).expect("save");
        let read = load_selection(&dir).expect("load");
        assert_eq!(read.backend, BackendKind::Postgres);
        assert_eq!(read.url.as_deref(), Some("postgres://shop:pw@localhost:5432/repos"));
    }

    /// The file holds a password, so it must not be world-readable.
    #[cfg(unix)]
    #[test]
    fn the_saved_choice_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("perms");
        save_selection(
            &dir,
            &DatabaseSelection { backend: BackendKind::Mysql, url: Some("mysql://u:p@h:3306/repos".into()) },
        )
        .expect("save");
        let mode = std::fs::metadata(selection_path(&dir)).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o077, 0, "the choice file is readable by others: {mode:o}");
    }

    /// A URL for the wrong engine is a typo, not something to discover later by
    /// watching a connection fail — so it is refused where it was typed.
    #[test]
    fn a_mysql_url_is_refused_for_postgres() {
        let postgres = BackendKind::Postgres;
        assert!(!postgres.accepts_url("mysql://u:p@h:3306/repos"));
        assert!(postgres.accepts_url("postgres://u:p@h:5432/repos"));
        assert!(BackendKind::Mysql.accepts_url("mysql://u:p@h:3306/repos"));
        assert!(!BackendKind::Mysql.accepts_url("postgres://u:p@h:5432/repos"));
    }

    /// An install with no saved choice reports `unconfigured`, which is what routes
    /// the operator to the database step instead of a dead app.
    #[test]
    fn an_install_with_no_choice_is_unconfigured() {
        let dir = scratch("unconfigured");
        assert!(load_selection(&dir).is_none());
        let status = status(&dir);
        assert_eq!(status.state, "unconfigured");
        assert_eq!(status.backend, None);
    }
}
