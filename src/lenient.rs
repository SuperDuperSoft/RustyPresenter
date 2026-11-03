//! Stored values that a newer version may have written in a shape this one doesn't know.
//! `Lenient<T>` keeps such a value as raw JSON and writes it back unchanged; `or_default`
//! turns it into the default. Neither ever fails the containing struct's load.
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};

/// A stored value this version understands (`Known`), or one it doesn't, kept as written.
#[derive(Clone, Debug, PartialEq)]
pub enum Lenient<T> { Known(T), Unknown(serde_json::Value) }

impl<T> Lenient<T> {
    pub fn known(&self) -> Option<&T> {
        match self { Lenient::Known(t) => Some(t), Lenient::Unknown(_) => None }
    }

    pub fn known_mut(&mut self) -> Option<&mut T> {
        match self { Lenient::Known(t) => Some(t), Lenient::Unknown(_) => None }
    }
}

impl<T: Default> Default for Lenient<T> {
    fn default() -> Self { Lenient::Known(T::default()) }
}

impl<T> From<T> for Lenient<T> {
    fn from(t: T) -> Self { Lenient::Known(t) }
}

impl<T: Serialize> Serialize for Lenient<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self { Lenient::Known(t) => t.serialize(s), Lenient::Unknown(v) => v.serialize(s) }
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Lenient<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        Ok(match serde_json::from_value::<T>(v.clone()) { Ok(t) => Lenient::Known(t), Err(_) => Lenient::Unknown(v) })
    }
}

/// `deserialize_with` for fields whose bad values should load as the default (a newer
/// version's object, a number where a name was). Fails only if the input isn't JSON at all.
pub fn or_default<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value::<T>(v).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum Shape { Dot, Line { len: u32 } }

    #[test]
    fn known_value_round_trips() {
        let l: Lenient<Shape> = serde_json::from_str(r#"{"type":"line","len":3}"#).unwrap();
        assert_eq!(l, Lenient::Known(Shape::Line { len: 3 }));
        assert_eq!(serde_json::to_string(&l).unwrap(), r#"{"type":"line","len":3}"#);
    }

    #[test]
    fn unknown_value_is_kept_and_written_back_unchanged() {
        let src = r#"{"type":"spiral","turns":7,"extra":[1,2]}"#;
        let l: Lenient<Shape> = serde_json::from_str(src).unwrap();
        assert!(matches!(l, Lenient::Unknown(_)));
        assert_eq!(serde_json::to_value(&l).unwrap(), serde_json::from_str::<serde_json::Value>(src).unwrap());
    }

    #[test]
    fn lenient_never_fails_the_containing_struct() {
        #[derive(Deserialize)] struct Holder { items: Vec<Lenient<Shape>> }
        let h: Holder = serde_json::from_str(r#"{"items":[{"type":"dot"},5,"x",{"type":"line"}]}"#).unwrap();
        assert_eq!(h.items[0], Lenient::Known(Shape::Dot));
        assert!(h.items[1..].iter().all(|i| matches!(i, Lenient::Unknown(_))));
    }

    #[test]
    fn or_default_turns_any_bad_value_into_default() {
        #[derive(Deserialize, Default, Debug, PartialEq)] #[serde(rename_all = "snake_case")]
        enum Place { TopLeft, #[default] #[serde(other)] TopRight }
        #[derive(Deserialize)] struct Holder { #[serde(default, deserialize_with = "or_default")] place: Place }
        let p = |s: &str| serde_json::from_str::<Holder>(s).unwrap().place;
        assert_eq!(p(r#"{"place":"top_left"}"#), Place::TopLeft);
        assert_eq!(p(r#"{"place":"middle"}"#), Place::TopRight);
        assert_eq!(p(r#"{"place":{"anchor":"top_left","width":25,"margin":4}}"#), Place::TopRight);
        assert_eq!(p(r#"{"place":7}"#), Place::TopRight);
        assert_eq!(p(r#"{}"#), Place::TopRight);
    }
}
