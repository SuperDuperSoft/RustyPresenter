use super::*;

fn c(text: &str) -> Chord { Chord::parse(text).unwrap_or_else(|| panic!("{text} parses")) }

#[test]
fn the_key_plan_gives_each_layer_its_own_f_key() {
    let map = Keymap::default();
    let f = |a: Action| map.keys(a).iter().map(Chord::to_text).collect::<Vec<_>>();
    assert_eq!(f(Action::ClearAll), ["F1"]);
    assert_eq!(f(Action::ClearSlide), ["F2"]);
    assert_eq!(f(Action::ClearMedia), ["F3"]);
    assert_eq!(f(Action::ClearAudio), ["F4"]);
    assert_eq!(f(Action::Logo), ["F5"]);
    assert_eq!(f(Action::ClearOverlays), ["F6"]);
    assert_eq!(f(Action::ClearMessage), ["F7"]);
    assert_eq!(f(Action::ClearTimers), ["F8"]);
    assert_eq!(f(Action::Group(BUILTIN_GROUP)), ["F9"]);
    assert_eq!(f(Action::FindMouse), ["Alt+M"]);
    assert_eq!(f(Action::ShowMessage), ["Ctrl+Shift+M"]);
    // No key is used twice by default.
    let all: Vec<&Chord> = map.entries().iter().flat_map(|(_, k)| k).collect();
    for (i, k) in all.iter().enumerate() { assert!(!all[i + 1..].contains(k), "{k:?} twice") }
}

#[test]
fn chords_read_back_what_they_write() {
    for text in ["F2", "Ctrl+Shift+M", "Alt+M", "Space", "Right", "PageDown", "Enter", "X", "Ctrl+Alt+Shift+F12", "Plus", "Ctrl+7"] {
        assert_eq!(c(text).to_text(), text);
    }
    assert_eq!(c("Ctrl+B"), Chord::new("b", true, false, false));
    assert_eq!(c("Space").key, " ");
    assert_eq!(c("Right").key, "ArrowRight");
    assert_eq!(Chord::parse(""), None);
    assert_eq!(Chord::parse("Ctrl+"), None);
    assert_eq!(Chord::parse("Banana"), None);
}

#[test]
fn labels_read_like_the_help_table() {
    let map = Keymap::default();
    assert_eq!(labels(map.keys(Action::Next)), "→  ↓  Space  Page Down");
    assert_eq!(labels(map.keys(Action::Back)), "←  ↑  Page Up");
    assert_eq!(labels(map.keys(Action::FocusBible)), "Ctrl + B");
    assert_eq!(labels(map.keys(Action::ShowMessage)), "Ctrl + Shift + M");
    assert_eq!(labels(map.keys(Action::FindMouse)), "Alt + M");
    assert_eq!(labels(&[]), "");
}

#[test]
fn slint_parts_name_keys_as_slint_does() {
    assert_eq!(c("Ctrl+Shift+M").slint_parts(), ["Control", "Shift", "m"]);
    assert_eq!(c("F9").slint_parts(), ["F9"]);
    assert_eq!(c("Right").slint_parts(), ["RightArrow"]);
    assert_eq!(c("Alt+Enter").slint_parts(), ["Alt", "Return"]);
    for (_, keys) in Keymap::default().entries() {
        for k in keys { assert!(slint::Keys::from_parts(k.slint_parts().iter().map(String::as_str)).is_ok(), "{k:?}") }
    }
}

#[test]
fn ids_round_trip() {
    for (a, _) in Keymap::default().entries() { assert_eq!(Action::from_id(&a.id()), Some(*a)) }
    assert_eq!(Action::from_id("nope"), None);
}

#[test]
fn a_change_is_stored_only_when_it_differs_from_the_default() {
    let mut s = KeySettings::default();
    s.set(Action::ClearSlide, vec![c("F10")]);
    assert_eq!(s.changed, [Binding { action: "clear_slide".into(), keys: vec!["F10".into()] }]);
    assert_eq!(Keymap::new(&s).keys(Action::ClearSlide), [c("F10")]);
    s.set(Action::ClearSlide, vec![c("F2")]);
    assert!(s.changed.is_empty(), "back to the default is no change");
    s.set(Action::Logo, vec![]);
    assert_eq!(Keymap::new(&s).keys(Action::Logo), [] as [Chord; 0], "no key at all is a change");
    s.reset(Action::Logo);
    assert_eq!(s, KeySettings::default());
}

