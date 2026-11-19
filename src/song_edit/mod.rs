//! Editing a song from its slides (Jake, November 18, 2026: "Editing a song should be MUCH easier. There should be a
//! way to just edit slides."). Pure: no UI, no locks. A song is one text (`Song::lyrics`): section names on lines of
//! their own, blank lines between slides, chords in `[G]` marks. A slide on screen is a few lines of that text, found
//! again here from the slide's place in the order (`SectionRef`), so a change to a slide goes back into the right
//! lines of the right section. Everything else in the text stays exactly as the church wrote it.
//!
//! - `Doc` reads the text the way `slides::parse_sections` does, keeping where each line is.
//! - `Doc::find` is the slide → lines mapping; the edits (`set_lines`, `split`, `join_next`, `add_after`, `delete`,
//!   `duplicate`, `change_section`) give the new text and what the order must do because of it.
//! - `words` shows a slide's words without chords, and `merge_words` puts the chords back after the words were
//!   edited, each over the same letter it was over.
//! - `order` changes the order (arrangement) and says where each part went, so the live slide stays put.
pub mod order;
pub mod words;
pub mod actions;
#[path = "../strings/song_edit.rs"]
pub mod strings;

#[cfg(test)]
mod tests;

use crate::model::{Settings, Song};
use crate::sections::{SectionRef, resolve_order};
use crate::slides::{group_pages, section_label};

/// One section as written: its name, its heading line (none for words before any heading) and its slides' lines.
#[derive(Clone, Debug, PartialEq)]
pub struct DocSection {
    pub label: String,
    pub heading: Option<usize>,
    /// Each blank-line group: the indexes of its lines in `Doc::lines`.
    pub groups: Vec<Vec<usize>>,
}

/// A song's words, line by line, with its sections found as the slides find them.
#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    pub lines: Vec<String>,
    pub sections: Vec<DocSection>,
    crlf: bool,
}

/// Where one slide is in the text.
#[derive(Clone, Debug, PartialEq)]
pub struct At {
    /// The written section, its blank-line group and the slide in that group (lines per slide).
    pub section: usize,
    pub group: usize,
    pub page: usize,
    /// The group's `{Label}` line, on the first slide of the group only.
    pub tag: Option<usize>,
    /// The slide's lines in `Doc::lines` (one after another).
    pub lines: std::ops::Range<usize>,
    /// The section's slides in all, and this slide's place among them.
    pub pages: usize,
    pub index: usize,
}

/// What an edit means for a song's order (and a service's own order of it), besides the new words.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderFix {
    /// A section was renamed.
    Rename { from: String, to: String },
    /// A section was split in two: wherever `label` plays, `new` plays after it.
    SplitAfter { label: String, new: String },
    /// A section is gone.
    Remove { label: String },
}

/// A change made from a slide: the new words, and the order fixes it needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub lyrics: String,
    pub fixes: Vec<OrderFix>,
}

/// Why an edit from a slide could not be made.
#[derive(Clone, Debug, PartialEq)]
pub enum Refused {
    /// The slide is not in the song any more (the song changed meanwhile).
    Gone,
    /// Join: the next slide is the same part shown a few lines at a time; `lines` per slide.
    SameGroup { lines: usize },
    /// Join: this is the last slide of its part.
    LastOfPart,
    /// Split: there is nowhere to split (one line, or the cursor on the first line).
    NoSplit,
    /// Delete: the song's only slide.
    OnlySlide,
}

impl Refused {
    pub fn message(&self) -> String {
        match self {
            Refused::Gone => strings::gone(),
            Refused::SameGroup { lines } => strings::same_group(*lines),
            Refused::LastOfPart => strings::last_of_part(),
            Refused::NoSplit => strings::no_split(),
            Refused::OnlySlide => strings::only_slide(),
        }
    }
}

fn blank(l: &str) -> bool { l.trim().is_empty() }

