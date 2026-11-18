//! Reading boxes a newer version wrote without losing anything on the next save.
//! A box that parses but would drop a field when written back (a newer version's extra field) stays `Unknown`.
use crate::lenient::Lenient;
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::Value;

/// True when every non-null key of `input` is still in `output` (recursively). Scalars are not compared.
pub fn keeps_fields(input: &Value, output: &Value) -> bool {
    match (input, output) {
        (Value::Object(a), Value::Object(b)) => a.iter().all(|(k, v)| v.is_null() || b.get(k).is_some_and(|w| keeps_fields(v, w))),
        (Value::Object(a), _) => a.values().all(Value::is_null),
        (Value::Array(a), Value::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(v, w)| keeps_fields(v, w)),
        (Value::Array(a), _) => a.is_empty(),
        _ => true,
    }
}

/// `deserialize_with` for a list of lenient values: `Known` only when it parses and writes back every field.
pub fn lenient_vec<'de, D, T>(d: D) -> Result<Vec<Lenient<T>>, D::Error>
where D: Deserializer<'de>, T: DeserializeOwned + Serialize {
    let items = Vec::<Value>::deserialize(d)?;
    Ok(items.into_iter().map(|v| match serde_json::from_value::<T>(v.clone()) {
        Ok(t) if serde_json::to_value(&t).is_ok_and(|out| keeps_fields(&v, &out)) => Lenient::Known(t),
        _ => Lenient::Unknown(v),
    }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keeps_fields_ignores_nulls_and_scalar_differences() {
        assert!(keeps_fields(&json!({"a": 3, "b": null}), &json!({"a": 3.0, "c": 1})));
        assert!(!keeps_fields(&json!({"a": {"x": 1}}), &json!({"a": {}})));
        assert!(!keeps_fields(&json!([1, 2]), &json!([1])));
    }
}
