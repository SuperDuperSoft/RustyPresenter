//! FEAT-2 message templates (spec §3.3, §5.7): `{Field}` parsing, filling and the built-ins.
use crate::automation::strings;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Where a message banner sits on the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessagePlace {
    Top,
    #[default]
    #[serde(other)]
    Bottom,
}

/// A saved message with `{Field}` boxes to fill in when it is shown.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageTemplate {
    pub id: Uuid,
    pub name: String,
    pub text: String,
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub place: MessagePlace,
    pub hide_after_secs: Option<u32>,
    pub remote: bool,
}

pub const NURSERY_ID: Uuid = Uuid::from_u128(0x52500000_0000_4000_8000_000000000011);
pub const VEHICLE_ID: Uuid = Uuid::from_u128(0x52500000_0000_4000_8000_000000000012);
pub const CUSTOM_ID: Uuid = Uuid::from_u128(0x52500000_0000_4000_8000_000000000013);

/// The "Hide after" choices: Never, 15 s, 30 s, 1 min, 2 min, 5 min.
pub const HIDE_AFTER_CHOICES: [Option<u32>; 6] = [None, Some(15), Some(30), Some(60), Some(120), Some(300)];

/// The longest message that fits the screen, in letters (`chars`), not bytes.
const MAX_CHARS: usize = 200;
/// The longest field name between braces, in letters.
const MAX_FIELD_CHARS: usize = 30;

/// The built-in templates, seeded once: Nursery, Vehicle and Custom.
pub fn builtin() -> Vec<MessageTemplate> {
    let make = |id, name: &str, text: &str| MessageTemplate {
        id, name: name.to_string(), text: text.to_string(), ..MessageTemplate::default()
    };
    vec![
        make(NURSERY_ID, strings::NURSERY_NAME, strings::NURSERY_TEXT),
        make(VEHICLE_ID, strings::VEHICLE_NAME, strings::VEHICLE_TEXT),
        make(CUSTOM_ID, strings::CUSTOM_NAME, strings::CUSTOM_TEXT),
    ]
}

/// A template's fields in order of first appearance, and the stray-brace warning if any.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed { pub fields: Vec<String>, pub warning: Option<&'static str> }

enum Segment { Text(String), Field(String) }

/// Splits a template into literal text and fields. `{{`/`}}` are literal braces; `{` + 1–30
/// letters with no braces + `}` is a field; any other `{` is literal and sets the flag.
fn segments(text: &str) -> (Vec<Segment>, bool) {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut stray = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if (c == '{' || c == '}') && chars.get(i + 1) == Some(&c) {
            lit.push(c);
            i += 2;
            continue;
        }
        if c == '{' {
            let close = chars[i + 1..].iter().position(|&d| d == '{' || d == '}');
            if let Some(n) = close.filter(|&n| chars[i + 1 + n] == '}' && (1..=MAX_FIELD_CHARS).contains(&n)) {
                if !lit.is_empty() { out.push(Segment::Text(std::mem::take(&mut lit))); }
                out.push(Segment::Field(chars[i + 1..i + 1 + n].iter().collect()));
                i += n + 2;
                continue;
            }
            stray = true;
        }
        lit.push(c);
        i += 1;
    }
    if !lit.is_empty() { out.push(Segment::Text(lit)); }
    (out, stray)
}

pub fn parse(text: &str) -> Parsed {
    let (segs, stray) = segments(text);
    let mut fields: Vec<String> = Vec::new();
    for s in segs {
        if let Segment::Field(f) = s && !fields.contains(&f) { fields.push(f); }
    }
    Parsed { fields, warning: stray.then_some(strings::UNMATCHED_BRACE) }
}

/// Line breaks of any kind become one space each.
fn flatten(s: &str) -> String { s.replace("\r\n", " ").replace(['\n', '\r'], " ") }

/// Puts trimmed values in place of the fields. Values are inserted as-is, never parsed again.
/// `Err(field)` for the first field whose value is missing or blank.
fn substitute(text: &str, values: &BTreeMap<String, String>) -> Result<String, String> {
    let (segs, _) = segments(text);
    let mut out = String::new();
    for s in segs {
        match s {
            Segment::Text(t) => out.push_str(&flatten(&t)),
            Segment::Field(f) => match values.get(&f).map(|v| v.trim()).filter(|v| !v.is_empty()) {
                Some(v) => out.push_str(&flatten(v)),
                None => return Err(f),
            },
        }
    }
    Ok(out)
}

/// The message to show: every field filled in (after trimming), line breaks flattened, at most
/// 200 letters. The error is the text to show the volunteer.
pub fn fill(text: &str, values: &BTreeMap<String, String>) -> Result<String, String> {
    let out = substitute(text, values).map_err(|f| strings::fill_in(&f))?;
    if out.chars().count() > MAX_CHARS { return Err(strings::MESSAGE_TOO_LONG.to_string()) }
    Ok(out)
}

