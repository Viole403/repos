//! Choosing which database this install runs on.
//!
//! These are the only commands that work before a database exists, so none of them
//! may call [`crate::db::db`]. Everything else assumes a live connection and is
//! unreachable until the wizard has configured one.

use serde::{Deserialize, Serialize};

use crate::db::{self, BackendKind, DatabaseSelection, DatabaseStatus};

fn data_dir() -> Result<std::path::PathBuf, String> {
    // The same directory `setup` uses. Resolved per call rather than captured so a
    // test can point it somewhere harmless.
    if let Ok(dir) = std::env::var("REPOS_APP_DATA_DIR") {
        return Ok(std::path::PathBuf::from(dir));
    }
    let base = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "could not resolve a home directory for the app data".to_owned())?;
    Ok(std::path::PathBuf::from(base).join(".repos"))
}

/// What the wizard needs to render its first step: whether a database is chosen,
/// which one, and if it failed, why.
#[tauri::command]
pub fn database_status() -> Result<DatabaseStatus, String> {
    Ok(db::status(&data_dir()?))
}

/// A URL as typed into the wizard, before it is committed.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseCandidate {
    pub backend: String,
    pub url: Option<String>,
}

impl DatabaseCandidate {
    fn into_selection(self) -> Result<DatabaseSelection, String> {
        let backend = BackendKind::parse(&self.backend).map_err(|e| e.to_string())?;
        // SQLite's location follows the app data directory, so a URL here would be
        // ignored. Rejecting it is clearer than silently discarding what was typed.
        if backend == BackendKind::Sqlite {
            return Ok(DatabaseSelection { backend, url: None });
        }
        let url = self
            .url
            .map(|u| u.trim().to_owned())
            .filter(|u| !u.is_empty())
            .ok_or_else(|| format!("a {} connection needs a URL", backend.key()))?;
        Ok(DatabaseSelection {
            backend,
            url: Some(url),
        })
    }
}

/// Checks a typed connection without saving it, so the wizard can say "that
/// password is wrong" before anything is written to disk.
#[tauri::command]
pub async fn test_database_connection(candidate: DatabaseCandidate) -> Result<(), String> {
    let selection = candidate.into_selection()?;
    let dir = data_dir()?;
    db::probe(&selection.resolved_url(&dir))
        .await
        .map_err(|e| e.to_string())
}

/// Commits the choice: persists it, connects, and runs the migrations. After this
/// the app is on that one database.
#[tauri::command]
pub async fn configure_database(candidate: DatabaseCandidate) -> Result<DatabaseStatus, String> {
    let selection = candidate.into_selection()?;
    let dir = data_dir()?;
    db::configure(&dir, &selection)
        .await
        .map_err(|e| e.to_string())
}

/// The three choices, so the frontend does not hardcode the list twice.
#[derive(Debug, Clone, Serialize)]
pub struct DatabaseOption {
    pub backend: &'static str,
    pub label: &'static str,
    pub detail: &'static str,
    pub needs_url: bool,
}

#[tauri::command]
pub fn database_options() -> Vec<DatabaseOption> {
    vec![
        DatabaseOption {
            backend: "sqlite",
            label: "SQLite",
            detail: "A single file inside the app folder. No server to install — the right choice for one till.",
            needs_url: false,
        },
        DatabaseOption {
            backend: "postgres",
            label: "PostgreSQL",
            detail: "A server you run. Use this for several tills or a branch.",
            needs_url: true,
        },
        DatabaseOption {
            backend: "mysql",
            label: "MySQL",
            detail: "A MySQL or MariaDB server you run.",
            needs_url: true,
        },
    ]
}
