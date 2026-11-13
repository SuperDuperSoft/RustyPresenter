//! When to change, blank and bring back the lyrics: the singers' pace predicts when the next slide's first
//! line starts, and the slide changes `lead` seconds before it. Unsure, talking or off-script: hold.
use super::align::Position;
use super::lyrics::SongText;
use serde::{Deserialize, Serialize};

/// How a song follows the singers.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The operator moves every slide (as before 1.3).
    #[default]
    Manual,
    /// The app highlights the slide it thinks is next; the operator takes it.
    Suggest,
    /// The app moves the slides itself; the operator can always take over.
    Auto,
}

impl Mode {
    /// The Auto chip's next mode: Manual → Suggest → Auto → Manual.
    pub fn next(self) -> Mode {
        match self { Mode::Manual => Mode::Suggest, Mode::Suggest => Mode::Auto, Mode::Auto => Mode::Manual }
    }

    /// The A key: Auto on, or (when on) back to Manual.
    pub fn toggle_auto(self) -> Mode { if self == Mode::Auto { Mode::Manual } else { Mode::Auto } }

    /// 0 Manual, 1 Suggest, 2 Auto (the chip and Settings' choices).
    pub fn index(self) -> usize { match self { Mode::Manual => 0, Mode::Suggest => 1, Mode::Auto => 2 } }

    pub fn from_index(i: usize) -> Mode { [Mode::Manual, Mode::Suggest, Mode::Auto].get(i).copied().unwrap_or_default() }
}

/// What the room sounds like right now (the voice tracker's label).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voice { Singing, Speaking, Music, Silence }

/// Settings → Auto-follow.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// The mode songs use unless an item has its own.
    pub mode: Mode,
    /// Seconds before the next slide's first word that it comes up (0.5–4). Counted back from that word as if
    /// the singers go straight on at their pace; the clock alone carries them at most `COAST_S` past the last
    /// word heard, then the change waits for another word (a held or stretched word is not a reason to change).
    pub lead_s: f32,
    /// Blank the words after this many seconds with no singing; `None` = never (the default: songs have long
    /// pauses, and the next words wait on the screen through them).
    pub blank_after_s: Option<f32>,
    /// Blank the words after this many seconds of talking; `None` = never (the default).
    pub talk_blank_after_s: Option<f32>,
    /// Well-matched words in a row needed to follow the singers outside the planned order.
    pub jump_words: u8,
    /// Below this confidence (0–1) Automatic holds and Suggest only highlights.
    pub min_conf: f32,
    /// The sound input to listen to (a name from `audio::input_devices`); `None`: the first one.
    pub input: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { mode: Mode::Manual, lead_s: 2.0, blank_after_s: None, talk_blank_after_s: None, jump_words: super::align::JUMP_WORDS, min_conf: 0.6,
            input: None }
    }
}

pub const LEAD_MIN: f32 = 0.5;
pub const LEAD_MAX: f32 = 4.0;

impl Settings {
    /// The lead time kept to 0.5–4 s, whatever was saved.
    pub fn lead(&self) -> f32 { if self.lead_s.is_nan() { 2.0 } else { self.lead_s.clamp(LEAD_MIN, LEAD_MAX) } }
    /// The jump strictness, at least 2 words.
    pub fn jump(&self) -> u8 { self.jump_words.max(2) }
}

/// What the rules want done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowAction {
    /// Take this slide live (Automatic).
    Show(usize),
    /// Highlight this slide for the operator (Suggest).
    Suggest(usize),
    /// Take the words off (keep the background).
    Blank,
    /// Singing is back: put the words up again on this slide.
    Unblank(usize),
}

