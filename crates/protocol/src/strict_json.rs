use std::fmt;

use serde::Deserialize;
use serde::de::{self, DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use thiserror::Error;

/// Payload-free strict JSON parsing failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum StrictJsonError {
    #[error("message body is not valid strict JSON")]
    Invalid,
    #[error("message root must be an object")]
    NonObject,
}

pub(crate) fn parse_object(body: &[u8]) -> Result<Map<String, Value>, StrictJsonError> {
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let value =
        StrictValue::deserialize(&mut deserializer).map_err(|_| StrictJsonError::Invalid)?;
    deserializer.end().map_err(|_| StrictJsonError::Invalid)?;
    match value.0 {
        Value::Object(object) => Ok(object),
        _ => Err(StrictJsonError::NonObject),
    }
}

struct StrictValue(Value);

impl<'de> serde::Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("one strict JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .map(StrictValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_string(value.to_owned())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        StrictValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<StrictValue>()? {
            values.push(value.0);
        }
        Ok(StrictValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(A::Error::custom("duplicate JSON object member"));
            }
            let value = object.next_value_seed(StrictValueSeed)?;
            values.insert(key, value.0);
        }
        Ok(StrictValue(Value::Object(values)))
    }
}

struct StrictValueSeed;

impl<'de> DeserializeSeed<'de> for StrictValueSeed {
    type Value = StrictValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        StrictValue::deserialize(deserializer)
    }
}

#[cfg(test)]
mod tests {
    use super::{StrictJsonError, parse_object};

    #[test]
    fn rejects_duplicate_members_at_every_depth() {
        assert_eq!(
            parse_object(br#"{"kind":"command","kind":"event"}"#),
            Err(StrictJsonError::Invalid)
        );
        assert_eq!(
            parse_object(br#"{"payload":{"limit":1,"limit":2}}"#),
            Err(StrictJsonError::Invalid)
        );
    }

    #[test]
    fn rejects_non_objects_invalid_utf8_and_trailing_data() {
        assert_eq!(parse_object(br"[]"), Err(StrictJsonError::NonObject));
        assert_eq!(parse_object(&[0xff]), Err(StrictJsonError::Invalid));
        assert_eq!(parse_object(br"{} {}"), Err(StrictJsonError::Invalid));
    }
}
