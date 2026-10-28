//! Deserialisation helpers shared by generated and hand-written bodies.

use std::fmt;
use std::marker::PhantomData;

use serde::de::value::MapAccessDeserializer;
use serde::de::{self, Deserialize, Deserializer, IntoDeserializer, MapAccess, SeqAccess, Visitor};

/// A list field: Bedrock's loader also accepts a single element where a list
/// is declared (vanilla `interact.interactions` as one object, `play_sounds`
/// as one string). Element errors keep their path.
pub fn one_or_many<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    deserializer.deserialize_any(OneOrMany(PhantomData))
}

/// [`one_or_many`] for an optional field (pair with `#[serde(default)]`).
pub fn optional_one_or_many<'de, D, T>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    one_or_many(deserializer).map(Some)
}

struct OneOrMany<T>(PhantomData<T>);

impl<T> OneOrMany<T> {
    fn one<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        T: Deserialize<'de>,
    {
        T::deserialize(deserializer).map(|item| vec![item])
    }
}

impl<'de, T: Deserialize<'de>> Visitor<'de> for OneOrMany<T> {
    type Value = Vec<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a list or a single element")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(items)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Vec<T>, A::Error> {
        Self::one(MapAccessDeserializer::new(map))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Vec<T>, E> {
        Self::one(value.into_deserializer())
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Vec<T>, E> {
        Self::one(value.into_deserializer())
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Vec<T>, E> {
        Self::one(value.into_deserializer())
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Vec<T>, E> {
        Self::one(value.into_deserializer())
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Vec<T>, E> {
        Self::one(value.into_deserializer())
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Vec<T>, E> {
        Self::one(value.into_deserializer())
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Body {
        #[serde(default, deserialize_with = "super::optional_one_or_many")]
        sounds: Option<Vec<String>>,
    }

    #[test]
    fn single_elements_and_lists_both_decode() {
        let body = |value| serde_json::from_value::<Body>(value).unwrap().sounds;
        assert_eq!(body(json!({"sounds": "milk"})), Some(vec!["milk".to_owned()]));
        assert_eq!(body(json!({"sounds": ["a", "b"]})), Some(vec!["a".into(), "b".into()]));
        assert_eq!(body(json!({})), None);
        assert!(serde_json::from_value::<Body>(json!({"sounds": 3})).is_err());
    }
}