impl Doc {
    /// Reads `lyrics` as `slides::parse_sections` does.
    pub fn read(lyrics: &str) -> Doc {
        let crlf = lyrics.contains("\r\n");
        let lines: Vec<String> = lyrics.replace("\r\n", "\n").split('\n').map(str::to_string).collect();
        let mut sections: Vec<DocSection> = vec![];
        let mut current: Option<DocSection> = None;
        let mut group: Vec<usize> = vec![];
        fn close(current: &mut Option<DocSection>, group: &mut Vec<usize>) {
            if !group.is_empty() {
                current.get_or_insert_with(|| DocSection { label: String::new(), heading: None, groups: vec![] }).groups.push(std::mem::take(group));
            }
        }
        for (i, raw) in lines.iter().enumerate() {
            let line = raw.trim();
            if let Some(label) = section_label(line) {
                close(&mut current, &mut group);
                if let Some(s) = current.take() && !s.groups.is_empty() { sections.push(s) }
                current = Some(DocSection { label, heading: Some(i), groups: vec![] });
            } else if line.is_empty() {
                close(&mut current, &mut group);
            } else {
                group.push(i);
            }
        }
        close(&mut current, &mut group);
        if let Some(s) = current && !s.groups.is_empty() { sections.push(s) }
        for (i, s) in sections.iter_mut().enumerate() { if s.label.is_empty() { s.label = format!("Part {}", i + 1) } }
        Doc { lines, sections, crlf }
    }

    /// The text again, with the line endings it had.
    pub fn text(&self) -> String {
        let t = self.lines.join("\n");
        if self.crlf { t.replace('\n', "\r\n") } else { t }
    }

    pub fn labels(&self) -> Vec<String> { self.sections.iter().map(|s| s.label.clone()).collect() }

    /// A group's slides: its `{Label}` line (if any) and its lines cut `lps` to a slide.
    fn pages(&self, group: &[usize], lps: usize) -> (Option<usize>, Vec<std::ops::Range<usize>>) {
        let texts: Vec<String> = group.iter().map(|&i| self.lines[i].trim().to_string()).collect();
        let (tag, _) = group_pages(&texts, lps);
        let body = if tag.is_some() { &group[1..] } else { group };
        let tag = tag.map(|_| group[0]);
        if body.is_empty() {
            // A group that is only a label is one empty slide, just after its label.
            let at = group[0] + 1;
            return (tag, vec![at..at]);
        }
        (tag, body.chunks(lps.max(1)).map(|c| c[0]..c[c.len() - 1] + 1).collect())
    }

    /// How many slides written section `s` makes.
    pub fn section_pages(&self, s: usize, lps: usize) -> usize {
        self.sections[s].groups.iter().map(|g| self.pages(g, lps).1.len()).sum()
    }

    /// The slide `page` (counted across the section's groups) of written section `section`.
    pub fn at(&self, section: usize, page: usize, lps: usize) -> Option<At> {
        let s = self.sections.get(section)?;
        let pages = self.section_pages(section, lps);
        let mut n = 0;
        for (g, group) in s.groups.iter().enumerate() {
            let (tag, ranges) = self.pages(group, lps);
            for (p, r) in ranges.iter().enumerate() {
                if n == page { return Some(At { section, group: g, page: p, tag: if p == 0 { tag } else { None }, lines: r.clone(), pages, index: n }) }
                n += 1;
            }
        }
        None
    }

    /// The slide a `SectionRef` points at, in a song played in `order`.
    pub fn find(&self, order: &[String], r: &SectionRef, lps: usize) -> Option<At> {
        let section = *resolve_order(order, &self.labels()).get(usize::from(r.occurrence))?;
        self.at(section, usize::from(r.page), lps)
    }

    /// How many times written section `s` plays in `order`.
    pub fn plays(&self, order: &[String], s: usize) -> usize {
        resolve_order(order, &self.labels()).into_iter().filter(|&i| i == s).count()
    }

    /// A slide's lines as written (with chords).
    pub fn slide_lines(&self, at: &At) -> Vec<String> { self.lines[at.lines.clone()].iter().map(|l| l.trim().to_string()).collect() }

    /// A section name as a line the slides read as a heading: "Chorus", or "[Hook]" for a name they would read as
    /// words.
    pub fn heading_line(name: &str) -> String {
        if section_label(name).is_some_and(|l| l == name) { name.to_string() } else { format!("[{name}]") }
    }

    /// `lines` put in place of `range`, as several slides where they have blank lines between them; blank lines at
    /// the edges are dropped. Runs of blank lines around the change become one.
    fn splice(&mut self, range: std::ops::Range<usize>, lines: &[String]) {
        let mut new: Vec<String> = vec![];
        for l in lines {
            let l = l.trim_end().to_string();
            if blank(&l) && new.last().is_none_or(|x: &String| blank(x)) { continue }
            new.push(l);
        }
        while new.last().is_some_and(|l| blank(l)) { new.pop(); }
        let (start, n) = (range.start, new.len());
        self.lines.splice(range, new).for_each(drop);
        self.tidy_blanks(start.saturating_sub(1), start + n + 1);
    }

