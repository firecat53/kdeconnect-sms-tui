use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use color_eyre::Result;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Persistent application state (group names, archive/spam lists).
/// Stored in XDG_STATE_HOME/kdeconnect-sms-tui/state.toml.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppState {
    /// Custom group names: thread_id → display name
    #[serde(default)]
    pub group_names: HashMap<String, String>,

    /// Thread IDs hidden in the "Archive" folder.
    #[serde(default)]
    pub archived_threads: Vec<i64>,

    /// Thread IDs hidden in the "Spam" folder.
    #[serde(default)]
    pub spam_threads: Vec<i64>,

    /// Selected theme name (None = default).
    #[serde(default)]
    pub theme: Option<String>,

    /// Thread ID aliases for merged group conversations.
    /// Maps alias_thread_id → canonical_thread_id (both as strings for TOML).
    /// When Android assigns different thread IDs to SMS vs MMS for the same
    /// group, this lets us route all messages to the canonical conversation.
    #[serde(default)]
    pub thread_aliases: HashMap<String, String>,
}

impl AppState {
    pub fn load() -> Result<Self> {
        let path = Self::state_path();
        if !path.exists() {
            debug!("No state file at {:?}, using defaults", path);
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)?;
        let state: AppState = toml::from_str(&content)?;
        debug!("Loaded state from {:?}", path);
        Ok(state)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        debug!("Saved state to {:?}", path);
        Ok(())
    }

    pub fn is_archived(&self, thread_id: i64) -> bool {
        self.archived_threads.contains(&thread_id)
    }

    pub fn is_spam(&self, thread_id: i64) -> bool {
        self.spam_threads.contains(&thread_id)
    }

    pub fn is_hidden(&self, thread_id: i64) -> bool {
        self.is_archived(thread_id) || self.is_spam(thread_id)
    }

    pub fn toggle_archived(&mut self, thread_id: i64) {
        if let Some(pos) = self.archived_threads.iter().position(|&t| t == thread_id) {
            self.archived_threads.remove(pos);
        } else {
            // Remove from spam if moving to archive
            self.spam_threads.retain(|&t| t != thread_id);
            self.archived_threads.push(thread_id);
        }
    }

    pub fn toggle_spam(&mut self, thread_id: i64) {
        if let Some(pos) = self.spam_threads.iter().position(|&t| t == thread_id) {
            self.spam_threads.remove(pos);
        } else {
            // Remove from archive if moving to spam
            self.archived_threads.retain(|&t| t != thread_id);
            self.spam_threads.push(thread_id);
        }
    }

    /// Resolve a thread_id to its canonical ID (following aliases).
    pub fn resolve_thread_id(&self, thread_id: i64) -> i64 {
        self.thread_aliases
            .get(&thread_id.to_string())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(thread_id)
    }

    /// Record that `alias` should be merged into `canonical`.
    pub fn add_thread_alias(&mut self, alias: i64, canonical: i64) {
        self.thread_aliases
            .insert(alias.to_string(), canonical.to_string());
    }

    /// Migrate state entries (group_names, archived, spam) from an alias
    /// thread_id to the canonical one.
    pub fn migrate_alias_state(&mut self, alias: i64, canonical: i64) {
        // Migrate group name (prefer canonical's existing name)
        let alias_key = alias.to_string();
        let canonical_key = canonical.to_string();
        if !self.group_names.contains_key(&canonical_key) {
            if let Some(name) = self.group_names.remove(&alias_key) {
                self.group_names.insert(canonical_key, name);
            }
        } else {
            self.group_names.remove(&alias_key);
        }

        // Migrate archived/spam status
        if self.archived_threads.contains(&alias) {
            self.archived_threads.retain(|&t| t != alias);
        }
        if self.spam_threads.contains(&alias) {
            self.spam_threads.retain(|&t| t != alias);
        }
    }

    /// Remove a thread from both archived and spam lists (restore to inbox).
    pub fn unarchive(&mut self, thread_id: i64) {
        self.archived_threads.retain(|&t| t != thread_id);
        self.spam_threads.retain(|&t| t != thread_id);
    }

    fn state_path() -> PathBuf {
        dirs::state_dir()
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".local/state")
            })
            .join("kdeconnect-sms-tui")
            .join("state.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state() {
        let state = AppState::default();
        assert!(state.group_names.is_empty());
        assert!(state.archived_threads.is_empty());
        assert!(state.spam_threads.is_empty());
        assert!(state.theme.is_none());
    }

    #[test]
    fn test_archive_spam_toggle() {
        let mut state = AppState::default();

        state.toggle_archived(1);
        assert!(state.is_archived(1));
        assert!(state.is_hidden(1));

        // Moving to spam removes from archive
        state.toggle_spam(1);
        assert!(!state.is_archived(1));
        assert!(state.is_spam(1));
        assert!(state.is_hidden(1));

        // Unarchive removes from both
        state.unarchive(1);
        assert!(!state.is_hidden(1));
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut state = AppState::default();
        state.group_names.insert("42".into(), "Family Chat".into());
        state.archived_threads = vec![10, 20];
        state.spam_threads = vec![30];
        state.theme = Some("Dracula".into());

        let serialized = toml::to_string_pretty(&state).unwrap();
        let deserialized: AppState = toml::from_str(&serialized).unwrap();

        assert_eq!(
            deserialized.group_names.get("42"),
            Some(&"Family Chat".to_string())
        );
        assert_eq!(deserialized.archived_threads, vec![10, 20]);
        assert_eq!(deserialized.spam_threads, vec![30]);
        assert_eq!(deserialized.theme, Some("Dracula".into()));
    }

    #[test]
    fn test_thread_alias_resolve() {
        let mut state = AppState::default();
        assert_eq!(state.resolve_thread_id(100), 100); // no alias
        state.add_thread_alias(200, 100);
        assert_eq!(state.resolve_thread_id(200), 100);
        assert_eq!(state.resolve_thread_id(100), 100); // canonical unchanged
    }

    #[test]
    fn test_migrate_alias_state() {
        let mut state = AppState::default();
        state.group_names.insert("200".into(), "BCs".into());
        state.archived_threads.push(200);
        state.migrate_alias_state(200, 100);
        // Name migrated to canonical
        assert_eq!(state.group_names.get("100"), Some(&"BCs".to_string()));
        assert!(!state.group_names.contains_key("200"));
        // Archive entry removed for alias
        assert!(!state.archived_threads.contains(&200));
    }

    #[test]
    fn test_migrate_alias_state_canonical_name_preferred() {
        let mut state = AppState::default();
        state.group_names.insert("100".into(), "Family".into());
        state.group_names.insert("200".into(), "BCs".into());
        state.migrate_alias_state(200, 100);
        // Canonical's existing name is preserved
        assert_eq!(state.group_names.get("100"), Some(&"Family".to_string()));
        assert!(!state.group_names.contains_key("200"));
    }

    #[test]
    fn test_deserialize_without_thread_aliases() {
        let toml_str = r#"
archived_threads = []
spam_threads = []

[group_names]
"#;
        let state: AppState = toml::from_str(toml_str).unwrap();
        assert!(state.thread_aliases.is_empty());
    }

    #[test]
    fn test_deserialize_without_theme_field() {
        let toml_str = r#"
archived_threads = []
spam_threads = []

[group_names]
"#;
        let state: AppState = toml::from_str(toml_str).unwrap();
        assert!(state.theme.is_none());
    }
}