/// The pace used until the singers have been heard: a calm hymn's syllables a second.
const DEFAULT_PACE: f64 = 2.5;
/// How fast the pace follows new words (moving average weight).
const PACE_WEIGHT: f64 = 0.3;
/// A gap longer than this between words is a breath or a break, not the pace.
const PACE_GAP: f64 = 3.0;
/// Singing heard this recently still counts as singing (the voice label flickers between words).
const STILL_SINGING: f64 = 1.0;
/// Words closer together than this came in one batch from the ear (it reports twice a second) or are too close
/// to time one by one: the pace is measured across them, not between them.
const PACE_MIN_GAP: f64 = 0.25;
/// The clock may stand in for words not heard yet for at most this long after the last word heard. The longest
/// wait an ordinary line needs is about 1.5 s; 2.5 s covers two missed words on a slow song, and anything longer
/// is a held or stretched word, not missed words: the change then waits for the next word.
const COAST_S: f64 = 2.5;
/// Going back to the slide just before the live one needs this many new words in a row on it: a real repeat or
/// tag gives three within about a second, one ad-lib or misheard word does not. A place that came back there
/// from a later slide needs no more: the aligner has already asked for `jump_words` words in a row.
const BACK_WORDS: u8 = 3;
/// A move to another slide needs a word that arrived this recently. While singing, words come at least every
/// second or so; a place older than this is left over from before a talk or a pause. Words "in a row" are no
/// further apart than this either.
const FRESH_S: f64 = 2.0;
/// After the song's end the words come back on this many sung words in a row (the same proof as `BACK_WORDS`):
/// the band may start the next song before the next item is clicked, and one of its words can match the old
/// slide. Spoken, unsure and long-ago words do not count.
const ENDED_BACK_WORDS: u8 = 3;
/// Going back a slide for less singing than this is only a flash (the old slide, then the next one again at
/// once): the next slide stays up.
const BACK_MIN_S: f64 = 1.0;
/// The song's last word was sung and nobody has sung for this long: the song is over, the words come off.
pub const SONG_END_QUIET: f64 = 5.0;
/// A slide's last word was sung and nobody has sung for this long: an instrumental slide after it comes up.
pub const INSTRUMENTAL_QUIET: f64 = 2.0;

pub struct Rules {
    settings: Settings,
    song: SongText,
    pace: Option<f64>,
    /// The last word heard and when it was sung (the ear delivers it later: see `arrived`).
    last_word: Option<(usize, f64)>,
    /// When the last word reached the rules. "Heard just now" is counted from here, not from when it was sung:
    /// a slow ear must not make every word look old.
    arrived: f64,
    started: Option<f64>,
    last_singing: Option<f64>,
    speaking_since: Option<f64>,
    blanked: bool,
    /// The word sung last when the words were taken off; singing must move on from it to bring them back.
    blank_word: Option<usize>,
    /// The pace the onsets give while singing, until words give one.
    beat: Option<f64>,
    /// Why the words were last taken off: talking (else no singing), and for how many seconds.
    blank_why: (bool, f32),
    /// The slide whose last word was surely sung, and that word. Talk afterwards does not undo it; singing
    /// that moves on to another word does.
    finished: Option<(usize, usize)>,
    /// The last word surely sung (not spoken) and when, and how many such words came in a row: what brings
    /// blanked words back.
    sung: Option<(usize, f64)>,
    sung_run: u8,
    /// The last word arrived while the room's label said talking.
    word_spoken: bool,
    /// Where the pace is measured from: a word and when it was sung.
    pace_from: Option<(usize, f64)>,
    /// The quiet is counted from here at the earliest: the operator's last move, or the end of a hold.
    quiet_from: Option<f64>,
    restart_quiet: bool,
    /// New words in a row going forward on one slide: (slide, the last of them, how many, proved). Sure or not:
    /// that is checked when acting. Proved: the place came onto this slide from a later one, surely and with
    /// nobody talking. The aligner only does that by a jump it has confirmed with `jump_words` words in a row,
    /// so the proof `BACK_WORDS` asks for is already given.
    run: Option<(usize, usize, u8, bool)>,
    /// On this slide the singers went back from its end: a tag (the last line sung again), not the tail of an
    /// early change.
    tag: Option<usize>,
    /// The screen went back to this slide, into its last line (a tag): it is left on its last word, not early
    /// again.
    came_back: Option<usize>,
    /// The live slide at the last look.
    last_live: Option<usize>,
    /// The words came off because the song ended (not by one of Settings' timers).
    ended: bool,
    /// Since the last word arrived the room's label has said talking, or the rules were not looking: the place
    /// may be left over from talk.
    talk_since_word: bool,
    /// When the room's label last said talking.
    last_talk: Option<f64>,
    /// Since the live slide went live, a look of the rules has found the room's label saying something other
    /// than singing. Only then can singing "start" on a slide with no words: the last note of the song before,
    /// still held when the operator puts the next song's intro slide up, is not a start.
    not_singing_here: bool,
}