#[test]
fn assigning_a_used_key_takes_it_off_the_other_action() {
    let mut s = KeySettings::default();
    s.assign(Action::ClearMessage, c("F2"));
    let map = Keymap::new(&s);
    assert_eq!(map.keys(Action::ClearMessage), [c("F2")]);
    assert!(map.keys(Action::ClearSlide).is_empty());
    assert_eq!(map.action_for(&c("F2")), Some(Action::ClearMessage));
    // One of several keys: the others stay.
    s.assign(Action::Take, c("Space"));
    let map = Keymap::new(&s);
    assert_eq!(labels(map.keys(Action::Next)), "→  ↓  Page Down");
    assert_eq!(map.action_for(&c("Space")), Some(Action::Take));
}

#[test]
fn shift_falls_back_to_the_key_without_it() {
    let map = Keymap::default();
    assert_eq!(map.action_for(&Chord::new("ArrowRight", false, false, true)), Some(Action::Next));
    assert_eq!(map.action_for(&Chord::new("m", true, false, true)), Some(Action::ShowMessage));
    assert_eq!(map.action_for(&Chord::new("m", true, false, false)), None, "Ctrl+M alone is not Show message");
    assert_eq!(map.action_for(&Chord::new("z", true, false, true)), Some(Action::Undo));
    assert_eq!(map.action_for(&Chord::new("M", false, true, false)), Some(Action::FindMouse), "case does not matter");
}

#[test]
fn menu_keys_are_never_typing_keys() {
    let mut s = KeySettings::default();
    s.set(Action::ClearSlide, vec![c("X"), c("F10")]);
    s.set(Action::Logo, vec![c("L")]);
    let map = Keymap::new(&s);
    assert_eq!(map.menu_key(Action::ClearSlide), Some(&c("F10")));
    assert_eq!(map.menu_key(Action::Logo), None);
    assert_eq!(map.menu_key(Action::Next), None);
    assert_eq!(map.menu_key(Action::Undo), Some(&c("Ctrl+Z")));
}

#[test]
fn clashes_name_what_the_key_already_does() {
    let map = Keymap::default();
    let usual = crate::sections::SectionLook::default();
    let on = Some(&usual);
    assert_eq!(clashes(&map, Action::ClearMessage, &c("F2"), on), [Clash::Action(Action::ClearSlide)]);
    assert_eq!(clashes(&map, Action::ClearSlide, &c("F2"), on), [], "its own key");
    assert_eq!(clashes(&map, Action::Logo, &c("C"), on), [Clash::SectionKey]);
    assert_eq!(clashes(&map, Action::Logo, &c("C"), None), [], "section keys are off");
    assert_eq!(clashes(&map, Action::Logo, &c("Alt+2"), on), [Clash::BibleChip]);
    assert_eq!(clashes(&map, Action::Logo, &c("Ctrl+C"), on), [Clash::TextEditing]);
    assert_eq!(clashes(&map, Action::Undo, &c("Ctrl+V"), on), [Clash::Action(Action::PasteLink), Clash::TextEditing]);
    assert_eq!(clashes(&map, Action::Logo, &c("F11"), on), []);
    assert!(reserved(&c("Escape")) && reserved(&c("Tab")) && !reserved(&c("F1")));
}

#[test]
fn section_key_clashes_use_the_churchs_own_keys() {
    use crate::sections::{SectionKind, SectionLook, look::KindLook};
    let map = Keymap::default();
    let mut look = SectionLook::default();
    look.kinds.push(KindLook { kind: SectionKind::Chorus, name: String::new(), color: String::new(), key: "r".into() });
    assert_eq!(clashes(&map, Action::Logo, &c("R"), Some(&look)), [Clash::SectionKey], "the church's Chorus key");
    assert_eq!(clashes(&map, Action::Logo, &c("C"), Some(&look)), [], "C is free now");
    assert_eq!(clashes(&map, Action::Logo, &c("7"), Some(&look)), [Clash::SectionKey], "verse numbers stay");
}

#[test]
fn groups_list_after_the_clear_keys_and_lose_their_keys_when_deleted() {
    let mut s = KeySettings::default();
    let mine = ClearGroup { id: Uuid::new_v4(), name: "Just words".into(), layers: vec![ClearLayer::Slide, ClearLayer::Message] };
    s.clear_groups.push(mine.clone());
    s.set(Action::Group(mine.id), vec![c("F10")]);
    let map = Keymap::new(&s);
    let order: Vec<Action> = map.entries().iter().map(|(a, _)| *a).collect();
    assert_eq!(order[11..14], [Action::ClearTimers, Action::Group(BUILTIN_GROUP), Action::Group(mine.id)]);
    assert_eq!(map.action_for(&c("F10")), Some(Action::Group(mine.id)));
    s.clear_groups.retain(|g| g.id != mine.id);
    assert_eq!(Keymap::new(&s).action_for(&c("F10")), None);
    assert_eq!(mine.command(), Command::ClearLayers { layers: vec![ClearLayer::Slide, ClearLayer::Message] });
}

