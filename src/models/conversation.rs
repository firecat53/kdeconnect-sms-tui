use serde::{Deserialize, Serialize};

use super::message::Message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub thread_id: i64,
    /// The most recent message (used for preview)
    pub latest_message: Option<Message>,
    /// All loaded messages, ordered by date ascending
    pub messages: Vec<Message>,
    /// Whether this is a group conversation
    pub is_group: bool,
    /// Custom display name (for group rename)
    pub display_name: Option<String>,
    /// How many messages we've requested so far (for lazy pagination).
    #[serde(default)]
    pub messages_requested: i32,
    /// Total messages available on the phone (from conversationLoaded signal).
    /// `None` means we don't know yet.
    #[serde(default)]
    pub total_messages: Option<u64>,
    /// Whether a page of older messages is currently being fetched.
    #[serde(default)]
    pub loading_more_messages: bool,
    /// Tick counter when loading started, used to detect timeouts.
    #[serde(skip)]
    pub loading_started_tick: Option<u32>,
    /// Other thread_ids that have been merged into this conversation.
    /// Android can assign different thread IDs to SMS vs MMS for the same
    /// group; this tracks the aliases so we can request messages from all.
    #[serde(default)]
    pub alias_thread_ids: Vec<i64>,
}

impl Conversation {
    pub fn new(thread_id: i64) -> Self {
        Self {
            thread_id,
            latest_message: None,
            messages: Vec::new(),
            is_group: false,
            display_name: None,
            messages_requested: 0,
            total_messages: None,
            loading_more_messages: false,
            loading_started_tick: None,
            alias_thread_ids: Vec::new(),
        }
    }

    /// Whether more older messages may be available on the phone.
    pub fn has_more_messages(&self) -> bool {
        match self.total_messages {
            Some(total) => (self.messages_requested as u64) < total,
            // Don't know yet — assume yes if we've requested at least once
            None => true,
        }
    }

    /// Preview text for the conversation list.
    pub fn preview_text(&self) -> &str {
        self.latest_message
            .as_ref()
            .map(|m| m.body.as_str())
            .unwrap_or("")
    }

    /// Timestamp of the most recent message (for sorting).
    pub fn last_timestamp(&self) -> i64 {
        self.latest_message.as_ref().map(|m| m.date).unwrap_or(0)
    }

    /// Primary address (first address from latest message).
    pub fn primary_address(&self) -> Option<&str> {
        self.latest_message
            .as_ref()
            .and_then(|m| m.addresses.first())
            .map(|a| a.address.as_str())
    }
}

/// Sort conversations by most recent first.
pub fn sort_by_recent(conversations: &mut [Conversation]) {
    conversations.sort_by_key(|conv| std::cmp::Reverse(conv.last_timestamp()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::message::{Address, MessageType};

    fn make_conversation(thread_id: i64, date: i64, body: &str) -> Conversation {
        let msg = Message {
            event: 0x1,
            body: body.into(),
            addresses: vec![Address {
                address: "+15551234".into(),
            }],
            date,
            message_type: MessageType::Inbox,
            read: false,
            thread_id,
            uid: 1,
            sub_id: -1,
            attachments: vec![],
        };
        Conversation {
            thread_id,
            latest_message: Some(msg),
            messages: Vec::new(),
            is_group: false,
            display_name: None,
            messages_requested: 0,
            total_messages: None,
            loading_more_messages: false,
            loading_started_tick: None,
            alias_thread_ids: Vec::new(),
        }
    }

    #[test]
    fn test_sort_by_recent() {
        let mut convos = vec![
            make_conversation(1, 1000, "old"),
            make_conversation(2, 3000, "newest"),
            make_conversation(3, 2000, "middle"),
        ];
        sort_by_recent(&mut convos);
        assert_eq!(convos[0].thread_id, 2);
        assert_eq!(convos[1].thread_id, 3);
        assert_eq!(convos[2].thread_id, 1);
    }

    #[test]
    fn test_preview_text() {
        let c = make_conversation(1, 1000, "Hello there");
        assert_eq!(c.preview_text(), "Hello there");
    }

    #[test]
    fn test_empty_conversation() {
        let c = Conversation::new(99);
        assert_eq!(c.preview_text(), "");
        assert_eq!(c.last_timestamp(), 0);
        assert!(c.primary_address().is_none());
    }

    #[test]
    fn test_primary_address() {
        let c = make_conversation(1, 1000, "hi");
        assert_eq!(c.primary_address(), Some("+15551234"));
    }

    #[test]
    fn test_has_more_messages_unknown_total() {
        let c = Conversation::new(1);
        // When total is unknown, assume more are available
        assert!(c.has_more_messages());
    }

    #[test]
    fn test_has_more_messages_not_all_requested() {
        let mut c = Conversation::new(1);
        c.total_messages = Some(100);
        c.messages_requested = 50;
        assert!(c.has_more_messages());
    }

    #[test]
    fn test_has_more_messages_all_requested() {
        let mut c = Conversation::new(1);
        c.total_messages = Some(50);
        c.messages_requested = 50;
        assert!(!c.has_more_messages());
    }

    #[test]
    fn test_has_more_messages_over_requested() {
        let mut c = Conversation::new(1);
        c.total_messages = Some(30);
        c.messages_requested = 50;
        assert!(!c.has_more_messages());
    }
}
