//! N8: the filter box under each list (`ui/filter.slint`). A row shows while its text holds every typed word, in any
//! order and any letter case; the Slint side asks `matches` for each row.
use super::window::Operator;
use crate::ui::ListFilter;
use slint::ComponentHandle;

/// Whether `text` holds every word of `query` (any order, any letter case). An empty query lets everything through.
pub fn matches(text: &str, query: &str) -> bool {
    let text = text.to_lowercase();
    query.split_whitespace().all(|w| text.contains(&w.to_lowercase()))
}

pub(super) fn wire(op: &Operator) {
    op.ui.global::<ListFilter>().on_matches(|text, query| matches(&text, &query));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_word_in_any_order_and_case() {
        assert!(matches("Amazing Grace", ""));
        assert!(matches("Amazing Grace", "grace"));
        assert!(matches("Amazing Grace", "  GRACE amaz "));
        assert!(!matches("Amazing Grace", "grace how"));
        assert!(matches("Ángel día", "ÁNGEL"), "letter case beyond A–Z");
    }
}