impl Rules {
    pub fn new(settings: Settings, song: &SongText) -> Rules {
        Rules { settings, song: song.clone(), pace: None, last_word: None, arrived: 0.0, started: None, last_singing: None, speaking_since: None, blanked: false, blank_word: None, beat: None, blank_why: (false, 0.0), finished: None, sung: None, sung_run: 0, word_spoken: false, pace_from: None, quiet_from: None, restart_quiet: false,
            run: None, tag: None, came_back: None, last_live: None, ended: false, talk_since_word: false, last_talk: None,
            not_singing_here: false }
    }

    /// The singers' pace in syllables a second, once heard.
    pub fn pace(&self) -> Option<f64> { self.pace }

    /// The rules took the words off and wait for singing.
    pub fn blanked(&self) -> bool { self.blanked }

    /// Why the words were last taken off: `(talking, seconds)`; not talking means nobody sang for that long.
    pub fn blank_why(&self) -> (bool, f32) { self.blank_why }

    /// The operator moved or cleared: forget the last word and the blank (the pace stays, it is the same band).
    /// The quiet clocks start again from the operator's move, so a timer never undoes it at once.
    pub fn reset(&mut self) {
        self.last_word = None;
        self.pace_from = None;
        self.blanked = false;
        self.blank_word = None;
        self.finished = None;
        self.sung = None;
        self.sung_run = 0;
        self.run = None;
        self.tag = None;
        self.came_back = None;
        self.ended = false;
        self.resume();
    }

    /// The rules were not looking for a while (Hold, a video, Manual, waiting for the singers after a hand
    /// move): what they heard before is old, so the quiet and the talk are counted from the next look.
    pub fn resume(&mut self) {
        self.restart_quiet = true;
        self.speaking_since = None;
        // Someone may have talked meanwhile.
        self.talk_since_word = true;
    }

    /// The rules are not looking (Hold, a video, Manual, waiting for the singers after a hand move) but the ear
    /// goes on: the engine calls this instead of `tick_sung`. A new place is noted with the time it was really
    /// heard, so it is as old as it is when the rules look again: one heard long before the hold ended is not
    /// "heard just now" then, and moves nothing. Nobody checked that it was sung, so it starts no run of sung
    /// words.
    pub fn missed(&mut self, now: f64, sung: f64, pos: &Position, voice: Voice) {
        if voice == Voice::Speaking { self.last_talk = Some(now) }
        if self.note_word(now, sung.min(now), pos) {
            self.word_spoken = voice == Voice::Speaking;
            self.sung = None;
            self.sung_run = 0;
        }
        self.resume();
    }

    /// The screen did not take the rules' last blank or un-blank (someone else changed it at that moment):
    /// `blanked` is what the screen really shows, so the next look tries again.
    pub fn set_blanked(&mut self, blanked: bool) { self.blanked = blanked }

    /// Settings → Auto-follow changed, or the item's mode.
    pub fn set_settings(&mut self, settings: Settings) { self.settings = settings }

    /// One look at `now` (seconds): where the aligner is, what the room sounds like, and the live slide.
    /// `onsets_per_s` (note onsets while singing) stands in for the pace until words have been heard.
    ///
    /// The next words go up as the slide's last line is sung and then wait there, however long the pause. No
    /// timer takes words off unless Settings asks for one: they come off when the song's last word has been
    /// sung and it stays quiet, and where the song has an instrumental slide. The room's sound alone (no heard
    /// word) only leaves an instrumental slide as singing starts, and brings blanked words back.
    ///
    /// The lead is counted back from the next slide's first word, as if the singers go straight on at their
    /// pace. The clock alone carries them at most `COAST_S` past the last word heard; then the change waits for
    /// another word.
    pub fn tick(&mut self, now: f64, pos: &Position, voice: Voice, onsets_per_s: f32, live_slide: usize) -> Option<FollowAction> {
        self.tick_sung(now, now, pos, voice, onsets_per_s, live_slide)
    }

