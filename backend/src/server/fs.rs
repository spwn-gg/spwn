//! Server-side file browser for the "pick a directory / file" flows.
//!
//! The browser can't return absolute host paths, so the SPA drives a picker against
//! this endpoint instead of a native dialog. Since `spwn serve` runs on the user's
//! own machine, listing the host filesystem is exactly what the old native dialog did.

use axum::extract::Query;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Deserialize)]
pub struct ListQuery {
    /// Absolute directory to list. Defaults to the user's home dir.
    pub path: Option<String>,
    /// Include regular files (default: directories only, for a folder picker).
    #[serde(default)]
    pub files: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    name: String,
    path: String,
    is_dir: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    /// The (canonicalized) directory being listed.
    path: String,
    /// Its parent, if any (for an "up" affordance).
    parent: Option<String>,
    entries: Vec<Entry>,
}

pub async fn list(Query(q): Query<ListQuery>) -> Result<Json<Listing>, (StatusCode, String)> {
    let dir = match q.path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => directories::BaseDirs::new()
            .map(|b| b.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/")),
    };
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);

    let read = std::fs::read_dir(&dir)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("{}: {e}", dir.display())))?;

    let mut entries: Vec<Entry> = Vec::new();
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        // Hide dotfiles by default to keep the picker uncluttered.
        if name.starts_with('.') {
            continue;
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir && !q.files {
            continue;
        }
        entries.push(Entry {
            name,
            path: e.path().to_string_lossy().into_owned(),
            is_dir,
        });
    }
    // Directories first, then case-insensitive by name.
    entries.sort_by(|a, b| {
        (!a.is_dir, a.name.to_lowercase()).cmp(&(!b.is_dir, b.name.to_lowercase()))
    });

    let parent = dir.parent().map(|p| p.to_string_lossy().into_owned());
    Ok(Json(Listing {
        path: dir.to_string_lossy().into_owned(),
        parent,
        entries,
    }))
}
