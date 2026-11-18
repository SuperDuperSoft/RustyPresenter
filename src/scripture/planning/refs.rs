//! Slide labels from verse labels. A copy of `slides::join_refs` (commit `ba73280`) so Part A
//! does not import `slides`, which Plan 3/4 rewrite; Part B Task B3 makes that one delegate here.
use crate::model::VerseText;

/// "John 3:16", "John 3:16–18", "Psalm 23:1, 4–6", "John 1:51–2:2".
/// Labels that do not parse as `book c:v` make the whole label a plain `", "` list.
pub fn join_refs(group: &[&VerseText]) -> String {
    let parsed: Vec<(&str, u32, u32)> = group
        .iter()
        .filter_map(|v| {
            let (book, cv) = v.reference.rsplit_once(' ')?;
            let (c, vs) = cv.split_once(':')?;
            Some((book, c.parse().ok()?, vs.parse().ok()?))
        })
        .collect();
    if parsed.len() != group.len() || parsed.is_empty() {
        return group.iter().map(|v| v.reference.as_str()).collect::<Vec<_>>().join(", ");
    }
    let book = parsed[0].0;
    // runs of consecutive verses
    let mut runs: Vec<((u32, u32), (u32, u32))> = vec![];
    for &(_, c, v) in &parsed {
        if let Some(last) = runs.last_mut() {
            let (lc, lv) = last.1;
            if (c == lc && v == lv + 1) || (c == lc + 1 && v == 1) {
                last.1 = (c, v);
                continue;
            }
        }
        runs.push(((c, v), (c, v)));
    }
    let mut out = String::new();
    let mut chapter = 0;
    for (i, (a, b)) in runs.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        if a.0 != chapter {
            out.push_str(&format!("{}:", a.0));
            chapter = a.0;
        }
        out.push_str(&a.1.to_string());
        if a != b {
            if b.0 != a.0 {
                out.push_str(&format!("–{}:{}", b.0, b.1));
                chapter = b.0;
            } else {
                out.push_str(&format!("–{}", b.1));
            }
        }
    }
    format!("{book} {out}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripture::test_support::v;

    #[test]
    fn join_refs_copy_matches_todays_output() {
        let g = |l: &[&str]| {
            let vs: Vec<_> = l.iter().map(|r| v(r, "x")).collect();
            join_refs(&vs.iter().collect::<Vec<_>>())
        };
        assert_eq!(g(&["John 3:16", "John 3:17"]), "John 3:16–17");
        assert_eq!(g(&["Psalm 23:1", "Psalm 23:4", "Psalm 23:5", "Psalm 23:6"]), "Psalm 23:1, 4–6");
        assert_eq!(g(&["John 3:36", "John 4:1", "John 4:2"]), "John 3:36–4:2");
        assert_eq!(g(&["John 3:16", "Odd label"]), "John 3:16, Odd label");
    }
}
