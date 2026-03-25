use std::collections::HashMap;
use std::time::Duration;

use color_eyre::Result;
use tokio::time::timeout;
use tracing::{debug, info};
use zbus::zvariant::{OwnedValue, Structure, Value};
use zbus::Connection;

use crate::dbus::types::parse_message_from_value;
use crate::models::conversation::{sort_by_recent, Conversation};
use crate::models::message::Message;

const KDECONNECT_SERVICE: &str = "org.kde.kdeconnect";
const CONVERSATIONS_INTERFACE: &str = "org.kde.kdeconnect.device.conversations";

/// Timeout for D-Bus method calls.
const DBUS_TIMEOUT: Duration = Duration::from_secs(5);

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
        timeout(DBUS_TIMEOUT, self.connection
            .call_method(
                Some(KDECONNECT_SERVICE),
                self.device_path().as_str(),
                Some(CONVERSATIONS_INTERFACE),
                "requestAllConversationThreads",
                &(),
            ))
            .await
            .map_err(|_| color_eyre::eyre::eyre!("D-Bus call timed out: requestAllConversationThreads"))??;
        info!("Requested all conversation threads");
        Ok(())
    }

    /// Get the list of active conversations (most recent message per thread).
    pub async fn active_conversations(&self) -> Result<Vec<Conversation>> {
        let msg = timeout(DBUS_TIMEOUT, self
            .connection
            .call_method(
                Some(KDECONNECT_SERVICE),
                self.device_path().as_str(),
                Some(CONVERSATIONS_INTERFACE),
                "activeConversations",
                &(),
            ))
            .await
            .map_err(|_| color_eyre::eyre::eyre!("D-Bus call timed out: activeConversations"))??;

        let body = msg.body();
        debug!("activeConversations response signature: {:?}", body.signature());

        // The response is av (array of variants), each variant wrapping a struct
        if let Ok(reply) = body.deserialize::<Vec<OwnedValue>>() {
            debug!("Deserialized {} conversation variants", reply.len());
            let conversations = parse_active_conversations(&reply);
            info!("Got {} active conversations", conversations.len());
            return Ok(conversations);
        }

        // Fallback: try as a single value wrapping an array
        if let Ok(reply) = body.deserialize::<OwnedValue>() {
            if let Ok(vec) = <Vec<OwnedValue>>::try_from(reply.clone()) {
                let conversations = parse_active_conversations(&vec);
                info!("Got {} active conversations (unwrapped)", conversations.len());
                return Ok(conversations);
            }
            let conversations = parse_active_conversations(&[reply]);
            info!("Got {} active conversations (single)", conversations.len());
            return Ok(conversations);
        }

        debug!("Could not deserialize activeConversations response");
        Ok(Vec::new())
    }

    /// Request messages for a specific conversation thread.
    pub async fn request_conversation(
        &self,
        thread_id: i64,
        start: i32,
        end: i32,
    ) -> Result<()> {
        timeout(DBUS_TIMEOUT, self.connection
            .call_method(
                Some(KDECONNECT_SERVICE),
                self.device_path().as_str(),
                Some(CONVERSATIONS_INTERFACE),
                "requestConversation",
                &(thread_id, start, end),
            ))
            .await
            .map_err(|_| color_eyre::eyre::eyre!("D-Bus call timed out: requestConversation"))??;
        debug!("Requested conversation {} (range {}-{})", thread_id, start, end);
        Ok(())
    }

    /// Reply to an existing conversation thread.
    /// If `attachment` is `Some((mime_type, base64_data))`, it is sent as an MMS attachment.
    pub async fn reply_to_conversation(
        &self,
        thread_id: i64,
        message: &str,
        attachment: Option<(&str, &str)>,
    ) -> Result<()> {
        // QVariantList maps to D-Bus type `av` (array of variants)
        let attachments: Vec<Value<'_>> = if let Some((mime, data)) = attachment {
            vec![Value::from(Structure::from((0i64, mime.to_string(), data.to_string(), String::new())))]
        } else {
            Vec::new()
        };
        timeout(DBUS_TIMEOUT, self.connection
            .call_method(
                Some(KDECONNECT_SERVICE),
                self.device_path().as_str(),
                Some(CONVERSATIONS_INTERFACE),
                "replyToConversation",
                &(thread_id, message, &attachments),
            ))
            .await
            .map_err(|_| color_eyre::eyre::eyre!("D-Bus call timed out: replyToConversation"))??;
        info!("Sent reply to thread {}", thread_id);
        Ok(())
    }

    /// Send a message to a new conversation (by address).
    /// If `attachment` is `Some((mime_type, base64_data))`, it is sent as an MMS attachment.
    pub async fn send_without_conversation(
        &self,
        addresses: &[String],
        message: &str,
        attachment: Option<(&str, &str)>,
    ) -> Result<()> {
        // QVariantList maps to D-Bus type `av` (array of variants)
        let attachments: Vec<Value<'_>> = if let Some((mime, data)) = attachment {
            vec![Value::from(Structure::from((0i64, mime.to_string(), data.to_string(), String::new())))]
        } else {
            Vec::new()
        };
        timeout(DBUS_TIMEOUT, self.connection
            .call_method(
                Some(KDECONNECT_SERVICE),
                self.device_path().as_str(),
                Some(CONVERSATIONS_INTERFACE),
                "sendWithoutConversation",
                &(addresses, message, &attachments),
            ))
            .await
            .map_err(|_| color_eyre::eyre::eyre!("D-Bus call timed out: sendWithoutConversation"))??;
        info!("Sent message to {:?}", addresses);
        Ok(())
    }

    /// Request an attachment file from the phone (cache-first via kdeconnect).
    /// The file arrives asynchronously via the `attachmentReceived` signal.
    pub async fn request_attachment_file(
        &self,
        part_id: i64,
        unique_identifier: &str,
    ) -> Result<()> {
        timeout(DBUS_TIMEOUT, self.connection
            .call_method(
                Some(KDECONNECT_SERVICE),
                self.device_path().as_str(),
                Some(CONVERSATIONS_INTERFACE),
                "requestAttachmentFile",
                &(part_id, unique_identifier),
            ))
            .await
            .map_err(|_| color_eyre::eyre::eyre!("D-Bus call timed out: requestAttachmentFile"))??;
        debug!("Requested attachment partID={} uid={}", part_id, unique_identifier);
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
/// Each element is a variant wrapping a struct with positional fields:
///   (event, body, addresses, date, type, read, threadID, uID, subID, attachments)
fn parse_active_conversations(values: &[OwnedValue]) -> Vec<Conversation> {
    let mut conversations_map: HashMap<i64, Conversation> = HashMap::new();

    for val in values {
        if let Some(msg) = parse_message_from_value(val) {
            let thread_id = msg.thread_id;

            let conv = conversations_map
                .entry(thread_id)
                .or_insert_with(|| Conversation::new(thread_id));

            // Determine group status by address count rather than the event
            // bitmask, since Android sets EventMultiTarget on MMS messages
            // even in 1:1 conversations.  MMS address lists include the
            // user's own number, so a 1:1 MMS has 2 addresses (self +
            // other) while a true group has 3+.
            conv.is_group = conv.is_group || msg.addresses.len() > 2;

            let is_newer = conv
                .latest_message
                .as_ref()
                .is_none_or(|existing| msg.date > existing.date);

            if is_newer {
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
    parse_message_from_value(val)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{StructureBuilder, Value, Array, Signature};

    /// Build a conversation variant matching what kdeconnect actually sends:
    /// variant { struct(event, body, addresses, date, type, read, threadID, uID, subID, attachments) }
    fn make_conversation_variant(thread_id: i64, date: i64, body: &str, event: i32) -> OwnedValue {
        make_conversation_variant_addrs(thread_id, date, body, event, &["+15551234"])
    }

    fn make_conversation_variant_addrs(thread_id: i64, date: i64, body: &str, event: i32, addrs: &[&str]) -> OwnedValue {
        let addr_values: Vec<Value<'_>> = addrs.iter().map(|a| {
            Value::Structure(
                StructureBuilder::new()
                    .add_field(Value::Str((*a).into()))
                    .build().unwrap(),
            )
        }).collect();
        let addresses = Value::Array(addr_values.into());
        let attachments: Value<'_> = Value::Array(
            Array::new(&Signature::from_bytes(b"(xsss)").unwrap())
        );

        let structure = Value::Structure(
            StructureBuilder::new()
                .add_field(Value::I32(event))
                .add_field(Value::Str(body.into()))
                .add_field(addresses)
                .add_field(Value::I64(date))
                .add_field(Value::I32(1))       // type: Inbox
                .add_field(Value::I32(0))       // read
                .add_field(Value::I64(thread_id))
                .add_field(Value::I32(1))       // uID
                .add_field(Value::I64(-1))      // subID
                .add_field(attachments)
                .build().unwrap(),
        );

        // Wrap in variant like kdeconnect does
        let variant = Value::Value(Box::new(structure));
        variant.try_into().unwrap()
    }

    #[test]
    fn test_parse_active_conversations_struct_format() {
        let values = vec![
            make_conversation_variant(1, 3000, "newest in thread 1", 0x1),
            make_conversation_variant(2, 2000, "thread 2 message", 0x1),
            make_conversation_variant(1, 1000, "older in thread 1", 0x1),
        ];

        let convos = parse_active_conversations(&values);
        assert_eq!(convos.len(), 2);

        // Sorted by most recent
        assert_eq!(convos[0].thread_id, 1);
        assert_eq!(convos[0].preview_text(), "newest in thread 1");
        assert_eq!(convos[1].thread_id, 2);
        assert_eq!(convos[1].preview_text(), "thread 2 message");
    }

    #[test]
    fn test_parse_active_conversations_group() {
        // A true group MMS has 3+ addresses (self + 2 others)
        let values = vec![
            make_conversation_variant_addrs(1, 1000, "group msg", 0x3, &["+15551111", "+15552222", "+15553333"]),
        ];

        let convos = parse_active_conversations(&values);
        assert_eq!(convos.len(), 1);
        assert!(convos[0].is_group);
    }

    #[test]
    fn test_two_addresses_not_group() {
        // A 1:1 MMS has 2 addresses (self + other) — not a group
        let values = vec![
            make_conversation_variant_addrs(1, 1000, "mms msg", 0x3, &["+15551111", "+15552222"]),
        ];

        let convos = parse_active_conversations(&values);
        assert_eq!(convos.len(), 1);
        assert!(!convos[0].is_group);
    }

    #[test]
    fn test_single_address_not_group_even_with_multitarget_event() {
        // Android sets EventMultiTarget on MMS messages even in 1:1 conversations
        let values = vec![
            make_conversation_variant(1, 1000, "mms msg", 0x3),
        ];

        let convos = parse_active_conversations(&values);
        assert_eq!(convos.len(), 1);
        assert!(!convos[0].is_group);
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
