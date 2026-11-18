//! The Slides panel's quick Key control (`ui/song-key.slint`): − / + move the selected song's chords on the stage
//! screen one semitone, and 0 puts them back as written, without opening the song editor. It saves the song's own
//! `transpose` through `song/key`, so it holds every week; when the song is live the stage screen changes at once
//! (the audience screen never shows chords). `connect` once when the window opens; `render` whenever the selection
//! or the library changes (beside the slides header's other song controls).
use crate::{
    model::{ItemKind, Song},
    ui::operator::window::Operator,
};
use serde_json::{Value, json};
use std::rc::Rc;

/// The song the selected service item plays.
fn selected_song(op: &Operator) -> Option<Song> {
    let st = op.state.borrow();
    let snap = st.snap.as_ref()?;
    let row = st.selected.and_then(|id| snap.items.iter().find(|i| i.id == id))?;
    let ItemKind::Song { song_id, .. } = row.item.kind else { return None };
    snap.data.songs.iter().find(|s| s.id == song_id).cloned()
}

/// What the control shows for the selected item: nothing unless it is a song.
pub fn render(op: &Operator) {
    // Every window: the Slides panel may be in a window of its own (dock panels).
    let g = op.ui.song_key_state_all();
    let song = selected_song(op);
    g.set_shown(song.is_some());
    let text = song.as_ref().map(crate::transpose::describe).filter(|t| !t.is_empty()).unwrap_or_else(|| crate::tr!("As written"));
    g.set_text(text.into());
    g.set_moved(song.is_some_and(|s| s.transpose != 0));
}

/// Saves the change; the control follows the library when the change comes back.
fn change(op: &Rc<Operator>, patch: impl FnOnce(&Song) -> Value) {
    let Some(song) = selected_song(op) else { return };
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("song/key", patch(&song), move |r| {
        let Some(op) = me.upgrade() else { return };
        if let Err(e) = r { op.toast(e, true) }
        render(&op);
    });
}

/// Answers the control's buttons. Call once when the window opens.
pub fn connect(op: &Rc<Operator>) {
    let g = op.ui.song_key_state_all();
    let me = Rc::downgrade(op);
    g.on_step(move |n| if let Some(op) = me.upgrade() { change(&op, |s| json!({ "id": s.id, "step": n.clamp(-11, 11) })) });
    let me = Rc::downgrade(op);
    g.on_reset(move || if let Some(op) = me.upgrade() { change(&op, |s| json!({ "id": s.id, "transpose": 0 })) });
    render(op);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::test_app,
        model::ServiceItem,
        ui::{dialogs::add::tests::{opened, settle}, operator::window::testing::refresh},
    };
    use uuid::Uuid;

    #[test]
    fn the_quick_key_control_moves_the_selected_songs_chords() {
        let app = test_app();
        let song = Song { id: Uuid::new_v4(), title: "Grace".into(), key: "G".into(), lyrics: "Verse 1\n[G]Amazing grace".into(), ..Default::default() };
        let (song_id, item_id, text_id) = (song.id, Uuid::new_v4(), Uuid::new_v4());
        {
            let mut d = app.data.lock().unwrap();
            d.songs.push(song);
            d.services[0].items.push(ServiceItem { id: item_id, kind: ItemKind::Song { song_id, arrangement: None }, background: None, notes: String::new(), theme: None });
            d.services[0].items.push(ServiceItem { id: text_id, kind: ItemKind::Text { title: "Hi".into(), body: "x".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None });
        }
        let op = opened(&app);
        connect(&op);
        let g = op.ui.global::<SongKeyState>();
        op.state.borrow_mut().selected = Some(text_id);
        render(&op);
        assert!(!g.get_shown(), "only for a song");
        op.state.borrow_mut().selected = Some(item_id);
        render(&op);
        assert!(g.get_shown());
        assert_eq!((g.get_text().as_str(), g.get_moved()), ("G", false));
        g.invoke_step(1);
        settle(&op);
        g.invoke_step(1);
        settle(&op);
        refresh(&op);
        render(&op);
        assert_eq!(app.data.lock().unwrap().songs.iter().find(|s| s.id == song_id).unwrap().transpose, 2);
        assert_eq!((g.get_text().as_str(), g.get_moved()), ("G → A, +2", true));
        g.invoke_reset();
        settle(&op);
        refresh(&op);
        render(&op);
        assert_eq!(app.data.lock().unwrap().songs.iter().find(|s| s.id == song_id).unwrap().transpose, 0);
        assert_eq!((g.get_text().as_str(), g.get_moved()), ("G", false));
    }
}
