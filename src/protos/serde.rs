use std::collections::HashMap;

use protobuf::{Message, MessageField, SpecialFields, UnknownValueRef};
use serde::{ser::SerializeMap, Deserialize, Serialize};

use crate::db::UnknownProtoBytes;

pub fn serialize_message_field<T, S: serde::Serializer>(
    field: &MessageField<T>,
    s: S,
) -> Result<S::Ok, S::Error>
where
    T: serde::Serialize,
{
    field.0.serialize(s)
}

pub fn deserialize_message_field<'de, T, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<MessageField<T>, D::Error>
where
    T: serde::Deserialize<'de>,
{
    Ok(MessageField(Option::deserialize(d)?))
}

pub fn serialize_special_fields<S: serde::Serializer>(
    field: &SpecialFields,
    s: S,
) -> Result<S::Ok, S::Error> {
    let map = field
        .unknown_fields()
        .into_iter()
        .collect::<HashMap<_, _>>();
    let mut ser_map = s.serialize_map(Some(map.len()))?;
    for (k, v) in map {
        ser_map.serialize_key(&k)?;
        match v {
            UnknownValueRef::Fixed64(ref v) => ser_map.serialize_value(v),
            UnknownValueRef::Fixed32(ref v) => ser_map.serialize_value(v),
            UnknownValueRef::Varint(ref v) => ser_map.serialize_value(v),
            UnknownValueRef::LengthDelimited(v) => {
                if let Ok(s) = std::str::from_utf8(v) {
                    ser_map.serialize_value(&s)
                } else if let Ok(p) = UnknownProtoBytes::parse_from_bytes(v) {
                    ser_map.serialize_value(&p)
                } else {
                    ser_map.serialize_value(&v)
                }
            }
        }?;
    }
    ser_map.end()
}
