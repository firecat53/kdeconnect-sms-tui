use serde::{Deserialize, Serialize};

use super::attachment::Attachment;

/// Message type as reported by Android's SMS database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum MessageType {
    Inbox = 1,
    Sent = 2,
    Draft = 3,
    Outbox = 4,
    Failed = 5,
    Queued = 6,
}

impl MessageType {
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            1 => Some(Self::Inbox),
            2 => Some(Self::Sent),
            3 => Some(Self::Draft),
            4 => Some(Self::Outbox),
            5 => Some(Self::Failed),
            6 => Some(Self::Queued),
            _ => None,
        }
    }
}

/// A single SMS/MMS message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Bitwise: 0x1 = text, 0x2 = multi-target (group)
    pub event: i32,
    pub body: String,
    pub addresses: Vec<Address>,
    /// Unix epoch milliseconds
    pub date: i64,
    pub message_type: MessageType,
    pub read: bool,
    pub thread_id: i64,
    pub uid: i32,
    /// SIM card subscriber ID
    pub sub_id: i64,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Address {
    pub address: String,
}

impl Message {
    pub fn is_incoming(&self) -> bool {
        self.message_type == MessageType::Inbox
    }

    pub fn is_outgoing(&self) -> bool {
        matches!(
            self.message_type,
            MessageType::Sent | MessageType::Outbox | MessageType::Failed | MessageType::Queued
        )
    }

    pub fn is_group(&self) -> bool {
        self.event & 0x2 != 0
    }

    pub fn has_text(&self) -> bool {
        self.event & 0x1 != 0
    }

    pub fn has_attachments(&self) -> bool {
        !self.attachments.is_empty()
    }

    /// Returns the time portion as a local-time formatted string (HH:MM).
    pub fn timestamp_display(&self) -> String {
        let tm = epoch_millis_to_local(self.date);
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }

    /// Returns the date portion as a local-time formatted string (YYYY-MM-DD).
    pub fn date_display(&self) -> String {
        let tm = epoch_millis_to_local(self.date);
        format!(
            "{:04}-{:02}-{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday
        )
    }
}

/// Convert epoch milliseconds to a local-time `libc::tm` struct.
fn epoch_millis_to_local(millis: i64) -> libc::tm {
    let secs = millis / 1000;
    let time_t = secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&time_t, &mut tm);
    }
    tm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_message(event: i32, msg_type: MessageType) -> Message {
        Message {
            event,
            body: "hello".into(),
            addresses: vec![Address {
                address: "+15551234".into(),
            }],
            date: 1700000000000,
            message_type: msg_type,
            read: false,
            thread_id: 1,
            uid: 100,
            sub_id: -1,
            attachments: vec![],
        }
    }

    #[test]
    fn test_incoming() {
        let msg = make_message(0x1, MessageType::Inbox);
        assert!(msg.is_incoming());
        assert!(!msg.is_outgoing());
    }

    #[test]
    fn test_outgoing() {
        let msg = make_message(0x1, MessageType::Sent);
        assert!(msg.is_outgoing());
        assert!(!msg.is_incoming());
    }

    #[test]
    fn test_group_detection() {
        let msg = make_message(0x3, MessageType::Inbox); // 0x1 | 0x2
        assert!(msg.is_group());
        assert!(msg.has_text());
    }

    #[test]
    fn test_not_group() {
        let msg = make_message(0x1, MessageType::Inbox);
        assert!(!msg.is_group());
    }

    #[test]
    fn test_message_type_roundtrip() {
        assert_eq!(MessageType::from_i32(1), Some(MessageType::Inbox));
        assert_eq!(MessageType::from_i32(5), Some(MessageType::Failed));
        assert_eq!(MessageType::from_i32(99), None);
    }

    #[test]
    fn test_has_attachments() {
        let mut msg = make_message(0x1, MessageType::Inbox);
        assert!(!msg.has_attachments());
        msg.attachments.push(Attachment {
            part_id: 1,
            mime_type: "image/jpeg".into(),
            unique_identifier: "abc".into(),
            cached_path: None,
        });
        assert!(msg.has_attachments());
    }
}
