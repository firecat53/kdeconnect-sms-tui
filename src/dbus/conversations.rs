use std::collections::HashMap;

use color_eyre::Result;
use tracing::{debug, info};
use zbus::zvariant::OwnedValue;
use zbus::Connection;

use crate::dbus::types::parse_message_from_variant;
use crate::models::conversation::{sort_by_recent, Conversation};
use crate::models::message::Message;

const KDECONNECT_SERVICE: &str = "org.kde.kdeconnect";
const CONVERSATIONS_INTERFACE: &str = "org.kde.kdeconnect.device.conversations";

/// Client for the kdeconnect conversations D-Bus interface.
pub struct ConversationsClient {
    connection: Connection,
    device_id: String,
}

impl ConversationsClient {
    pub fn new(connection: Connection, device_id: String) -> Self {
        Self {
            connection,
            device_id,
        }
    }

    fn device_path(&self) -> String {
        format!("/modules/kdeconnect/devices/{}", self.device_id)
    }

    /// Request kdeconnect to fetch all conversation threads from the phone.
    pub async fn request_all_conversation_threads(&self) -> Result<()> {
        self.connection
            .call_method(
                Some(KDECONNECT_SERVICE),
                self.device_path().as_str(),
                Some(CONVERSATIONS_INTERFACE),
                "requestAllConversationThreads",
                &(),
            )
            .await?;
        info!("Requested all conversation threads");
        Ok(())
    }

    /// Get the list of active conversations (most recent message per thread).
    pub async fn active_conversations(&self) -> Result<Vec<Conversation>> {
        let reply: Vec<OwnedValue> = self
            .connection
            .call_method(
                Some(KDECONNECT_SERVICE),
                self.device_path().as_str(),
                Some(CONVERSATIONS_INTERFACE),
                "activeConversations",
                &(),
            )
            .await?
            .body()
            .deserialize()?;

        let conversations = parse_active_conversations(&reply);
        info!("Got {} active conversations", conversations.len());
        Ok(conversations)
    }

    /// Request messages for a specific conversation thread.
    pub async fn request_conversation(
        &self,
        thread_id: i64,
        start: i32,
        end: i32,
    ) -> Result<()> {
        self.connection
            .call_method(
                Some(KDECONNECT_SERVICE),
                self.device_path().as_str(),
                Some(CONVERSATIONS_INTERFACE),
                "requestConversation",
                &(thread_id, start, end),
            )
            .await?;
        debug!("Requested conversation {} (range {}-{})", thread_id, start, end);
        Ok(())
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }
}

/// Parse the response from activeConversations() into our Conversation model.
///
/// Each element in the list is a variant containing a map of message fields
/// representing the most recent message in each conversation thread.
fn parse_active_conversations(values: &[OwnedValue]) -> Vec<Conversation> {
    let mut conversations_map: HashMap<i64, Conversation> = HashMap::new();

    for val in values {
        // Try to parse as a HashMap<String, OwnedValue>
        let map: HashMap<String, OwnedValue> = match val.clone().try_into() {
            Ok(m) => m,
            Err(e) => {
                debug!("Failed to parse conversation variant as map: {}", e);
                continue;
            }
        };

        if let Some(msg) = parse_message_from_variant(&map) {
            let thread_id = msg.thread_id;
            let is_group = msg.is_group();

            let conv = conversations_map
                .entry(thread_id)
                .or_insert_with(|| Conversation::new(thread_id));

            conv.is_group = is_group;

            // Update latest message if this one is newer
            let dominated = conv
                .latest_message
                .as_ref()
                .is_none_or(|existing| msg.date > existing.date);

            if dominated {
                conv.latest_message = Some(msg);
            }
        }
    }

    let mut conversations: Vec<Conversation> = conversations_map.into_values().collect();
    sort_by_recent(&mut conversations);
    conversations
}

/// Parse a single message variant (from conversationUpdated/conversationCreated signals).
pub fn parse_signal_message(val: &OwnedValue) -> Option<Message> {
    let map: HashMap<String, OwnedValue> = val.clone().try_into().ok()?;
    parse_message_from_variant(&map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    fn make_owned(val: impl Into<Value<'static>>) -> OwnedValue {
        val.into().try_into().unwrap()
    }

    fn make_conversation_variant(thread_id: i64, date: i64, body: &str, event: i32) -> OwnedValue {
        let mut map: HashMap<String, Value<'_>> = HashMap::new();
        map.insert("event".into(), Value::I32(event));
        map.insert("body".into(), Value::Str(body.into()));
        map.insert("date".into(), Value::I64(date));
        map.insert("type".into(), Value::I32(1)); // Inbox
        map.insert("read".into(), Value::I32(0));
        map.insert("threadID".into(), Value::I64(thread_id));
        map.insert("uID".into(), Value::I32(1));
        map.insert("subID".into(), Value::I64(-1));

        // Build addresses as array of dicts
        let mut addr_map: HashMap<String, Value<'_>> = HashMap::new();
        addr_map.insert("address".into(), Value::Str("+15551234".into()));
        let addresses = Value::Array(vec![Value::Dict(addr_map.into())].into());
        map.insert("addresses".into(), addresses);

        let dict_val: Value<'_> = Value::Dict(map.into());
        dict_val.try_into().unwrap()
    }

    #[test]
    fn test_parse_active_conversations_basic() {
        let values = vec![
            make_conversation_variant(1, 3000, "newest in thread 1", 0x1),
            make_conversation_variant(2, 2000, "thread 2 message", 0x1),
            make_conversation_variant(1, 1000, "older in thread 1", 0x1),
        ];

        let convos = parse_active_conversations(&values);

        // Should have 2 conversations (threads 1 and 2)
        assert_eq!(convos.len(), 2);

        // Sorted by most recent: thread 1 (date 3000) first
        assert_eq!(convos[0].thread_id, 1);
        assert_eq!(convos[0].preview_text(), "newest in thread 1");

        assert_eq!(convos[1].thread_id, 2);
        assert_eq!(convos[1].preview_text(), "thread 2 message");
    }

    #[test]
    fn test_parse_active_conversations_group() {
        let values = vec![
            make_conversation_variant(1, 1000, "group msg", 0x3), // 0x1 | 0x2 = group+text
        ];

        let convos = parse_active_conversations(&values);
        assert_eq!(convos.len(), 1);
        assert!(convos[0].is_group);
    }

    #[test]
    fn test_parse_active_conversations_empty() {
        let convos = parse_active_conversations(&[]);
        assert!(convos.is_empty());
    }

    #[test]
    fn test_parse_signal_message() {
        let val = make_conversation_variant(5, 5000, "new message!", 0x1);
        let msg = parse_signal_message(&val).unwrap();
        assert_eq!(msg.body, "new message!");
        assert_eq!(msg.thread_id, 5);
    }

    #[test]
    fn test_parse_active_keeps_newest() {
        let values = vec![
            make_conversation_variant(1, 1000, "old", 0x1),
            make_conversation_variant(1, 5000, "new", 0x1),
            make_conversation_variant(1, 3000, "middle", 0x1),
        ];

        let convos = parse_active_conversations(&values);
        assert_eq!(convos.len(), 1);
        assert_eq!(convos[0].preview_text(), "new");
        assert_eq!(convos[0].last_timestamp(), 5000);
    }
}
