//! `song/slides`: one change made from the Slides panel (a slide's words, a split, a new order…) as one step, so
//! Edit → Undo takes it back in one go. Called from `App::do_action`.
use super::{OrderFix, fix_order};
use crate::app::{ActionError, App};
use crate::model::ItemKind;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

pub fn handles(action: &str) -> bool { action == "song/slides" }

/// A service item's order: its own, or the song's usual one again.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ItemOrder { Own(Vec<String>), Usual }

#[derive(Deserialize)]
struct Body {
    song_id: Uuid,
    /// The song's new words.
    #[serde(default)]
    lyrics: Option<String>,
    /// What the new words mean for the orders (a part renamed, split or gone): applied to the song's order and to
    /// every service's own order of this song.
    #[serde(default)]
    fixes: Vec<OrderFix>,
    /// The song's new usual order.
    #[serde(default)]
    song_order: Option<Vec<String>>,
    /// A service item's new order.
    #[serde(default)]
    item_id: Option<Uuid>,
    #[serde(default)]
    item_order: Option<ItemOrder>,
    /// Where each part of `item_id` as it played went (old played place → new), so a song on screen keeps its live
    /// part live (`Live::order_hint`).
    #[serde(default)]
    moved: Option<Vec<Option<usize>>>,
}

pub async fn run(app: &Arc<App>, _action: &str, body: &[u8]) -> Result<Value, ActionError> {
    let b: Body = serde_json::from_slice(body).map_err(|e| ActionError::bad(format!("Bad request: {e}")))?;
    if let (Some(item), Some(moved)) = (b.item_id, b.moved.clone()) {
        app.live().order_hint = Some((item, moved));
    }
    let r = app.edit(|d| {
        let Some(song) = d.songs.iter_mut().find(|s| s.id == b.song_id) else { return Err(crate::tr!("That song is not in the library any more.")) };
        if let Some(l) = &b.lyrics { song.lyrics = l.clone() }
        fix_order(&mut song.arrangement, &b.fixes);
        if let Some(o) = &b.song_order { song.arrangement = o.clone() }
        for item in d.services.iter_mut().flat_map(|s| s.items.iter_mut()) {
            let ItemKind::Song { song_id, arrangement } = &mut item.kind else { continue };
            if *song_id != b.song_id { continue }
            if let Some(own) = arrangement { fix_order(own, &b.fixes) }
            if Some(item.id) == b.item_id {
                match &b.item_order {
                    Some(ItemOrder::Own(o)) => *arrangement = Some(o.clone()),
                    Some(ItemOrder::Usual) => *arrangement = None,
                    None => {}
                }
            }
        }
        Ok(())
    });
    // A hint not used (nothing live changed) is not kept for a later change.
    app.live().order_hint = None;
    r.map_err(ActionError::bad)?;
    Ok(json!({}))
}
