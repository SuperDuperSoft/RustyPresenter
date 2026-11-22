use super::*;
use crate::ui::dialogs::settings::testing::{find_any as find, find_all_any as find_all};
use crate::{
    app::{App, test_app},
    model::{ItemKind, ServiceItem},
    ui::{
        SettingsDlg, SlideCell,
        dialogs::{SettingsTab, settings},
        operator::window::testing::{open as open_window, refresh},
    },
};
use slint::{Model, platform::PointerEventButton};
use std::sync::Arc;
use uuid::Uuid;

fn opened(app: &Arc<App>) -> Rc<Operator> {
    let op = open_window(app);
    op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
    slint::platform::update_timers_and_animations();
    refresh(&op);
    op
}

fn d(op: &Operator) -> SectionsDlg<'_> { crate::ui::dialogs::settings_window::win(&op).global::<SectionsDlg>() }

fn click(op: &Operator, id: &str) {
    slint::platform::update_timers_and_animations();
    find(op, id).unwrap_or_else(|| panic!("{id} not found")).mock_single_click(PointerEventButton::Left);
    slint::platform::update_timers_and_animations();
}

fn toasts(op: &Operator) -> Vec<String> {
    find_all(op, "ToastStack::toast").iter().map(|t| t.accessible_label().unwrap_or_default().to_string()).collect()
}

fn hex(c: slint::Color) -> String { format!("#{:02x}{:02x}{:02x}", c.red(), c.green(), c.blue()) }

#[test]
fn sections_page_edits_names_colours_keys_and_labels() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Sections);
    assert_eq!(crate::ui::dialogs::settings::shown(&op), Some(SettingsTab::Sections));
    assert_eq!(crate::ui::dialogs::settings::shown(&op), Some(SettingsTab::Sections), "Songs → Sections and labels");
    assert!(find(&op, "SettingsSections::kind-name").is_some());
    let kinds: Vec<(String, String)> = d(&op).get_kinds().iter().map(|k| (k.usual_name.to_string(), k.usual_key.to_string())).collect();
    assert_eq!(kinds.len(), 8);
    assert_eq!(kinds[1], ("Chorus".to_string(), "C".to_string()));

    d(&op).invoke_name_edited(1, " Refrain ".into());
    d(&op).invoke_key_edited(1, "R".into());
    d(&op).set_picking(1);
    d(&op).invoke_pick("#ff6b70".into());
    assert_eq!(hex(d(&op).get_kinds().row_data(1).unwrap().color), "#ff6b70");
    assert_eq!(d(&op).get_picking(), -1, "the picker closes");
    d(&op).set_new_section("Spontaneous".into());
    d(&op).set_new_family(7);
    d(&op).invoke_add_section();
    assert_eq!(d(&op).get_added().row_data(0).map(|a| (a.name.to_string(), a.family.to_string())), Some(("Spontaneous".into(), "Interlude".into())));
    d(&op).set_new_label("Choir only".into());
    d(&op).invoke_add_label();
    d(&op).invoke_remove_label(0);
    d(&op).set_picking(100 + 3);
    d(&op).invoke_pick("#3ccf91".into());
    let labels: Vec<String> = d(&op).get_labels().iter().map(|l| l.name.to_string()).collect();
    assert_eq!(labels, ["Instrumental", "Repeat", "Spoken Word", "Choir only"]);
    click(&op, "SettingsWindow::save");
    assert_eq!(op.dialog(), None);

    let look = app.data().settings.sections.clone();
    assert_eq!(look.name(SectionKind::Chorus).as_deref(), Some("Refrain"));
    assert_eq!(look.key(SectionKind::Chorus), Some('r'));
    assert_eq!(look.label_color("Choir only").as_deref(), Some("#3ccf91"));
    assert_eq!(SectionKind::Chorus.name(), "Refrain", "in use at once");
    assert_eq!(SectionKind::from_label("Spontaneous").0, SectionKind::Interlude);
    refresh(&op);
    assert_eq!(hex(op.ui.theme_all().get_sec_chorus()), "#ff6b70", "the operator's colours follow");

    // Back to the usual.
    refresh(&op);
    settings::open(&op, SettingsTab::Sections);
    assert_eq!(d(&op).get_kinds().row_data(1).unwrap().name, "Refrain");
    d(&op).invoke_reset();
    click(&op, "SettingsWindow::save");
    assert_eq!(app.data().settings.sections, SectionLook::default());
    refresh(&op);
    assert_eq!(hex(op.ui.theme_all().get_sec_chorus()), "#ff8f93");
}

#[test]
fn a_key_two_kinds_share_is_not_saved() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Sections);
    d(&op).invoke_key_edited(1, "b".into());
    click(&op, "SettingsWindow::save");
    assert!(toasts(&op).contains(&"Chorus and Bridge both use the key B.".to_string()), "{:?}", toasts(&op));
    assert!(settings::is_open(&op), "Settings stays open");
    assert_eq!(app.data().settings.sections, SectionLook::default());
}

#[test]
fn slide_labels_show_on_thumbnails_in_their_colour() {
    let app = test_app();
    let song = Song { id: Uuid::new_v4(), title: "Grace".into(), lyrics: "Verse 1\n{Instrumental}\n\n{Choir}\nla\nChorus\nx".into(), ..Default::default() };
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None };
    {
        let mut data = app.data();
        data.songs.push(song);
        data.services[0].items.push(item.clone());
    }
    let op = opened(&app);
    op.ui.invoke_service_select(item.id.to_string().into());
    let cells: Vec<SlideCell> = op.ui.get_slide_rows().iter().flat_map(|r| r.cells.iter().collect::<Vec<_>>()).collect();
    let tags: Vec<(String, String)> = cells.iter().map(|c| (c.tag.to_string(), hex(c.tag_color))).collect();
    assert_eq!(tags, [("Instrumental".into(), "#c8d36a".into()), ("Choir".into(), "#8d95a8".into()), (String::new(), "#000000".into())],
        "a label not in Settings is grey");
    slint::platform::update_timers_and_animations();
    let shown: Vec<String> = find_all(&op, "Thumb::tag").iter().map(|t| t.accessible_label().unwrap_or_default().to_string()).collect();
    assert_eq!(shown, ["Instrumental", "Choir"]);
}

#[test]
fn credits_on_off_where_and_template() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Songs);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    assert!(g.get_credits_on());
    assert_eq!(g.get_credits(), 0, "first and last slide");
    assert_eq!(g.get_credit_sample(), "Example: Amazing Grace (My Chains Are Gone) · John Newton, Chris Tomlin, Louie Giglio · © 2006 sixsteps Music · CCLI Song #4768151");
    g.set_credit_template("{title} — {artist}".into());
    g.invoke_credit_template_edited();
    assert_eq!(g.get_credit_sample(), "Example: Amazing Grace (My Chains Are Gone) — Chris Tomlin");
    g.set_credits(2);
    click(&op, "SettingsWindow::save");
    let s = app.data().settings.clone();
    assert_eq!((s.credits.as_str(), s.credit_template.as_str()), ("last", "{title} — {artist}"));

    refresh(&op);
    settings::open(&op, SettingsTab::Songs);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    assert_eq!(g.get_credits(), 2);
    g.set_credits_on(false);
    assert_eq!(settings::tab_index(SettingsTab::Songs), 4);
    click(&op, "SettingsWindow::save");
    assert_eq!(app.data().settings.credits, "none");
}
