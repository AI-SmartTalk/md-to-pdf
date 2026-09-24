//! The quality record.
//!
//! Every competitor's account history is a list of files. Ours is a list of **what each
//! operation cost**: this contract was compressed on 12 March, the text survived, twelve
//! pages, 96 out of 100. A single verdict is a card shown once and forgotten; kept, they
//! become a record — and a record is the one thing in this product that compounds.
//!
//! Two places cooperate so that no route has to remember anything. `helpers::finish_tool`
//! runs on the worker thread that carries the owner of the request (see `assets::owned_by`)
//! and resolves it to an account through the same index the API keys use;
//! `helpers::deliver_tool` writes the entry, because it is the first place the operation is
//! *complete* — the verdict is attached to the response after `finish_tool` has returned,
//! and a record without the verdict is a list of file names like everybody else's.

use crate::assets::{now_unix, rfc3339};
use crate::types::Verdict;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Entries kept per account. Beyond this the oldest are dropped: a record nobody prunes
/// becomes a directory nobody can list, and the value of the hundredth entry is not the
/// value of the first.
const MAX_ENTRIES: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// Which tool ran — `compress`, `ocr`, `office-to-pdf`, …
    pub tool: String,
    pub at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<usize>,
    /// The whole point of keeping any of this
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    /// Unix seconds, for pruning
    #[serde(skip)]
    at_unix: u64,
}

fn history_dir(account_id: &str) -> PathBuf {
    std::path::Path::new("public")
        .join("accounts")
        .join(account_id)
        .join("history")
}

/// Record what a tool just produced, for whoever owns the work in flight.
///
/// Silent when nobody is signed in: an anonymous visitor leaves no trace, which is both the
/// privacy promise and the reason the free tier needs no consent banner.
pub fn record(response: &crate::types::ToolResponse, tool: &str) {
    let Some(account_id) = response.account.as_deref() else {
        return;
    };

    let asset = response
        .asset
        .as_ref()
        .or_else(|| response.assets.as_ref().and_then(|list| list.first()));

    let entry = Entry {
        id: crate::helpers::random_id("op").unwrap_or_else(|_| format!("op_{}", now_unix())),
        tool: tool.to_string(),
        at: rfc3339(now_unix()),
        // The caller's own file name when we have it, and only then what the tool called
        // its output — a record of "compressed.pdf" ten times over is not a record.
        file: response
            .source_name
            .clone()
            .or_else(|| asset.map(|meta| meta.name.clone())),
        asset_id: asset.map(|meta| meta.id.clone()),
        bytes: asset.map(|meta| meta.bytes),
        pages: response.pages.or_else(|| asset.and_then(|meta| meta.pages)),
        verdict: response.verdict.clone(),
        at_unix: now_unix(),
    };

    let dir = history_dir(account_id);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }

    // A history that fails to write must never fail the document that was produced: this is
    // bookkeeping, and the caller is holding a file they asked for.
    if let Ok(json) = serde_json::to_string(&entry) {
        let _ = fs::write(
            dir.join(format!("{}-{}.json", entry.at_unix, entry.id)),
            json,
        );
    }

    prune(&dir);
}

/// Newest first — which is the only order anyone reads a history in.
pub fn list(account_id: &str, limit: usize) -> Vec<Entry> {
    let Ok(entries) = fs::read_dir(history_dir(account_id)) else {
        return Vec::new();
    };

    let mut names: Vec<_> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    // The file name starts with the timestamp, so sorting the names sorts the entries
    names.sort_unstable_by(|a, b| b.cmp(a));

    names
        .into_iter()
        .take(limit)
        .filter_map(|name| fs::read_to_string(history_dir(account_id).join(name)).ok())
        .filter_map(|raw| serde_json::from_str::<Entry>(&raw).ok())
        .collect()
}

fn prune(dir: &std::path::Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<_> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();

    if names.len() <= MAX_ENTRIES {
        return;
    }

    names.sort_unstable();
    for name in names.iter().take(names.len() - MAX_ENTRIES) {
        let _ = fs::remove_file(dir.join(name));
    }
}

/// Forget everything this account did. Called when the account is deleted, and available on
/// its own because "delete my history" is a request people make without wanting to leave.
pub fn clear(account_id: &str) {
    let _ = fs::remove_dir_all(history_dir(account_id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_serialises_without_its_internal_timestamp() {
        let entry = Entry {
            id: "op_x".to_string(),
            tool: "compress".to_string(),
            at: rfc3339(0),
            file: Some("contrat.pdf".to_string()),
            asset_id: None,
            bytes: Some(1024),
            pages: Some(12),
            verdict: None,
            at_unix: 42,
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"tool\":\"compress\""));
        assert!(json.contains("\"pages\":12"));
        // `at_unix` is a pruning detail; the API answers with the readable date
        assert!(!json.contains("at_unix"));
        // Absent fields do not appear at all, like everywhere else in this API
        assert!(!json.contains("asset_id"));
        assert!(!json.contains("verdict"));
    }
}