    /// One blank line at most in a row between `from` and `to` (the lines a change touched), and none before the
    /// first line of the song.
    fn tidy_blanks(&mut self, from: usize, to: usize) {
        let mut i = from.min(self.lines.len());
        let to = to.min(self.lines.len());
        let mut end = to;
        while i + 1 < end.min(self.lines.len()) {
            if blank(&self.lines[i]) && blank(&self.lines[i + 1]) { self.lines.remove(i + 1); end -= 1 } else { i += 1 }
        }
        while self.lines.first().is_some_and(|l| blank(l)) && self.lines.len() > 1 { self.lines.remove(0); }
    }

    fn done(self, fixes: Vec<OrderFix>) -> Change { Change { lyrics: self.text(), fixes } }

    /// The slide's lines replaced by `lines` (a blank line in them starts another slide). Empty `lines` deletes it.
    pub fn set_lines(&self, at: &At, lines: &[String]) -> Result<Change, Refused> {
        if lines.iter().all(|l| blank(l)) { return self.delete(at) }
        let mut d = self.clone();
        let range = if at.lines.is_empty() { at.lines.start..at.lines.start } else { at.lines.clone() };
        d.splice(range, lines);
        Ok(d.done(vec![]))
    }

    /// Split: a new slide starts at line `line` of the slide (0 is its first line).
    pub fn split(&self, at: &At, line: usize) -> Result<Change, Refused> {
        if line == 0 || line >= at.lines.len() { return Err(Refused::NoSplit) }
        let mut d = self.clone();
        d.lines.insert(at.lines.start + line, String::new());
        Ok(d.done(vec![]))
    }

    /// Join with the next slide of the same part: the blank line between them goes (and the next one's label).
    pub fn join_next(&self, at: &At, lps: usize) -> Result<Change, Refused> {
        let s = &self.sections[at.section];
        let (_, ranges) = self.pages(&s.groups[at.group], lps);
        if at.page + 1 < ranges.len() { return Err(Refused::SameGroup { lines: lps }) }
        let Some(next) = s.groups.get(at.group + 1) else { return Err(Refused::LastOfPart) };
        let mut d = self.clone();
        let (end, first) = (*s.groups[at.group].last().unwrap_or(&at.lines.end), next[0]);
        let (tag, _) = self.pages(next, lps);
        let to = if tag.is_some() { first + 1 } else { first };
        d.lines.drain(end + 1..to);
        let joined = self.pages(&s.groups[at.group].iter().chain(next.iter().filter(|&&i| Some(i) != tag)).copied().collect::<Vec<_>>(), lps).1.len();
        let before = ranges.len() + self.pages(next, lps).1.len();
        if joined >= before { return Err(Refused::SameGroup { lines: lps }) }
        Ok(d.done(vec![]))
    }

    /// A new slide of `lines` just after this one, in the same part.
    pub fn add_after(&self, at: &At, lines: &[String]) -> Result<Change, Refused> {
        let lines: Vec<String> = lines.iter().map(|l| l.trim_end().to_string()).collect();
        if lines.iter().all(|l| blank(l)) { return Ok(Change { lyrics: self.text(), fixes: vec![] }) }
        let mut d = self.clone();
        let end = at.lines.end.max(at.tag.map_or(0, |t| t + 1));
        let mut new = vec![String::new()];
        new.extend(lines);
        new.push(String::new());
        let n = new.len();
        d.lines.splice(end..end, new).for_each(drop);
        d.tidy_blanks(end.saturating_sub(1), end + n + 1);
        // A blank line at the very end of the song was not there before.
        if !self.lines.last().is_some_and(|l| blank(l)) { while d.lines.last().is_some_and(|l| blank(l)) && d.lines.len() > 1 { d.lines.pop(); } }
        Ok(d.done(vec![]))
    }

    /// A copy of this slide just after it.
    pub fn duplicate(&self, at: &At) -> Result<Change, Refused> {
        let mut lines = self.slide_lines(at);
        if lines.is_empty() {
            // A slide that is only a label: its label again.
            lines = at.tag.map(|t| vec![self.lines[t].trim().to_string()]).unwrap_or_default();
            if lines.is_empty() { return Err(Refused::Gone) }
        }
        self.add_after(at, &lines)
    }

