use std::collections::HashMap;

use zbus::zvariant::{OwnedValue, Value};

use crate::models::attachment::Attachment;
use crate::models::message::{Address, Message, MessageType};

/// Parse a message from the QDBusVariant map that kdeconnect sends.
///
/// The variant contains a list of maps, each map representing a message.
/// Keys are strings, values are variants.
pub fn parse_message_from_variant(map: &HashMap<String, OwnedValue>) -> Option<Message> {
    let event = get_i32(map, "event").unwrap_or(0x1);
    let body = get_string(map, "body").unwrap_or_default();
    let date = get_i64(map, "date").unwrap_or(0);
    let msg_type_raw = get_i32(map, "type").unwrap_or(1);
    let read = get_i32(map, "read").unwrap_or(0);
    let thread_id = get_i64(map, "threadID").unwrap_or(0);
    let uid = get_i32(map, "uID").unwrap_or(0);
    let sub_id = get_i64(map, "subID").unwrap_or(-1);

    let message_type = MessageType::from_i32(msg_type_raw)?;

    let addresses = parse_addresses(map);
    let attachments = parse_attachments(map);

    Some(Message {
        event,
        body,
        addresses,
        date,
        message_type,
        read: read != 0,
        thread_id,
        uid,
        sub_id,
        attachments,
    })
}

fn parse_addresses(map: &HashMap<String, OwnedValue>) -> Vec<Address> {
    let Some(val) = map.get("addresses") else {
        return Vec::new();
    };
    // addresses comes as an array of dicts with key "address"
    if let Ok(arr) = <Vec<HashMap<String, OwnedValue>>>::try_from(val.clone()) {
        return arr
            .iter()
            .filter_map(|a| get_string(a, "address").map(|s| Address { address: s }))
            .collect();
    }
    Vec::new()
}

fn parse_attachments(map: &HashMap<String, OwnedValue>) -> Vec<Attachment> {
    let Some(val) = map.get("attachments") else {
        return Vec::new();
    };
    if let Ok(arr) = <Vec<HashMap<String, OwnedValue>>>::try_from(val.clone()) {
        return arr
            .iter()
            .filter_map(|a| {
                let part_id = get_i64(a, "partID").unwrap_or(0);
                let mime_type = get_string(a, "mimeType").unwrap_or_default();
                let unique_id = get_string(a, "uniqueIdentifier").unwrap_or_default();
                Some(Attachment {
                    part_id,
                    mime_type,
                    unique_identifier: unique_id,
                    cached_path: None,
                })
            })
            .collect();
    }
    Vec::new()
}

fn get_string(map: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    let val = map.get(key)?;
    match Value::from(val.clone()) {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    }
}

fn get_i32(map: &HashMap<String, OwnedValue>, key: &str) -> Option<i32> {
    let val = map.get(key)?;
    match Value::from(val.clone()) {
        Value::I32(n) => Some(n),
        Value::I64(n) => Some(n as i32),
        Value::U32(n) => Some(n as i32),
        _ => None,
    }
}

fn get_i64(map: &HashMap<String, OwnedValue>, key: &str) -> Option<i64> {
    let val = map.get(key)?;
    match Value::from(val.clone()) {
        Value::I64(n) => Some(n),
        Value::I32(n) => Some(n as i64),
        Value::U64(n) => Some(n as i64),
        Value::U32(n) => Some(n as i64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::OwnedValue;

    fn make_owned(val: impl Into<Value<'static>>) -> OwnedValue {
        val.into().try_into().unwrap()
    }

    fn make_test_message_map() -> HashMap<String, OwnedValue> {
        let mut map = HashMap::new();
        map.insert("event".into(), make_owned(Value::I32(0x1)));
        map.insert("body".into(), make_owned(Value::Str("Hello!".into())));
        map.insert("date".into(), make_owned(Value::I64(1700000000000)));
        map.insert("type".into(), make_owned(Value::I32(1)));
        map.insert("read".into(), make_owned(Value::I32(0)));
        map.insert("threadID".into(), make_owned(Value::I64(42)));
        map.insert("uID".into(), make_owned(Value::I32(100)));
        map.insert("subID".into(), make_owned(Value::I64(-1)));
        map
    }

    #[test]
    fn test_parse_basic_message() {
        let map = make_test_message_map();
        let msg = parse_message_from_variant(&map).unwrap();
        assert_eq!(msg.body, "Hello!");
        assert_eq!(msg.thread_id, 42);
        assert_eq!(msg.uid, 100);
        assert!(msg.is_incoming());
        assert!(!msg.is_group());
        assert!(msg.has_text());
        assert!(!msg.read);
    }

    #[test]
    fn test_parse_invalid_type() {
        let mut map = make_test_message_map();
        map.insert("type".into(), make_owned(Value::I32(99)));
        assert!(parse_message_from_variant(&map).is_none());
    }

    #[test]
    fn test_parse_missing_fields_use_defaults() {
        let map = HashMap::new();
        // With no "type" field, from_i32(1) is default → Some
        // Actually default is 1, so it should work
        // But type defaults to 1 which is valid
        let msg = parse_message_from_variant(&map).unwrap();
        assert_eq!(msg.body, "");
        assert_eq!(msg.thread_id, 0);
        assert!(msg.addresses.is_empty());
    }

    #[test]
    fn test_get_string() {
        let mut map = HashMap::new();
        map.insert("key".into(), make_owned(Value::Str("value".into())));
        assert_eq!(get_string(&map, "key"), Some("value".into()));
        assert_eq!(get_string(&map, "missing"), None);
    }

    #[test]
    fn test_get_i32_from_various_types() {
        let mut map = HashMap::new();
        map.insert("a".into(), make_owned(Value::I32(42)));
        map.insert("b".into(), make_owned(Value::I64(99)));
        assert_eq!(get_i32(&map, "a"), Some(42));
        assert_eq!(get_i32(&map, "b"), Some(99));
    }
}
