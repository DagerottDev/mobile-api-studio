use crate::{AppState, State};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use core_model::AppError;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor};
use serde::Serialize;
use serde_json::Value;

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_MESSAGES: usize = 1_000;
const MAX_DEPTH: usize = 64;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolInspection {
    pub kind: &'static str,
    pub content_type: String,
    pub byte_size: usize,
    pub raw_base64: String,
    pub messages: Vec<ProtocolMessage>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolMessage {
    pub index: usize,
    pub offset: usize,
    pub length: usize,
    pub compressed: bool,
    pub raw_base64: String,
    pub decoded: Option<Value>,
    pub decode_error: Option<String>,
}

pub fn decode_protocol_body(
    sha256: String,
    content_type: String,
    descriptor_base64: Option<String>,
    message_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<ProtocolInspection, AppError> {
    if content_type.len() > 256 || message_type.as_ref().is_some_and(|name| name.len() > 256) {
        return Err(AppError::new("protocol_input_too_large", "Protocol metadata exceeds 256 bytes.", true));
    }
    let bytes = state.body_store.read_bounded(&sha256, MAX_BYTES).map_err(|error| AppError::storage(error.to_string()))?;
    if bytes.len() > MAX_BYTES {
        return Err(AppError::new("protocol_body_too_large", "Protocol body exceeds 2 MiB.", true));
    }
    inspect_bytes(&bytes, content_type, descriptor_base64.as_deref(), message_type.as_deref())
}

pub fn inspect_bytes(
    bytes: &[u8],
    content_type: String,
    descriptor_base64: Option<&str>,
    message_type: Option<&str>,
) -> Result<ProtocolInspection, AppError> {
    if content_type.len() > 256 || message_type.is_some_and(|name| name.len() > 256) {
        return Err(AppError::new("protocol_input_too_large", "Protocol metadata exceeds 256 bytes.", true));
    }
    if bytes.len() > MAX_BYTES {
        return Err(AppError::new("protocol_body_too_large", "Protocol body exceeds 2 MiB.", true));
    }
    let media_type = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let kind = if media_type == "application/grpc" || media_type.starts_with("application/grpc+") {
        "grpc"
    } else if matches!(media_type.as_str(), "application/x-protobuf" | "application/protobuf" | "application/vnd.google.protobuf") {
        "protobuf"
    } else {
        "raw"
    };
    let mut result = ProtocolInspection {
        kind, content_type, byte_size: bytes.len(), raw_base64: BASE64.encode(bytes),
        messages: Vec::new(), error: None,
    };
    if kind == "raw" { return checked_output(result); }

    let descriptor = match (descriptor_base64, message_type) {
        (None, None) => None,
        (Some(encoded), Some(name)) => match load_descriptor(encoded, name) {
            Ok(descriptor) => Some(descriptor),
            Err(error) => { result.error = Some(error); None }
        },
        _ => { result.error = Some("Provide both a binary FileDescriptorSet and a full message type to decode Protobuf.".into()); None }
    };

    if kind == "protobuf" {
        result.messages.push(make_message(0, 0, bytes, false, descriptor.as_ref()));
        return checked_output(result);
    }
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if result.messages.len() == MAX_MESSAGES {
            result.error = Some("gRPC body exceeds 1,000 messages.".into());
            break;
        }
        if bytes.len() - cursor < 5 {
            result.error = Some(format!("Truncated gRPC frame header at byte {cursor}."));
            break;
        }
        let flag = bytes[cursor];
        if flag > 1 {
            result.error = Some(format!("Invalid gRPC compression flag {flag} at byte {cursor}."));
            break;
        }
        let length = u32::from_be_bytes(bytes[cursor + 1..cursor + 5].try_into().expect("four length bytes")) as usize;
        cursor += 5;
        if length > MAX_BYTES || length > bytes.len() - cursor {
            result.error = Some(format!("Truncated or oversized gRPC message at byte {cursor}: declared {length} bytes, {} available.", bytes.len() - cursor));
            break;
        }
        result.messages.push(make_message(result.messages.len(), cursor, &bytes[cursor..cursor + length], flag == 1, descriptor.as_ref()));
        cursor += length;
    }
    checked_output(result)
}

fn load_descriptor(encoded: &str, name: &str) -> Result<MessageDescriptor, String> {
    if encoded.len() > 4 * MAX_BYTES.div_ceil(3) || name.is_empty() || name.starts_with('.') && name.len() == 1 {
        return Err("Descriptor or message type is too large or empty.".into());
    }
    let bytes = BASE64.decode(encoded).map_err(|error| format!("Invalid descriptor base64: {error}"))?;
    if bytes.len() > MAX_BYTES { return Err("Descriptor exceeds 2 MiB.".into()); }
    let pool = DescriptorPool::decode(bytes.as_slice()).map_err(|error| format!("Invalid FileDescriptorSet: {error}"))?;
    pool.get_message_by_name(name.trim_start_matches('.')).ok_or_else(|| format!("Message type {name} was not found in the FileDescriptorSet."))
}

fn make_message(index: usize, offset: usize, bytes: &[u8], compressed: bool, descriptor: Option<&MessageDescriptor>) -> ProtocolMessage {
    let (decoded, decode_error) = if compressed {
        (None, Some("Compressed gRPC message: decompression is unavailable; raw bytes are shown.".into()))
    } else if let Some(descriptor) = descriptor {
        match DynamicMessage::decode(descriptor.clone(), bytes) {
            Ok(message) => match serde_json::to_value(message) {
                Ok(value) if json_depth(&value, 0) <= MAX_DEPTH => (Some(value), None),
                Ok(_) => (None, Some("Decoded Protobuf exceeds 64 nesting levels.".into())),
                Err(error) => (None, Some(format!("Protobuf JSON conversion failed: {error}"))),
            },
            Err(error) => (None, Some(format!("Invalid Protobuf message: {error}"))),
        }
    } else {
        (None, None)
    };
    ProtocolMessage { index, offset, length: bytes.len(), compressed, raw_base64: BASE64.encode(bytes), decoded, decode_error }
}

fn json_depth(value: &Value, depth: usize) -> usize {
    match value {
        Value::Array(items) => items.iter().map(|item| json_depth(item, depth + 1)).max().unwrap_or(depth + 1),
        Value::Object(items) => items.values().map(|item| json_depth(item, depth + 1)).max().unwrap_or(depth + 1),
        _ => depth,
    }
}

fn checked_output(mut result: ProtocolInspection) -> Result<ProtocolInspection, AppError> {
    let size = serde_json::to_vec(&result).map_err(|error| AppError::new("protocol_output_failed", error.to_string(), true))?.len();
    if size > MAX_BYTES && !result.messages.is_empty() {
        for message in &mut result.messages { message.raw_base64.clear(); }
        let note = "Per-message raw copies were omitted to keep output under 2 MiB; use rawBase64 with message byte offsets.";
        result.error = Some(result.error.map_or_else(|| note.to_owned(), |error| format!("{error} {note}")));
    }
    if serde_json::to_vec(&result).map_err(|error| AppError::new("protocol_output_failed", error.to_string(), true))?.len() > MAX_BYTES {
        return Err(AppError::new("protocol_output_too_large", "Protocol inspection output exceeds 2 MiB; use the raw body view.", true));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;
    use prost_types::{DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet};
    use prost_types::field_descriptor_proto::{Label, Type};

    #[test]
    fn decodes_real_descriptor_and_keeps_malformed_frames_raw() {
        let schema = FileDescriptorSet { file: vec![FileDescriptorProto {
            name: Some("demo.proto".into()), package: Some("demo".into()), syntax: Some("proto3".into()),
            message_type: vec![DescriptorProto {
                name: Some("Widget".into()), field: vec![FieldDescriptorProto {
                    name: Some("id".into()), number: Some(1), label: Some(Label::Optional as i32),
                    r#type: Some(Type::String as i32), ..Default::default()
                }], ..Default::default()
            }], ..Default::default()
        }] };
        let descriptor = BASE64.encode(schema.encode_to_vec());
        let frame = [0, 0, 0, 0, 4, 10, 2, b'h', b'i'];
        let result = inspect_bytes(&frame, "application/grpc+proto".into(), Some(&descriptor), Some("demo.Widget")).unwrap();
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].offset, 5);
        assert_eq!(result.messages[0].decoded.as_ref().unwrap()["id"], "hi");

        let malformed = inspect_bytes(&frame[..8], "application/grpc".into(), None, None).unwrap();
        assert!(malformed.error.as_deref().unwrap().contains("Truncated"));
        assert_eq!(malformed.raw_base64, BASE64.encode(&frame[..8]));
        let compressed = inspect_bytes(&[1, 0, 0, 0, 1, 7], "application/grpc".into(), Some(&descriptor), Some("demo.Widget")).unwrap();
        assert!(compressed.messages[0].decoded.is_none());
        assert!(compressed.messages[0].decode_error.as_deref().unwrap().contains("Compressed"));
    }

    #[test]
    fn hostile_descriptors_and_frame_counts_fail_boundedly() {
        let invalid = inspect_bytes(&[10, 0], "application/protobuf".into(), Some("%%%"), Some("demo.Widget")).unwrap();
        assert!(invalid.error.as_deref().unwrap().contains("Invalid descriptor base64"));
        assert!(invalid.messages[0].decoded.is_none());
        assert_eq!(invalid.messages[0].raw_base64, BASE64.encode([10, 0]));

        let many_frames = vec![0u8; 5 * (MAX_MESSAGES + 1)];
        let bounded = inspect_bytes(&many_frames, "application/grpc".into(), None, None).unwrap();
        assert_eq!(bounded.messages.len(), MAX_MESSAGES);
        assert!(bounded.error.as_deref().unwrap().contains("1,000"));

        let payload = vec![0u8; 1_000_000];
        let mut large_frame = vec![0];
        large_frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        large_frame.extend_from_slice(&payload);
        let large = inspect_bytes(&large_frame, "application/grpc".into(), None, None).unwrap();
        assert!(large.messages[0].raw_base64.is_empty());
        assert_eq!(large.raw_base64, BASE64.encode(&large_frame));
        assert!(large.error.as_deref().unwrap().contains("raw copies"));

        let recursive = FileDescriptorSet { file: vec![FileDescriptorProto {
            name: Some("recursive.proto".into()), package: Some("demo".into()), syntax: Some("proto3".into()),
            message_type: vec![DescriptorProto { name: Some("Node".into()),
                field: vec![FieldDescriptorProto { name: Some("child".into()), number: Some(1),
                    label: Some(Label::Optional as i32), r#type: Some(Type::Message as i32),
                    type_name: Some(".demo.Node".into()), ..Default::default() }], ..Default::default() }],
            ..Default::default()
        }] };
        // Nested payloads become larger than one-byte varints; build valid length prefixes.
        let mut nested = Vec::new();
        for _ in 0..65 {
            let mut outer = vec![10];
            let mut length = nested.len();
            while length >= 128 { outer.push((length as u8) | 0x80); length >>= 7; }
            outer.push(length as u8);
            outer.extend_from_slice(&nested);
            nested = outer;
        }
        let deep = inspect_bytes(&nested, "application/protobuf".into(), Some(&BASE64.encode(recursive.encode_to_vec())), Some("demo.Node")).unwrap();
        assert!(deep.messages[0].decoded.is_none());
        assert!(deep.messages[0].decode_error.as_deref().unwrap().contains("64 nesting"));
    }
}
