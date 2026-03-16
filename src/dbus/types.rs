use std::collections::HashMap;

use tracing::debug;
use zbus::zvariant::{OwnedValue, Structure, Value};

use crate::models::attachment::Attachment;
use crate::models::message::{Address, Message, MessageType};

/// Parse a message from a D-Bus variant.
///
/// kdeconnect sends messages as structs with fields in this order:
///   (i32 event, string body, array<struct{string}> addresses,
///    i64 date, i32 type, i32 read, i64 threadID,
///    i32 uID, i64 subID, array<struct{...}> attachments)
pub fn parse_message_from_value(val: &OwnedValue) -> Option<Message> {
    let inner = Value::from(val.clone());

    // Unwrap variant wrapper(s) — kdeconnect wraps in QDBusVariant
    let unwrapped = unwrap_variant(inner);

    match unwrapped {
        Value::Structure(structure) => parse_message_from_struct(&structure),
        _ => {
            // Fallback: try as a map (for signals that may use dict format)
            if let Ok(map) = <HashMap<String, OwnedValue>>::try_from(val.clone()) {
                return parse_message_from_map(&map);
            }
            debug!("Cannot parse message: unexpected value type");
            None
        }
    }
}

/// Recursively unwrap Variant wrappers.
fn unwrap_variant(val: Value<'_>) -> Value<'_> {
    match val {
        Value::Value(boxed) => unwrap_variant(*boxed),
        other => other,
    }
}

/// Parse a message from a positional struct as kdeconnect sends over D-Bus.
fn parse_message_from_struct(s: &Structure<'_>) -> Option<Message> {
    let fields = s.fields();

    if fields.len() < 9 {
        debug!("Struct has {} fields, expected at least 9", fields.len());
        return None;
    }

    let event = value_to_i32(&fields[0]).unwrap_or(0x1);
    let body = value_to_string(&fields[1]).unwrap_or_default();
    let addresses = parse_addresses_from_value(&unwrap_variant(fields[2].clone()));
    let date = value_to_i64(&fields[3]).unwrap_or(0);
    let msg_type_raw = value_to_i32(&fields[4]).unwrap_or(1);
    let read = value_to_i32(&fields[5]).unwrap_or(0);
    let thread_id = value_to_i64(&fields[6]).unwrap_or(0);
    let uid = value_to_i32(&fields[7]).unwrap_or(0);
    let sub_id = value_to_i64(&fields[8]).unwrap_or(-1);
    let attachments = if fields.len() > 9 {
        parse_attachments_from_value(&unwrap_variant(fields[9].clone()))
    } else {
        Vec::new()
    };

    let message_type = MessageType::from_i32(msg_type_raw)?;

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

/// Parse addresses from the array of structs: array[struct{string}]
fn parse_addresses_from_value(val: &Value<'_>) -> Vec<Address> {
    let mut result = Vec::new();

    if let Value::Array(arr) = val {
        for item in arr.iter() {
            let unwrapped = unwrap_variant(item.clone());
            match unwrapped {
                Value::Structure(s) => {
                    if let Some(addr) = s.fields().first().and_then(value_to_string) {
                        result.push(Address { address: addr });
                    }
                }
                // Also handle plain strings
                _ => {
                    if let Some(addr) = value_to_string(&unwrapped) {
                        result.push(Address { address: addr });
                    }
                }
            }
        }
    }

    result
}

/// Parse attachments from array of structs:
/// array[struct{i64 partID, string mimeType, string base64, string uniqueId}]
fn parse_attachments_from_value(val: &Value<'_>) -> Vec<Attachment> {
    let mut result = Vec::new();

    if let Value::Array(arr) = val {
        for item in arr.iter() {
            let unwrapped = unwrap_variant(item.clone());
            if let Value::Structure(s) = unwrapped {
                let fields = s.fields();
                if fields.len() >= 4 {
                    result.push(Attachment {
                        part_id: value_to_i64(&fields[0]).unwrap_or(0),
                        mime_type: value_to_string(&fields[1]).unwrap_or_default(),
                        unique_identifier: value_to_string(&fields[3]).unwrap_or_default(),
                        cached_path: None,
                    });
                }
            }
        }
    }

    result
}

/// Parse a message from a named-key map (used by some signal formats).
pub fn parse_message_from_map(map: &HashMap<String, OwnedValue>) -> Option<Message> {
    let event = get_i32(map, "event").unwrap_or(0x1);
    let body = get_string(map, "body").unwrap_or_default();
    let date = get_i64(map, "date").unwrap_or(0);
    let msg_type_raw = get_i32(map, "type").unwrap_or(1);
    let read = get_i32(map, "read").unwrap_or(0);
    let thread_id = get_i64(map, "threadID").unwrap_or(0);
    let uid = get_i32(map, "uID").unwrap_or(0);
    let sub_id = get_i64(map, "subID").unwrap_or(-1);

    let message_type = MessageType::from_i32(msg_type_raw)?;

    let addresses = parse_addresses_from_map(map);
    let attachments = parse_attachments_from_map(map);

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

fn parse_addresses_from_map(map: &HashMap<String, OwnedValue>) -> Vec<Address> {
    let Some(val) = map.get("addresses") else {
        return Vec::new();
    };
    if let Ok(arr) = <Vec<HashMap<String, OwnedValue>>>::try_from(val.clone()) {
        return arr
            .iter()
            .filter_map(|a| get_string(a, "address").map(|s| Address { address: s }))
            .collect();
    }
    Vec::new()
}

fn parse_attachments_from_map(map: &HashMap<String, OwnedValue>) -> Vec<Attachment> {
    let Some(val) = map.get("attachments") else {
        return Vec::new();
    };
    if let Ok(arr) = <Vec<HashMap<String, OwnedValue>>>::try_from(val.clone()) {
        return arr
            .iter()
            .filter_map(|a| {
                Some(Attachment {
                    part_id: get_i64(a, "partID").unwrap_or(0),
                    mime_type: get_string(a, "mimeType").unwrap_or_default(),
                    unique_identifier: get_string(a, "uniqueIdentifier").unwrap_or_default(),
                    cached_path: None,
                })
            })
            .collect();
    }
    Vec::new()
}

fn value_to_string(val: &Value<'_>) -> Option<String> {
    match val {
        Value::Str(s) => Some(s.to_string()),
        Value::Value(boxed) => value_to_string(boxed),
        _ => None,
    }
}

fn value_to_i32(val: &Value<'_>) -> Option<i32> {
    match val {
        Value::I32(n) => Some(*n),
        Value::I64(n) => Some(*n as i32),
        Value::U32(n) => Some(*n as i32),
        Value::Value(boxed) => value_to_i32(boxed),
        _ => None,
    }
}

fn value_to_i64(val: &Value<'_>) -> Option<i64> {
    match val {
        Value::I64(n) => Some(*n),
        Value::I32(n) => Some(*n as i64),
        Value::U64(n) => Some(*n as i64),
        Value::U32(n) => Some(*n as i64),
        Value::Value(boxed) => value_to_i64(boxed),
        _ => None,
    }
}

fn get_string(map: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    let val = map.get(key)?;
    value_to_string(&Value::from(val.clone()))
}

fn get_i32(map: &HashMap<String, OwnedValue>, key: &str) -> Option<i32> {
    let val = map.get(key)?;
    value_to_i32(&Value::from(val.clone()))
}

fn get_i64(map: &HashMap<String, OwnedValue>, key: &str) -> Option<i64> {
    let val = map.get(key)?;
    value_to_i64(&Value::from(val.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{OwnedValue, Value};

    fn make_owned(val: impl Into<Value<'static>>) -> OwnedValue {
        val.into().try_into().unwrap()
    }

    // Test with map format (used by some code paths)
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
    fn test_parse_basic_message_from_map() {
        let map = make_test_message_map();
        let msg = parse_message_from_map(&map).unwrap();
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
        assert!(parse_message_from_map(&map).is_none());
    }

    #[test]
    fn test_parse_missing_fields_use_defaults() {
        let map = HashMap::new();
        let msg = parse_message_from_map(&map).unwrap();
        assert_eq!(msg.body, "");
        assert_eq!(msg.thread_id, 0);
        assert!(msg.addresses.is_empty());
    }

    #[test]
    fn test_value_to_string() {
        assert_eq!(value_to_string(&Value::Str("hi".into())), Some("hi".into()));
        assert_eq!(value_to_string(&Value::I32(42)), None);
    }

    #[test]
    fn test_value_to_i32_from_various_types() {
        assert_eq!(value_to_i32(&Value::I32(42)), Some(42));
        assert_eq!(value_to_i32(&Value::I64(99)), Some(99));
        assert_eq!(value_to_i32(&Value::Str("nope".into())), None);
    }

    #[test]
    fn test_value_to_i64_from_various_types() {
        assert_eq!(value_to_i64(&Value::I64(42)), Some(42));
        assert_eq!(value_to_i64(&Value::I32(99)), Some(99));
    }

    #[test]
    fn test_parse_addresses_from_value() {
        // Simulate array of structs with one string field each
        let addr1 = Value::Structure(
            zbus::zvariant::StructureBuilder::new()
                .add_field(Value::Str("+15551234".into()))
                .build().unwrap(),
        );
        let addr2 = Value::Structure(
            zbus::zvariant::StructureBuilder::new()
                .add_field(Value::Str("+15559876".into()))
                .build().unwrap(),
        );
        let arr = Value::Array(vec![addr1, addr2].into());

        let addresses = parse_addresses_from_value(&arr);
        assert_eq!(addresses.len(), 2);
        assert_eq!(addresses[0].address, "+15551234");
        assert_eq!(addresses[1].address, "+15559876");
    }

    #[test]
    fn test_parse_message_from_struct() {
        let addr = Value::Structure(
            zbus::zvariant::StructureBuilder::new()
                .add_field(Value::Str("+15551234".into()))
                .build().unwrap(),
        );
        let addresses = Value::Array(vec![addr].into());
        let attachments: Value<'_> = Value::Array(
            zbus::zvariant::Array::new(&zbus::zvariant::Signature::from_bytes(b"(xsss)").unwrap())
        );

        let structure = zbus::zvariant::StructureBuilder::new()
            .add_field(Value::I32(0x1))            // event
            .add_field(Value::Str("Hello!".into())) // body
            .add_field(addresses)                    // addresses
            .add_field(Value::I64(1700000000000))   // date
            .add_field(Value::I32(1))               // type (Inbox)
            .add_field(Value::I32(0))               // read
            .add_field(Value::I64(42))              // threadID
            .add_field(Value::I32(100))             // uID
            .add_field(Value::I64(-1))              // subID
            .add_field(attachments)                  // attachments
            .build().unwrap();

        let msg = parse_message_from_struct(&structure).unwrap();
        assert_eq!(msg.body, "Hello!");
        assert_eq!(msg.thread_id, 42);
        assert_eq!(msg.uid, 100);
        assert!(msg.is_incoming());
        assert!(!msg.read);
        assert_eq!(msg.addresses.len(), 1);
        assert_eq!(msg.addresses[0].address, "+15551234");
    }

    #[test]
    fn test_parse_group_message_struct() {
        let addr1 = Value::Structure(
            zbus::zvariant::StructureBuilder::new()
                .add_field(Value::Str("+15551111".into()))
                .build().unwrap(),
        );
        let addr2 = Value::Structure(
            zbus::zvariant::StructureBuilder::new()
                .add_field(Value::Str("+15552222".into()))
                .build().unwrap(),
        );
        let addresses = Value::Array(vec![addr1, addr2].into());
        let attachments: Value<'_> = Value::Array(
            zbus::zvariant::Array::new(&zbus::zvariant::Signature::from_bytes(b"(xsss)").unwrap())
        );

        let structure = zbus::zvariant::StructureBuilder::new()
            .add_field(Value::I32(0x3))            // event: text + group
            .add_field(Value::Str("Group msg".into()))
            .add_field(addresses)
            .add_field(Value::I64(2000))
            .add_field(Value::I32(1))
            .add_field(Value::I32(0))
            .add_field(Value::I64(99))
            .add_field(Value::I32(1))
            .add_field(Value::I64(-1))
            .add_field(attachments)
            .build().unwrap();

        let msg = parse_message_from_struct(&structure).unwrap();
        assert!(msg.is_group());
        assert!(msg.has_text());
        assert_eq!(msg.addresses.len(), 2);
    }

    #[test]
    fn test_unwrap_variant() {
        let inner = Value::I32(42);
        let wrapped = Value::Value(Box::new(inner.clone()));
        let double_wrapped = Value::Value(Box::new(wrapped.clone()));

        assert_eq!(value_to_i32(&unwrap_variant(inner)), Some(42));
        assert_eq!(value_to_i32(&unwrap_variant(wrapped)), Some(42));
        assert_eq!(value_to_i32(&unwrap_variant(double_wrapped)), Some(42));
    }
}