/// The Show message preview line: like `fill`, but a missing value is just left empty.
pub fn preview(text: &str, values: &BTreeMap<String, String>) -> String {
    let (segs, _) = segments(text);
    let mut out = String::new();
    for s in segs {
        match s {
            Segment::Text(t) => out.push_str(&flatten(&t)),
            Segment::Field(f) => out.push_str(&flatten(values.get(&f).map_or("", |v| v.trim()))),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::strings::{MESSAGE_TOO_LONG, PRACTICE_WARNING, UNMATCHED_BRACE};
    use std::collections::BTreeMap;

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn template(id: Uuid) -> MessageTemplate {
        builtin().into_iter().find(|t| t.id == id).unwrap()
    }

    #[test]
    fn fields_in_order_without_duplicates() {
        assert_eq!(parse("{A} and {B} and {A}").fields, ["A", "B"]);
        assert_eq!(parse("{A} and {B} and {A}").warning, None);
        assert_eq!(parse(&template(NURSERY_ID).text).fields, ["Number"]);
    }

    #[test]
    fn double_braces_are_literal() {
        let p = parse("{{Number}}");
        assert!(p.fields.is_empty());
        assert_eq!(p.warning, None);
        assert_eq!(fill("{{Number}}", &values(&[])), Ok("{Number}".to_string()));
    }

    #[test]
    fn stray_brace_is_literal_with_warning() {
        let p = parse("50% off {today");
        assert!(p.fields.is_empty());
        assert_eq!(p.warning, Some(UNMATCHED_BRACE));
        assert_eq!(fill("50% off {today", &values(&[])), Ok("50% off {today".to_string()));
        let long = format!("{{{}}}", "x".repeat(31));
        let p = parse(&long);
        assert!(p.fields.is_empty());
        assert_eq!(p.warning, Some(UNMATCHED_BRACE));
        assert_eq!(fill(&long, &values(&[])), Ok(long.clone()));
        assert_eq!(parse(&format!("{{{}}}", "x".repeat(30))).fields, ["x".repeat(30)]);
        assert_eq!(parse("a } b").warning, None);
    }

    #[test]
    fn fill_requires_every_field() {
        let t = template(NURSERY_ID);
        assert_eq!(fill(&t.text, &values(&[("Number", "  ")])), Err("Fill in “Number”.".to_string()));
        assert_eq!(fill(&t.text, &values(&[])), Err("Fill in “Number”.".to_string()));
        assert_eq!(fill(&t.text, &values(&[("Number", " 27 ")])),
                   Ok("Parent of child #27, please come to the nursery.".to_string()));
    }

    #[test]
    fn fill_trims_and_flattens_newlines() {
        let t = template(CUSTOM_ID);
        assert_eq!(fill(&t.text, &values(&[("Message", "  Hello\r\nworld  ")])), Ok("Hello world".to_string()));
        assert_eq!(fill("a\nb\r{X}", &values(&[("X", "c\rd\ne")])), Ok("a b c d e".to_string()));
    }

    #[test]
    fn fill_rejects_over_200_chars() {
        let t = template(CUSTOM_ID);
        assert_eq!(fill(&t.text, &values(&[("Message", &"a".repeat(201))])), Err(MESSAGE_TOO_LONG.to_string()));
        assert_eq!(fill(&t.text, &values(&[("Message", &"a".repeat(200))])), Ok("a".repeat(200)));
    }

    #[test]
    fn fill_counts_letters_not_bytes() {
        let t = template(CUSTOM_ID);
        assert_eq!(fill(&t.text, &values(&[("Message", &"é".repeat(200))])), Ok("é".repeat(200)));
    }

    #[test]
    fn values_are_not_reparsed() {
        let t = template(VEHICLE_ID);
        assert_eq!(fill(&t.text, &values(&[("Plate", "{Plate}}")])),
                   Ok("Vehicle {Plate}}: your lights are on.".to_string()));
        assert_eq!(preview(&t.text, &values(&[("Plate", "{{x}")])), "Vehicle {{x}: your lights are on.");
        assert_eq!(preview(&t.text, &values(&[])), "Vehicle : your lights are on.");
    }

    #[test]
    fn builtin_templates_have_fixed_ids_and_text() {
        let b = builtin();
        let ids: Vec<Uuid> = b.iter().map(|t| t.id).collect();
        assert_eq!(ids, [NURSERY_ID, VEHICLE_ID, CUSTOM_ID]);
        assert_eq!(NURSERY_ID, Uuid::from_u128(0x52500000_0000_4000_8000_000000000011));
        assert_eq!(VEHICLE_ID, Uuid::from_u128(0x52500000_0000_4000_8000_000000000012));
        assert_eq!(CUSTOM_ID, Uuid::from_u128(0x52500000_0000_4000_8000_000000000013));
        let names: Vec<&str> = b.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Nursery", "Vehicle", "Custom"]);
        let texts: Vec<&str> = b.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["Parent of child #{Number}, please come to the nursery.",
                           "Vehicle {Plate}: your lights are on.", "{Message}"]);
        for t in &b {
            assert_eq!(t.place, MessagePlace::Bottom);
            assert_eq!(t.hide_after_secs, None);
            assert!(!t.remote);
        }
        assert_eq!(HIDE_AFTER_CHOICES, [None, Some(15), Some(30), Some(60), Some(120), Some(300)]);
    }

    #[test]
    fn unknown_place_loads_as_bottom() {
        let place = |s: &str| serde_json::from_str::<MessageTemplate>(s).unwrap().place;
        assert_eq!(place(r#"{"place":"top"}"#), MessagePlace::Top);
        assert_eq!(place(r#"{"place":"side"}"#), MessagePlace::Bottom);
        assert_eq!(place(r#"{"place":{"anchor":"top"}}"#), MessagePlace::Bottom);
        assert_eq!(place(r#"{}"#), MessagePlace::Bottom);
        let t = template(NURSERY_ID);
        let back: MessageTemplate = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn practice_warning_is_exact() {
        assert_eq!(PRACTICE_WARNING,
                   "Practice mode is on — this shows only in your preview. End practice to show it in the room.");
    }
}