    /// `tick`, when the word in `pos` has only just been heard: `sung` is when it was sung, on the same clock as
    /// `now`. The ear delivers words late and several at once, so the pace and the lead are counted from when
    /// words were sung, not from when they arrived. On a look without a new word `sung` is not used.
    pub fn tick_sung(&mut self, now: f64, sung: f64, pos: &Position, voice: Voice, onsets_per_s: f32, live_slide: usize) -> Option<FollowAction> {
        let started = *self.started.get_or_insert(now);
        // Another slide is live than at the last look: a tag was about the old one, "came back" only holds for
        // the slide the screen came back to, and nothing but singing has been heard on the new one yet.
        if self.last_live != Some(live_slide) {
            self.last_live = Some(live_slide);
            self.tag = None;
            if self.came_back != Some(live_slide) { self.came_back = None }
            self.not_singing_here = false;
        }
        if std::mem::take(&mut self.restart_quiet) { self.quiet_from = Some(now) }
        match voice {
            Voice::Singing => {
                self.last_singing = Some(now);
                self.speaking_since = None;
                let r = onsets_per_s as f64;
                if (0.5..=12.0).contains(&r) { self.beat = Some(r) }
            }
            Voice::Speaking => {
                self.speaking_since.get_or_insert(now);
                self.talk_since_word = true;
                self.last_talk = Some(now);
            }
            Voice::Music | Voice::Silence => self.speaking_since = None,
        }
        // (Only at a look of the rules, not in `missed`: it can only make the next words wait for a heard word.)
        if voice != Voice::Singing { self.not_singing_here = true }
        let sure = pos.confidence >= self.settings.min_conf && !pos.off_script;
        // A new word of the song is singing, whatever the room's label says (a loud band hides the voice from it).
        let new = self.note_word(now, sung.min(now), pos);
        if let Some(w) = pos.word.filter(|&w| w < self.song.words.len()) {
            if new {
                self.word_spoken = voice == Voice::Speaking;
                self.talk_since_word = self.word_spoken;
            }
            // Sung: a word that just arrived (not under a talking label) and is sure, now or a moment later.
            let just_heard = self.last_word.is_some() && now - self.arrived <= STILL_SINGING;
            if sure && voice != Voice::Speaking && !self.word_spoken && just_heard && self.sung.is_none_or(|(s, _)| s != w) {
                self.last_singing = Some(now);
                self.sung = Some((w, now));
                self.sung_run = self.sung_run.saturating_add(1);
            } else if new {
                // A spoken or unsure word breaks the run of sung words.
                self.sung = None;
                self.sung_run = 0;
            }
            // A slide's last word, surely sung: remembered until singing moves on to another word.
            if sure && voice != Voice::Speaking {
                let slide = self.song.words[w].slide;
                self.finished = (w + 1 == self.song.slides[slide].end_word).then_some((slide, w));
            }
        }
        // Quiet: since anyone sang (or the operator's move); for "the last word was sung, then quiet", also
        // since that word was heard, whatever the room's label said at the time.
        let quiet_base = self.last_singing.unwrap_or(started).max(self.quiet_from.unwrap_or(started));
        let quiet = now - quiet_base;
        let quiet_since_word = now - quiet_base.max(if self.last_word.is_some() { self.arrived } else { started });
        let finished_here = self.finished.filter(|&(s, _)| s == live_slide);
        let mode = self.settings.mode;
        if mode == Mode::Manual { return None }
        let live_has_words = self.song.slides.get(live_slide).is_some_and(|s| !s.instrumental);

        // Blanking (Automatic only).
        if mode == Mode::Auto {
            if self.blanked {
                // Back when singing moves on from where it stopped; with no words heard at all (the ear can't
                // tell words yet), as singing starts, on the live slide.
                // A word counts when it was just sung (not a place left over from talk): one under a Singing
                // label, two in a row when a loud band hides the voice from the label. After one of Settings'
                // timers the word the singing stopped on does not count; after the song's end the words in a
                // row asked for below say more than that, and a short last line could not come back without it.
                let on = match pos.word {
                    Some(w) => {
                        let just_sung = self.sung.is_some_and(|(s, t)| s == w && now - t <= STILL_SINGING);
                        let label = voice == Voice::Singing || (voice != Voice::Speaking && self.sung_run >= 2);
                        just_sung && label && (self.ended || Some(w) != self.blank_word) && w < self.song.words.len()
                    }
                    None => live_has_words && voice == Voice::Singing,
                };
                if !on { return None }
                // After the song's end one matching word is not enough, whatever the label says: the band may be
                // starting the next song. A tag or reprise gives several words in a row, and every one of them
                // must have been surely sung (a line the pastor quotes is not singing, even when the label
                // flickers on its last word). Singing on to another slide (the chorus again) is proved by the
                // aligner's own words in a row: its first sung word there is enough.
                if self.ended && pos.word.is_some() {
                    let run = self.run.filter(|r| r.3 || r.2 >= ENDED_BACK_WORDS);
                    if !run.is_some_and(|r| self.sung_run >= r.2.min(ENDED_BACK_WORDS)) { return None }
                }
                self.blanked = false;
                self.ended = false;
                let slide = pos.word.map_or(live_slide, |w| self.song.words[w].slide);
                return Some(FollowAction::Unblank(slide));
            }
            // Timers, only when Settings asks for them.
            let no_singing = matches!(voice, Voice::Music | Voice::Silence) && self.settings.blank_after_s.is_some_and(|b| quiet >= b as f64);
            let talking = self.speaking_since.is_some_and(|s| self.settings.talk_blank_after_s.is_some_and(|b| now - s >= b as f64));
            // The end of the song: its last word was sung on the last slide, and it has stayed quiet.
            // Talk straight after the song does not undo "the last word was sung".
            let ended = finished_here.is_some() && live_slide + 1 == self.song.slides.len() && voice != Voice::Singing && quiet_since_word >= SONG_END_QUIET;
            if live_has_words && (no_singing || talking || ended) {
                self.blanked = true;
                // By the song's end alone, not by one of Settings' timers (those keep the quicker way back).
                self.ended = !no_singing && !talking;
                self.blank_word = if self.ended { finished_here.map(|(_, w)| w) } else { pos.word };
                self.sung = None;
                self.sung_run = 0;
                // Words in a row are counted again from here. After the song's end, with the aligner on the
                // song's last word, that place is kept (no words counted): singing on to an earlier slide is
                // then seen to come from it, which the aligner only does by a confirmed jump.
                self.run = match self.blank_word {
                    Some(b) if self.ended && pos.word == Some(b) => Some((live_slide, b, 0, false)),
                    _ => None,
                };
                self.blank_why = if no_singing {
                    (false, self.settings.blank_after_s.unwrap_or(0.0))
                } else if talking {
                    (true, self.settings.talk_blank_after_s.unwrap_or(0.0))
                } else {
                    (false, SONG_END_QUIET as f32)
                };
                return Some(FollowAction::Blank);
            }
        }

        let act = |n: usize| Some(if mode == Mode::Auto { FollowAction::Show(n) } else { FollowAction::Suggest(n) });
        // Where the singers surely are: (word, its slide). Talk, an ad-lib or an unsure place gives none.
        let placed = pos.word.filter(|_| sure && voice != Voice::Speaking).and_then(|w| Some((w, self.song.words.get(w)?.slide)));

        // An instrumental slide is live (an intro, a break): as singing starts the next words come up, unless
        // the singers are already heard somewhere else. Starts: the room's label was something else on this
        // slide first. Singing that was already going on when the slide went live (the last note of the song
        // before, as the operator takes the next song's intro live) leaves it alone; a sure word heard on
        // another slide still moves the screen, further down.
        if !live_has_words && voice == Voice::Singing && self.not_singing_here
            && !placed.is_some_and(|(w, at)| at != live_slide && !self.trailing(w, at, live_slide)) {
            if let Some(n) = (live_slide + 1..self.song.slides.len()).find(|&s| !self.song.slides[s].instrumental) { return act(n) }
        }

        // An instrumental slide after the live one would take the words off: only once the live slide's last
        // word is sung and it is quiet (talk holds).
        if finished_here.is_some() && self.song.slides.get(live_slide + 1).is_some_and(|s| s.instrumental)
            && matches!(voice, Voice::Music | Voice::Silence) && quiet_since_word >= INSTRUMENTAL_QUIET {
            return act(live_slide + 1);
        }

        // Talk, an ad-lib or an unsure place: hold.
        let (w, at) = placed?;

        if at != live_slide {
            // The singers are on another slide. Just behind the live one, on its last line, is where they are
            // after an early change: hold.
            if self.trailing(w, at, live_slide) { return None }
            // A move to another slide needs a word heard just now: a place the aligner was left on before a
            // talk or a pause moves nothing when the label stops saying "talking".
            let fresh = self.last_word.is_some() && now - self.arrived <= FRESH_S;
            // But not left over from talk: the first words of the next slide, surely sung as they arrived, and
            // nobody has talked since. The singers went on and then paused; the next words go up however long
            // the pause (Suggest is switched to Automatic in it).
            let sung_on_next = at > live_slide && (live_slide + 1..at).all(|s| self.song.slides[s].instrumental)
                && !self.talk_since_word && self.sung.is_some_and(|(s, _)| s == w);
            if !fresh && !sung_on_next { return None }
            // Back to the slide just before the live one: one ad-lib or misheard word must not flip the screen
            // back and forward again. A real repeat or tag gives several words in a row; a place that came back
            // from the live slide has given them to the aligner already. The same when the live slide is an
            // instrumental one: a song word found in something the leader says over the break must not bring the
            // verse before it back.
            let just_before = at < live_slide && (at + 1..live_slide).all(|s| self.song.slides[s].instrumental);
            if just_before {
                if !self.run.is_some_and(|r| r.0 == at && (r.3 || r.2 >= BACK_WORDS)) { return None }
                if live_has_words {
                    // The slide's last word, or under a second of it left (a tag of a short last line): going
                    // back would only flash the old slide, the next one comes up again at once. The next slide
                    // stays.
                    let span = &self.song.slides[at];
                    let left: u32 = self.song.words[w + 1..span.end_word].iter().map(|x| x.syllables as u32).sum();
                    if (left as f64) < BACK_MIN_S * self.pace.or(self.beat).unwrap_or(DEFAULT_PACE) { return None }
                    // Back into the last line (a tag): see `came_back`. A slide sung again from further up gets
                    // its lead as usual.
                    if w >= span.last_line_first_word { self.came_back = Some(at) }
                } else if self.word_spoken || self.sung_run < 2 {
                    // Over an instrumental slide the words in a row must have been sung, too: not a word that
                    // came while the room's label said talking, and at least two of them surely sung in a row.
                    // Holding here keeps the instrumental slide up; it never shows another slide.
                    return None;
                }
            }
            return act(at);
        }

        // On the live slide: change `lead` seconds before the next slide's first word, never before the last
        // line has started, never past the last slide. Once the last word is heard the next words are up and
        // wait there, however long it is held or the band plays on.
        let span = &self.song.slides[live_slide];
        if w < span.last_line_first_word {
            // After a tag the slide is sung again from further up (three words in a row say so, as for going
            // back): the tag is over, and the next slide gets its lead as usual.
            if self.run.is_some_and(|r| r.0 == live_slide && r.2 >= BACK_WORDS) { self.came_back = None }
            return None;
        }
        let next = live_slide + 1;
        let after = self.song.slides.get(next)?;
        // An instrumental slide next: handled above, when it is quiet.
        if after.instrumental { return None }
        // The screen came back into this slide's last line (a tag): the words being sung are already inside the
        // lead, so without this the next slide would flip up again at once. It is left on its last word; if the
        // ear misses that, the next slide's first heard word brings it up.
        if self.came_back == Some(live_slide) && w + 1 != span.end_word { return None }
        // The slide's last word is sung: the next words go up now and wait, however long ago it was (after a
        // hold, or when Suggest is switched to Automatic in the pause).
        if w + 1 == span.end_word { return act(next) }
        let singing = self.last_singing.is_some_and(|t| now - t <= STILL_SINGING);
        if !singing { return None }
        let (_, heard_at) = self.last_word?;
        let left: u32 = self.song.words[w + 1..span.end_word].iter().map(|x| x.syllables as u32).sum();
        let pace = self.pace.or(self.beat).unwrap_or(DEFAULT_PACE);
        // The clock stands in for words not heard yet, but only for `COAST_S`: when the leader holds or
        // stretches a word, or sings something else over it, the unsung line stays up until a word is heard.
        let coasted = (now - heard_at).clamp(0.0, COAST_S);
        // Seconds to the next slide's first word.
        let to_go = left as f64 / pace - coasted;
        if to_go <= self.settings.lead() as f64 + 1e-6 { act(next) } else { None }
    }