    /// The slide taken out of the song. A part with no slides left goes too (its name, and from the order).
    pub fn delete(&self, at: &At) -> Result<Change, Refused> {
        let s = &self.sections[at.section];
        if self.sections.len() == 1 && s.groups.len() == 1 && at.pages <= 1 { return Err(Refused::OnlySlide) }
        let mut d = self.clone();
        let group = &s.groups[at.group];
        let rest: Vec<usize> = group.iter().copied().filter(|i| !at.lines.contains(i) && Some(*i) != at.tag).collect();
        let tag_alone = at.tag.is_some() && rest.is_empty();
        let mut fixes = vec![];
        let mut remove: Vec<usize> = at.lines.clone().collect();
        if tag_alone || (at.lines.is_empty() && at.tag.is_some()) { remove.extend(at.tag) }
        let group_gone = rest.is_empty() || tag_alone;
        if group_gone && s.groups.len() == 1 {
            // The whole part: its heading too, and it leaves the order unless another part has the same name.
            remove.extend(s.heading);
            let same = self.sections.iter().enumerate().any(|(i, o)| i != at.section && o.label.eq_ignore_ascii_case(&s.label));
            if !same { fixes.push(OrderFix::Remove { label: s.label.clone() }) }
        }
        remove.sort_unstable();
        remove.dedup();
        let first = remove.first().copied().unwrap_or(at.lines.start);
        for i in remove.into_iter().rev() { if i < d.lines.len() { d.lines.remove(i); } }
        d.tidy_blanks(first.saturating_sub(1), first + 1);
        while d.lines.last().is_some_and(|l| blank(l)) && !self.lines.last().is_some_and(|l| blank(l)) && d.lines.len() > 1 { d.lines.pop(); }
        Ok(d.done(fixes))
    }

    /// From this slide on, the part is `name`: on the part's first slide the part is renamed; further in, the part
    /// is split here and the rest gets `name` (wherever the part plays, the new part plays after it).
    pub fn change_section(&self, at: &At, name: &str) -> Result<Change, Refused> {
        let name = name.trim();
        let s = &self.sections[at.section];
        if name.is_empty() || name.eq_ignore_ascii_case(&s.label) { return Ok(Change { lyrics: self.text(), fixes: vec![] }) }
        let mut d = self.clone();
        let heading = Self::heading_line(name);
        if at.index == 0 {
            match s.heading {
                Some(h) => d.lines[h] = heading,
                None => {
                    let first = s.groups[0][0];
                    d.lines.insert(first, heading);
                }
            }
            return Ok(d.done(vec![OrderFix::Rename { from: s.label.clone(), to: name.to_string() }]));
        }
        let start = at.tag.unwrap_or(at.lines.start);
        let before_blank = start > 0 && blank(&d.lines[start - 1]);
        d.lines.insert(start, heading);
        if !before_blank { d.lines.insert(start, String::new()) }
        Ok(d.done(vec![OrderFix::SplitAfter { label: s.label.clone(), new: name.to_string() }]))
    }

    /// The names "Change section" offers for written section `s`: each kind once, with a number that no other part
    /// has ("Verse 3", "Chorus 2"), its own name left out.
    pub fn section_choices(&self, s: usize) -> Vec<String> {
        let taken = |n: &str| self.sections.iter().any(|x| x.label.eq_ignore_ascii_case(n));
        let free = |base: &str, numbered: bool| -> String {
            let first = if numbered { format!("{base} 1") } else { base.to_string() };
            if !taken(&first) && !(numbered && taken(base)) { return first }
            (2..100).map(|n| format!("{base} {n}")).find(|n| !taken(n)).unwrap_or(first)
        };
        let _ = s;
        [("Verse", true), ("Chorus", false), ("Pre-Chorus", false), ("Bridge", false), ("Tag", false), ("Intro", false),
            ("Instrumental", false), ("Interlude", false), ("Ending", false)].into_iter().map(|(base, numbered)| free(base, numbered)).collect()
    }
}

/// `fixes` applied to an order as typed. An empty order (the written one) follows the words by itself.
pub fn fix_order(order: &mut Vec<String>, fixes: &[OrderFix]) {
    if order.is_empty() { return }
    for f in fixes {
        match f {
            OrderFix::Rename { from, to } => for e in order.iter_mut() { if e.trim().eq_ignore_ascii_case(from) { *e = to.clone() } },
            OrderFix::SplitAfter { label, new } => {
                let mut out = vec![];
                for e in order.drain(..) {
                    let hit = e.trim().eq_ignore_ascii_case(label);
                    out.push(e);
                    if hit { out.push(new.clone()) }
                }
                *order = out;
            }
            OrderFix::Remove { label } => order.retain(|e| !e.trim().eq_ignore_ascii_case(label)),
        }
    }
}

/// The lines per slide a song shows with `settings`.
pub fn lps(song: &Song, settings: &Settings) -> usize { song.lines(settings) }
