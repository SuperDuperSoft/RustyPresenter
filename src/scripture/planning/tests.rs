use super::*;
use crate::scripture::test_support::{cm, input, labels, short, v};

const T: char = '\u{2009}';

#[test]
fn auto_groups_by_measure_not_chars() {
    let i = input(&[v("John 3:16", &"a".repeat(40)), v("John 3:17", &"b".repeat(40))], None); // 43 chars each composed
    assert_eq!(plan_scripture(&i, &mut cm()).slides.len(), 2);
    assert_eq!(plan_scripture(&i, &mut CharMeasure::with_limits(200, 400)).slides.len(), 1);
}

#[test]
fn manual_break_before_starts_new_slide() {
    let mut i = input(&[v("John 3:16", &short(1)), v("John 3:17", &short(2)), v("John 3:18", &short(3))], None);
    assert_eq!(labels(&plan_scripture(&i, &mut cm())), ["John 3:16–18"]);
    i.breaks.before.insert(VerseId(42, 3, 18));
    assert_eq!(labels(&plan_scripture(&i, &mut cm())), ["John 3:16–17", "John 3:18"]);
}

#[test]
fn join_overrides_auto_break() {
    let mut i = input(&[v("John 3:16", &"a".repeat(40)), v("John 3:17", &"b".repeat(40))], None);
    i.breaks.join.insert(VerseId(42, 3, 17));
    let p = plan_scripture(&i, &mut cm()); // 87 chars: Shrunk, still one slide
    assert_eq!((labels(&p), p.slides[0].split), (vec!["John 3:16–17".to_string()], false));
}

#[test]
fn per_slide_n_with_manual_break() {
    let mut i = input(&(16..=20).map(|n| v(&format!("John 3:{n}"), &short(n))).collect::<Vec<_>>(), None);
    i.per_slide = 2;
    i.breaks.before.insert(VerseId(42, 3, 17));
    assert_eq!(labels(&plan_scripture(&i, &mut cm())), ["John 3:16", "John 3:17–18", "John 3:19–20"]);
}

#[test]
fn per_slide_one_with_join_still_merges() {
    let mut i = input(&[v("John 3:16", &short(1)), v("John 3:17", &short(2)), v("John 3:18", &short(3))], None);
    i.per_slide = 1;
    i.breaks.join.insert(VerseId(42, 3, 17));
    assert_eq!(labels(&plan_scripture(&i, &mut cm())), ["John 3:16–17", "John 3:18"]);
}

#[test]
fn before_on_first_verse_is_ignored() {
    let mut i = input(&[v("John 3:16", &short(1)), v("John 3:17", &short(2))], None);
    i.breaks.before.insert(VerseId(42, 3, 16));
    assert_eq!(labels(&plan_scripture(&i, &mut cm())), ["John 3:16–17"]);
}

#[test]
fn parallel_pairs_verses_by_id() {
    let i = input(&[v("John 3:16", "k16"), v("John 3:17", "k17")], Some(("NIV", &[v("John 3:17", "n17"), v("John 3:16", "n16")])));
    let p = plan_scripture(&i, &mut CharMeasure::with_limits(200, 400));
    assert_eq!(p.slides.len(), 1);
    assert_eq!(p.slides[0].text, format!("¹⁶{T}k16 ¹⁷{T}k17"));
    let s = p.slides[0].second.as_ref().unwrap();
    assert_eq!((s.text.clone(), s.main_code.as_str(), s.second_code.as_str()), (format!("¹⁶{T}n16 ¹⁷{T}n17"), "KJV", "NIV"));
}

#[test]
fn parallel_missing_verse_leaves_column_empty_and_notes_it() {
    let i = input(
        &[v("Acts 8:36", "k36"), v("Acts 8:37", "k37"), v("Acts 8:38", "k38")],
        Some(("NIV", &[v("Acts 8:36", "n36"), v("Acts 8:38", "n38")])),
    );
    let p = plan_scripture(&i, &mut CharMeasure::with_limits(200, 400));
    assert_eq!(p.notes, ["NIV doesn't include Acts 8:37."]);
    let s = &p.slides[0].second.as_ref().unwrap().text;
    assert!(!s.contains("³⁷") && p.slides[0].text.contains("³⁷"));
}