    /// The singers at word `w` of slide `at` are just behind the live slide, on `at`'s last line: where they are
    /// after an early change. Not when they went back there from the slide's end: that is a tag, and its words
    /// should come back.
    fn trailing(&self, w: usize, at: usize, live_slide: usize) -> bool {
        at < live_slide && (at + 1..live_slide).all(|s| self.song.slides[s].instrumental) && w >= self.song.slides[at].last_line_first_word
            && self.tag != Some(at)
    }

    /// A new word moves the pace on (syllables sung since the last word over the time between) and is counted
    /// into the run of words in a row. `sung`: when it was sung; `now`: when it got here. True when the word is
    /// a new one.
    fn note_word(&mut self, now: f64, sung: f64, pos: &Position) -> bool {
        let Some(w) = pos.word else { return false };
        if w >= self.song.words.len() { return false }
        if self.last_word.is_some_and(|(last, _)| last == w) { return false }
        match self.pace_from {
            // More words of the same batch: measured when the next batch comes.
            Some((from, t)) if w > from && sung >= t && sung - t < PACE_MIN_GAP => {}
            // (A word stamped earlier than the one before it gives no rate: the measuring starts again from it.)
            Some((from, t)) if w > from && sung - t <= PACE_GAP => {
                let syl: u32 = self.song.words[from + 1..=w].iter().map(|x| x.syllables as u32).sum();
                let rate = syl as f64 / (sung - t);
                if (0.5..=12.0).contains(&rate) {
                    self.pace = Some(match self.pace { Some(p) => p + PACE_WEIGHT * (rate - p), None => rate });
                }
                self.pace_from = Some((w, sung));
            }
            _ => self.pace_from = Some((w, sung)),
        }
        // Words in a row going forward on one slide. In a row in time too: a word that comes long after the one
        // before it starts the count again.
        let slide = self.song.words[w].slide;
        let end = self.song.slides[slide].end_word;
        let before = self.run;
        let stale = now - self.arrived > FRESH_S;
        // The place comes onto an earlier slide from a later one. The aligner only does that by a jump it has
        // confirmed with `jump_words` words in a row, which is the proof asked of words in a row. Not when the
        // place is unsure, when jumps are set to less than that proof, or when someone talked while those
        // words came (a quoted line is not singing).
        let sure = pos.confidence >= self.settings.min_conf && !pos.off_script;
        let jumped = sure && self.settings.jump() >= BACK_WORDS && self.last_talk.is_none_or(|t| now - t > FRESH_S);
        self.run = Some(match before {
            Some((s, last, n, proved)) if s == slide && w > last && !stale => (slide, w, n.saturating_add(1), proved),
            // After the song's end a short last line is sung round again: the count goes on (two words never
            // give three in a row otherwise).
            Some((s, last, n, proved)) if self.ended && s == slide && w < last && last + 2 >= end && !stale => {
                (slide, w, n.saturating_add(1), proved)
            }
            Some((s, _, _, _)) if slide < s => (slide, w, 1, jumped),
            _ => (slide, w, 1, false),
        });
        // A tag: they were at the slide's last or next-to-last word and are now further back on it. (A step
        // back from the middle of a line is more likely one misheard word.)
        if before.is_some_and(|(s, last, _, _)| s == slide && w < last && last + 2 >= end) { self.tag = Some(slide) }
        self.last_word = Some((w, sung));
        self.arrived = now;
        true
    }
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
