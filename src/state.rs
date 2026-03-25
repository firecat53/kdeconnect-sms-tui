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

        let serialized = toml::to_string_pretty(&state).unwrap();
        let deserialized: AppState = toml::from_str(&serialized).unwrap();

        assert_eq!(
            deserialized.group_names.get("42"),
            Some(&"Family Chat".to_string())
        );
        assert_eq!(deserialized.archived_threads, vec![10, 20]);
        assert_eq!(deserialized.spam_threads, vec![30]);
    }
}