#[test]
fn label_includes_verse_only_in_second_translation() {
    let i = input(&[v("John 3:16", "k16")], Some(("NIV", &[v("John 3:16", "n16"), v("John 3:17", "n17")])));
    let p = plan_scripture(&i, &mut CharMeasure::with_limits(200, 400));
    assert_eq!((p.slides[0].label.as_str(), p.slides[0].reference.as_str()), ("John 3:16–17", "John 3:16–17 (KJV · NIV)"));
    assert_eq!(p.notes, ["KJV doesn't include John 3:17."]);
}

#[test]
fn reference_line_names_both_translations() {
    let one = input(&[v("John 3:16", "a")], None);
    assert_eq!(plan_scripture(&one, &mut cm()).slides[0].reference, "John 3:16 (KJV)");
    let two = input(&[v("John 3:16", "a"), v("John 3:17", "b")], Some(("NIV", &[v("John 3:16", "c"), v("John 3:17", "d")])));
    assert_eq!(plan_scripture(&two, &mut CharMeasure::with_limits(200, 400)).slides[0].reference, "John 3:16–17 (KJV · NIV)");
}

#[test]
fn footer_joins_non_empty_notices() {
    let mut i = input(&[v("John 3:16", "a")], Some(("NIV", &[v("John 3:16", "b")])));
    i.second.as_mut().unwrap().notice = Some("© NIV".into());
    assert_eq!(plan_scripture(&i, &mut cm()).slides[0].footer.as_deref(), Some("© NIV"));
    i.main_notice = Some("© KJV".into());
    assert_eq!(plan_scripture(&i, &mut cm()).slides[0].footer.as_deref(), Some("© KJV · © NIV"));
    i.main_notice = Some("  ".into());
    i.second.as_mut().unwrap().notice = Some(String::new());
    assert_eq!(plan_scripture(&i, &mut cm()).slides[0].footer, None);
}

#[test]
fn discontiguous_labels_stay_exact() {
    let i = input(&[v("Psalm 23:1", "a"), v("Psalm 23:4", "b"), v("Psalm 23:5", "c"), v("Psalm 23:6", "d")], None);
    assert_eq!(labels(&plan_scripture(&i, &mut cm())), ["Psalm 23:1, 4–6"]);
}

#[test]
fn slide_source_set_for_every_slide() {
    let i = input(&[v("John 3:16", &"a".repeat(40)), v("John 3:17", &"b".repeat(40))], None);
    let src: Vec<_> = plan_scripture(&i, &mut cm()).slides.into_iter().map(|s| s.source).collect();
    assert_eq!(
        src,
        [
            Some(SlideSource { first: VerseId(42, 3, 16), last: VerseId(42, 3, 16), part: 0 }),
            Some(SlideSource { first: VerseId(42, 3, 17), last: VerseId(42, 3, 17), part: 0 })
        ]
    );
}

#[test]
fn unparseable_labels_plan_in_order_without_source() {
    let mut i = input(&[v("John 3:16", "a"), v("Odd label", "b"), v("John 3:17", "c")], None);
    i.per_slide = 1;
    let p = plan_scripture(&i, &mut cm());
    assert_eq!(p.slides.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), [format!("¹⁶{T}a"), "b".to_string(), format!("¹⁷{T}c")]);
    assert_eq!(p.slides.iter().map(|s| s.source.is_some()).collect::<Vec<_>>(), [true, false, true]);
    assert_eq!(p.slides[1].label, "Odd label");
    i.per_slide = 0;
    let one = plan_scripture(&i, &mut cm());
    assert_eq!(one.slides[0].source, Some(SlideSource { first: VerseId(42, 3, 16), last: VerseId(42, 3, 17), part: 0 }));
    assert_eq!(one.slides[0].text, format!("¹⁶{T}a b ¹⁷{T}c"));
}

#[test]
fn union_orders_by_id_and_keeps_unlabelled_after_previous() {
    let i = input(&[v("Odd start", "o"), v("John 3:17", "k17"), v("Odd label", "b")], Some(("NIV", &[v("John 3:16", "n16"), v("John 3:17", "n17")])));
    let u = union(&i);
    let got: Vec<_> = u.iter().map(|x| (x.id, x.main.is_some(), x.second.is_some())).collect();
    assert_eq!(
        got,
        [(None, true, false), (Some(VerseId(42, 3, 16)), false, true), (Some(VerseId(42, 3, 17)), true, true), (None, true, false)]
    );
    assert!(plan_scripture(&i, &mut cm()).auto_at.iter().all(|a| !a));
}