#[test]
fn settings_without_keys_load_the_defaults_and_bad_keys_load_too() {
    let s: KeySettings = serde_json::from_str("{}").unwrap();
    assert_eq!(s, KeySettings::default());
    assert_eq!(s.clear_groups[0].name, "Everything but the background");
    let odd: KeySettings = serde_json::from_str(r#"{"changed":[{"action":"clear_slide","keys":["Banana","F10"]}]}"#).unwrap();
    assert_eq!(Keymap::new(&odd).keys(Action::ClearSlide), [c("F10")], "an unreadable key is skipped");
}

#[test]
fn edit_keys_are_the_windows_ones_and_clash_with_no_default_shortcut() {
    let map = Keymap::default();
    let f = |e: EditKey| e.chords().iter().map(Chord::to_text).collect::<Vec<_>>();
    assert_eq!(f(EditKey::Delete), ["Delete", "Backspace"]);
    assert_eq!(f(EditKey::Copy), ["Ctrl+C"]);
    assert_eq!(f(EditKey::Cut), ["Ctrl+X"]);
    assert_eq!(f(EditKey::Duplicate), ["Ctrl+D"]);
    assert_eq!(f(EditKey::MoveUp), ["Alt+Up", "Ctrl+Up"]);
    assert_eq!(f(EditKey::MoveDown), ["Alt+Down", "Ctrl+Down"]);
    assert_eq!(f(EditKey::NewService), ["Ctrl+N"]);
    assert_eq!(f(EditKey::OpenService), ["Ctrl+O"]);
    assert_eq!(f(EditKey::Print), ["Ctrl+P"]);
    assert_eq!(f(EditKey::Settings), ["Ctrl+,"]);
    for e in EditKey::ALL {
        for k in e.chords() {
            assert_eq!(map.action_for(&k), None, "{k:?}");
            assert_eq!(EditKey::of(&k), Some(e));
            assert!(slint::Keys::from_parts(k.slint_parts().iter().map(String::as_str)).is_ok(), "{k:?}");
        }
        assert_eq!(e.menu_key(&map), e.chords().first().cloned());
    }
    assert_eq!(EditKey::of(&c("Up")), None, "a plain arrow is Back");
    // A church that gave Delete to a shortcut keeps it: the menu item lets the key go.
    let mut s = KeySettings::default();
    s.assign(Action::ClearAll, c("Delete"));
    assert_eq!(EditKey::Delete.menu_key(&Keymap::new(&s)), None);
}

#[test]
fn livestream_scenes_have_no_key_until_the_church_gives_one() {
    use crate::stream::scene::Scene;
    let map = Keymap::default();
    for s in Scene::ALL { assert!(map.keys(Action::Scene(s)).is_empty(), "{s:?}") }
    assert_eq!(Action::Scene(Scene::CameraWords).id(), "scene:camera_words");
    let ours = Uuid::from_u128(77);
    assert_eq!(Action::from_id(&format!("scene:{ours}")), Some(Action::Scene(Scene::Own(ours))));
    assert_eq!(Action::from_id("scene:camera_words"), Some(Action::Scene(Scene::CameraWords)));
    assert_eq!(Action::from_id("scene:nonsense"), None);
    // A key for one of the church's scenes works without the list of scenes; the editor lists them all.
    let mut s = KeySettings::default();
    s.set(Action::Scene(Scene::Own(ours)), vec![Chord::parse("Ctrl+1").unwrap()]);
    s.set(Action::Scene(Scene::Hold), vec![Chord::parse("F11").unwrap()]);
    let map = Keymap::new(&s);
    assert_eq!(map.action_for(&Chord::parse("Ctrl+1").unwrap()), Some(Action::Scene(Scene::Own(ours))));
    assert_eq!(map.action_for(&Chord::parse("F11").unwrap()), Some(Action::Scene(Scene::Hold)));
    assert_eq!(Action::Scene(Scene::Hold).reach(), Reach::Anywhere);
    let other = Uuid::from_u128(78);
    let all = Keymap::with_scenes(&s, &[other, ours]);
    let listed: Vec<Action> = all.entries().iter().map(|(a, _)| *a).filter(|a| matches!(a, Action::Scene(Scene::Own(_)))).collect();
    assert_eq!(listed, [Action::Scene(Scene::Own(other)), Action::Scene(Scene::Own(ours))], "each once");
    let back: KeySettings = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
    assert_eq!(back, s);
}
