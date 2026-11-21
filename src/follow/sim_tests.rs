//! A singer and an ear made of numbers, to measure the follower. A song is "sung" at it many times over, at
//! different speeds, in and out of the planned order, through an ear that misses, mishears and runs words
//! together as the real one does (`Room`, from the listening logs), and what Automatic puts on the screen is
//! scored against where the singers really were: was each slide up before its first word, did it leave while its
//! words were still being sung, was a slide up that nobody was singing.
//!
//! `sim_report` (ignored: `--ignored --nocapture`) prints the table used when tuning. The tests below it keep
//! the follower from getting worse.
use super::align::Heard;
use super::lyrics::SongText;
use super::review::{self, Shown, Take};
use super::rules::{Settings, Voice};
use super::testing::text;

/// A small random number maker, the same on every run.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng { Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1) }
    /// 0–1.
    pub fn unit(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn between(&mut self, a: f64, b: f64) -> f64 { a + (b - a) * self.unit() }
    pub fn chance(&mut self, p: f64) -> bool { self.unit() < p }
    pub fn pick(&mut self, n: usize) -> usize { ((self.unit() * n as f64) as usize).min(n.saturating_sub(1)) }
}

/// How the ear does in a room.
#[derive(Clone, Copy, Debug)]
pub struct Room {
    pub name: &'static str,
    /// A sung word is not delivered at all.
    pub miss: f64,
    /// It is delivered as something else.
    pub wrong: f64,
    /// It and the next one are delivered as one word that is neither.
    pub merge: f64,
    /// Words nobody sang, a second, while nobody sings (a held vowel, the band).
    pub stray: f64,
    /// The share of the singing the room's label calls talking.
    pub mislabel: f64,
    /// The ear runs this often (seconds).
    pub hop: f64,
    /// How likely the leader says something in a long gap between slides.
    pub talk: f64,
}

/// The lead singer's microphone on its own.
pub const CLEAN: Room = Room { name: "clean", miss: 0.06, wrong: 0.03, merge: 0.03, stray: 0.05, mislabel: 0.15, hop: 0.5, talk: 0.4 };
/// The main mix, or a microphone in the room.
pub const ROUGH: Room = Room { name: "rough", miss: 0.25, wrong: 0.08, merge: 0.08, stray: 0.25, mislabel: 0.5, hop: 0.5, talk: 0.4 };

/// A sung word.
#[derive(Clone, Copy, Debug)]
pub struct Sung { pub start: f64, pub end: f64, pub slide: usize, pub word: usize, pub pass: usize, pub heard_at: f64 }

/// A song sung once: the tape it makes, and where the singers really were.
pub struct Show { pub take: Take, pub sung: Vec<Sung>, pub passes: Vec<Pass> }

/// One slide sung from top to bottom.
#[derive(Clone, Copy, Debug)]
pub struct Pass { pub slide: usize, pub start: f64, pub end: f64, pub last_line: f64 }

fn garble(key: &str, rng: &mut Rng) -> String {
    let mut c: Vec<char> = key.chars().collect();
    let letters = ['T', 'K', 'S', 'M', 'N', 'R', 'L', 'F', 'P', 'X'];
    let i = rng.pick(c.len().max(1));
    if c.is_empty() { return "X".into() }
    if c.len() > 2 && rng.chance(0.3) { c.remove(i); } else { c[i] = letters[rng.pick(letters.len())]; }
    if rng.chance(0.3) { c.push(letters[rng.pick(letters.len())]) }
    c.into_iter().collect()
}

/// Sings `slides` in `order` (slide numbers, any may repeat or be left out) at `tempo` syllables a second.
pub fn perform(slides: &[&str], order: &[usize], tempo: f64, room: Room, seed: u64) -> Show { perform_as(slides, order, tempo, room, seed, None) }

/// `perform`, as the band has arranged the song (`arranged`): where it plays between the slides and for how
/// long, and which last words it holds, are the same every time it is sung (give or take a tenth), as they are
/// from practice to Sunday. Without one, every show makes them up anew.
pub fn perform_as(slides: &[&str], order: &[usize], tempo: f64, room: Room, seed: u64, arranged: Option<u64>) -> Show {
    let song = SongText::from_slides(&slides.iter().map(|s| text(s)).collect::<Vec<_>>());
    let mut rng = Rng::new(seed);
    let mut band = arranged.map(|a| Rng::new(a ^ 0x5EED_BA4D));
    let mut t = 3.0;
    let mut sung: Vec<Sung> = vec![];
    let mut passes: Vec<Pass> = vec![];
    // (from, to): somebody is singing; (from, to): a note is held.
    let mut singing: Vec<(f64, f64)> = vec![];
    let mut holds: Vec<(f64, f64)> = vec![];
    // Words the leader says between the singing.
    let mut said: Vec<(f64, String)> = vec![];
    for (p, &s) in order.iter().enumerate() {
        let span = &song.slides[s];
        let start = t;
        let mut last_line = t;
        // The arrangement of this pass: the hold on its last word, and the playing after it.
        let plan = band.as_mut().map(|b| {
            let hold = if b.chance(0.15) { b.between(2.0, 4.0) } else { b.between(0.3, 1.5) };
            (hold, b.unit(), b.unit(), b.between(2.0, 8.0), b.between(0.0, 0.8))
        });
        for w in span.words() {
            let word = &song.words[w];
            if w == span.last_line_first_word { last_line = t }
            let line_end = w + 1 == span.end_word || song.words[w + 1].line != word.line;
            let dur = word.syllables as f64 / tempo * rng.between(0.85, 1.15);
            // The last word of a line is held, the last of a slide longer; now and then much longer.
            let hold = if !line_end { 0.0 } else if w + 1 == span.end_word {
                match plan { Some(p) => p.0 * rng.between(0.9, 1.1), None => if rng.chance(0.15) { rng.between(2.0, 4.0) } else { rng.between(0.3, 1.5) } }
            } else { rng.between(0.1, 0.8) };
            // A word that ends on a held vowel is only delivered when the note ends ("me——").
            let heard_at = if hold >= 0.5 && rng.chance(0.5) { t + dur + hold } else { t + dur };
            sung.push(Sung { start: t, end: t + dur, slide: s, word: w, pass: p, heard_at });
            singing.push((t, t + dur + hold));
            if hold >= 0.5 { holds.push((t + dur * 0.5, t + dur + hold)) }
            t += dur + hold;
            // A breath after a line.
            if line_end { t += rng.between(0.2, 0.6) }
        }
        passes.push(Pass { slide: s, start, end: t, last_line });
        // Between slides: mostly straight on; where the order leaves the plan, or now and then, the band plays.
        let out_of_plan = order.get(p + 1).is_some_and(|&n| n != s + 1);
        let gap = match plan {
            Some((_, a, b, long, short)) => (if out_of_plan && a < 0.5 || b < 0.12 { long } else { short }) * rng.between(0.9, 1.1),
            None => if out_of_plan && rng.chance(0.5) || rng.chance(0.12) { rng.between(2.0, 8.0) } else { rng.between(0.0, 0.8) },
        };
        // In a long gap the leader may say something: a few words, some of them words of the song.
        if gap > 4.0 && rng.chance(room.talk) {
            let mut at = t + 0.8;
            for _ in 0..(4 + rng.pick(5)) {
                at += rng.between(0.2, 0.45);
                if at > t + gap - 1.0 { break }
                let key = if rng.chance(0.5) { song.words[rng.pick(song.words.len())].key.clone() } else { garble("SATRMANT", &mut rng) };
                said.push((at, key));
            }
        }
        t += gap;
    }
    let until = t + 3.0;

    // The ear.
    let mut heard: Vec<(f64, Heard)> = vec![];
    let deliver = |end: f64| ((end + 0.12) / room.hop).ceil() * room.hop + 0.08;
    let mut i = 0;
    while i < sung.len() {
        let s = sung[i];
        let key = song.words[s.word].key.clone();
        let short = key.chars().count() <= 1;
        let miss = if short { (room.miss * 2.5).min(0.9) } else { room.miss };
        if rng.chance(miss) { i += 1; continue }
        if rng.chance(room.merge) && i + 1 < sung.len() {
            let both = format!("{key}{}", song.words[sung[i + 1].word].key);
            heard.push((deliver(sung[i + 1].heard_at), Heard { key: garble(&both, &mut rng), at: sung[i + 1].end, conf: rng.between(0.5, 0.95) as f32 }));
            i += 2;
            continue;
        }
        if rng.chance(room.wrong) {
            // Mostly something that is no word of the song; now and then another word of it.
            let k = if rng.chance(0.25) { song.words[rng.pick(song.words.len())].key.clone() } else { garble(&key, &mut rng) };
            heard.push((deliver(s.heard_at), Heard { key: k, at: s.end, conf: rng.between(0.45, 0.95) as f32 }));
        } else {
            heard.push((deliver(s.heard_at), Heard { key, at: s.end, conf: rng.between(0.75, 1.0) as f32 }));
        }
        i += 1;
    }
    // Words nobody sang: in the quiet and the held notes.
    let mut at = 0.5;
    while at < until {
        let sings = singing.iter().any(|&(a, b)| at >= a && at < b) && !holds.iter().any(|&(a, b)| at >= a && at < b);
        if !sings && rng.chance(room.stray * 0.25) {
            let k = if rng.chance(0.6) { "A".to_string() } else if rng.chance(0.15) { song.words[rng.pick(song.words.len())].key.clone() } else { garble("HATN", &mut rng) };
            heard.push((deliver(at), Heard { key: k, at, conf: rng.between(0.4, 0.9) as f32 }));
        }
        at += 0.25;
    }
    for (at, key) in &said { heard.push((deliver(*at), Heard { key: key.clone(), at: *at, conf: rng.between(0.6, 1.0) as f32 })) }
    heard.sort_by(|a, b| a.0.total_cmp(&b.0));

    // The room's label, ten times a second. What it calls talking comes in stretches.
    let mut labels = vec![];
    let mut at = 0.0;
    let mut talk_until = 0.0;
    while at < until {
        let sings = singing.iter().any(|&(a, b)| at >= a && at < b);
        let held = holds.iter().find(|&&(a, b)| at >= a + 0.5 && at < b).map(|h| h.0);
        if sings && at >= talk_until && rng.chance(room.mislabel * 0.1) { talk_until = at + rng.between(0.5, 3.0) }
        let talks = said.iter().any(|(t, _)| (at - t).abs() < 0.4);
        let voice = if talks { if rng.chance(0.7) { Voice::Speaking } else { Voice::Singing } }
            else if !sings { if rng.chance(0.5) { Voice::Music } else { Voice::Silence } }
            else if at < talk_until && held.is_none() { Voice::Speaking } else { Voice::Singing };
        labels.push((at, voice, tempo as f32, if voice == Voice::Singing { held } else { None }));
        at += 0.1;
    }
    // A careful hand: each slide a second before its first word, never before the last word of the one before
    // has begun.
    let mut screen = vec![Shown { at: 0.0, slide: order[0], words: true, by_hand: true }];
    for (p, pass) in passes.iter().enumerate().skip(1) {
        let last_word = sung.iter().rev().find(|s| s.pass == p - 1).map_or(pass.start, |s| s.start);
        screen.push(Shown { at: (pass.start - 1.0).max(last_word), slide: pass.slide, words: true, by_hand: true });
    }
    Show { take: Take { item: None, title: String::new(), from: 0.0, until, slides: slides.iter().map(|s| s.to_string()).collect(), labels, heard, screen }, sung, passes }
}

/// How one show went.
#[derive(Clone, Copy, Debug, Default)]
pub struct Marks {
    /// Slides to change to.
    pub changes: f64,
    /// Not up 0.3 s before its first word.
    pub late: f64,
    /// Still not up a second after its first word.
    pub very_late: f64,
    /// Never shown during its pass.
    pub never: f64,
    /// Left the screen more than 3.5 s before its last word ended.
    pub early: f64,
    /// Seconds the slide was up before its first word, where it was up in time (summed).
    pub headroom: f64,
    /// Seconds of singing with a slide up that was neither being sung nor up next, and all the seconds of singing.
    pub wrong_s: f64,
    pub sung_s: f64,
    /// Sung words that were on the screen as they began, and all of them.
    pub on_screen: f64,
    pub words: f64,
    /// Changes to a slide that was neither being sung nor up next.
    pub false_moves: f64,
    pub shows: f64,
    /// Seconds after its first word that a late slide came up (summed), and how many came up at all.
    pub lateness: f64,
    pub late_up: f64,
}

impl Marks {
    pub fn add(&mut self, o: &Marks) {
        self.changes += o.changes; self.late += o.late; self.very_late += o.very_late; self.never += o.never; self.early += o.early;
        self.headroom += o.headroom; self.wrong_s += o.wrong_s; self.sung_s += o.sung_s; self.on_screen += o.on_screen; self.words += o.words;
        self.false_moves += o.false_moves; self.shows += o.shows; self.lateness += o.lateness; self.late_up += o.late_up;
    }
    pub fn late_share(&self) -> f64 { self.late / self.changes.max(1.0) }
    pub fn very_late_share(&self) -> f64 { self.very_late / self.changes.max(1.0) }
    pub fn early_share(&self) -> f64 { self.early / self.changes.max(1.0) }
    pub fn wrong_share(&self) -> f64 { self.wrong_s / self.sung_s.max(1.0) }
    pub fn on_screen_share(&self) -> f64 { self.on_screen / self.words.max(1.0) }
    pub fn row(&self) -> String {
        format!("late {:>5.1}% (by {:>3.1} s)  very late {:>5.1}%  never {:>4.1}%  early {:>4.1}%  headroom {:>3.1} s  wrong slide {:>5.1}%  false moves {:>4.2} a song  words on screen {:>5.1}%",
            self.late_share() * 100.0, self.lateness / self.late_up.max(1.0), self.very_late_share() * 100.0, self.never / self.changes.max(1.0) * 100.0, self.early_share() * 100.0,
            self.headroom / (self.changes - self.late).max(1.0), self.wrong_share() * 100.0, self.false_moves / self.shows.max(1.0), self.on_screen_share() * 100.0)
    }
}

fn slide_at(screen: &[Shown], t: f64) -> Option<usize> { screen.iter().take_while(|s| s.at <= t).last().filter(|s| s.words).map(|s| s.slide) }

/// Marks what Automatic showed (`auto`) against where the singers were.
pub fn mark(show: &Show, auto: &[Shown]) -> Marks {
    let mut m = Marks::default();
    for (p, pass) in show.passes.iter().enumerate() {
        if p > 0 {
            m.changes += 1.0;
            // When the slide came up for this pass: it may be up already (shown early, or the same slide again).
            let from = show.passes[p - 1].start;
            let up_at = if slide_at(auto, from) == Some(pass.slide) { Some(from) }
                else { auto.iter().find(|s| s.at > from && s.at <= pass.end && s.words && s.slide == pass.slide).map(|s| s.at) };
            match up_at {
                None => { m.never += 1.0; m.late += 1.0; m.very_late += 1.0 }
                Some(at) => {
                    let room = pass.start - at;
                    if room < 0.3 { m.late += 1.0; m.lateness += -room; m.late_up += 1.0 } else { m.headroom += room }
                    if room < -1.0 { m.very_late += 1.0 }
                }
            }
        }
        // It left while there was more than 3.5 s of it still to sing.
        let left = auto.iter().find(|s| s.at > pass.start && s.at < pass.end && (s.slide != pass.slide || !s.words)).map(|s| s.at);
        if left.is_some_and(|at| slide_at(auto, pass.start + 0.01) == Some(pass.slide) && pass.end - at > 3.5 + 1.5) { m.early += 1.0 }
    }
    // A move to a slide that is not the one being sung (or last sung) nor the one sung next.
    m.shows = 1.0;
    for a in auto.iter().skip(1).filter(|a| a.words) {
        let now = show.passes.iter().rposition(|p| p.start <= a.at + 0.01);
        let ok = match now {
            None => show.passes.first().is_some_and(|p| p.slide == a.slide),
            Some(i) => show.passes[i].slide == a.slide || show.passes.get(i + 1).is_some_and(|p| p.slide == a.slide),
        };
        if !ok { m.false_moves += 1.0 }
    }
    for s in &show.sung {
        m.words += 1.0;
        if slide_at(auto, s.start) == Some(s.slide) { m.on_screen += 1.0 }
        m.sung_s += s.end - s.start;
        let next = show.passes.get(s.pass + 1).map(|p| p.slide);
        let up = slide_at(auto, (s.start + s.end) / 2.0);
        if up.is_some_and(|u| u != s.slide && Some(u) != next) { m.wrong_s += s.end - s.start }
    }
    m
}

pub const HYMN: [&str; 8] = [
    "Amazing grace how sweet the sound\nThat saved a wretch like me",
    "I once was lost but now am found\nWas blind but now I see",
    "Twas grace that taught my heart to fear\nAnd grace my fears relieved",
    "How precious did that grace appear\nThe hour I first believed",
    "Through many dangers toils and snares\nI have already come",
    "Tis grace hath brought me safe thus far\nAnd grace will lead me home",
    "When we've been there ten thousand years\nBright shining as the sun",
    "We've no less days to sing God's praise\nThan when we first begun",
];

/// A song built like many sung today: verses that begin alike, a chorus whose first two slides are the same, lines
/// that come back, a bridge of one line over and over.
pub const MODERN: [&str; 10] = [
    "Morning breaks upon the hill\nMorning sings across the sea",
    "Morning calls my heart to rise\nAnd I lift my hands to Thee",
    "You are good and You are kind\nI will trust You all my life",
    "You are good and You are kind\nI will trust You all my life",
    "Every hour I need Your grace\nI will follow where You lead",
    "Step by step and day by day\nI will trust You all my life",
    "Evening falls upon the hill\nEvening hushes all the sea",
    "Evening calls my heart to rest\nAnd I lay my cares on Thee",
    "Holy holy holy Lord\nHoly holy holy Lord",
    "Worthy worthy is the Lamb\nWorthy worthy is the Lamb",
];

pub fn in_order(n: usize) -> Vec<usize> { (0..n).collect() }
/// The chorus again where the plan has it once, the bridge twice round, a verse left out, the last slide as a tag.
pub const SURPRISES: [usize; 23] = [0, 1, 2, 3, 4, 5, 2, 3, 4, 5, 6, 7, 2, 3, 4, 5, 8, 9, 8, 9, 4, 5, 5];
/// The hymn with the second verse left out and the last verse sung twice.
pub const HYMN_SURPRISES: [usize; 10] = [0, 1, 4, 5, 2, 3, 6, 7, 6, 7];

/// The marks of `seeds` shows of one kind.
pub fn run(slides: &[&str], order: &[usize], tempo: f64, room: Room, seeds: std::ops::Range<u64>, settings: &Settings) -> Marks {
    run_expecting(slides, order, &[], tempo, room, seeds, settings)
}

/// `run` with the follower expecting the song in `learned` order (`order::SungOrder`; empty: as listed).
pub fn run_expecting(slides: &[&str], order: &[usize], learned: &[usize], tempo: f64, room: Room, seeds: std::ops::Range<u64>, settings: &Settings) -> Marks {
    let mut all = Marks::default();
    for seed in seeds {
        let show = perform(slides, order, tempo, room, seed);
        let (auto, _) = review::replay_expecting(&show.take, settings, learned);
        all.add(&mark(&show, &auto));
    }
    all
}

/// How Sunday goes after a practice that was recorded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sunday {
    /// The order was learned, not the times (1.4.4).
    Order,
    /// The times too, and the band plays the song as it did at practice.
    Timed,
    /// The times too, but the band plays between the slides and holds its notes as it likes that day.
    TimedOtherwise,
}

/// The marks of `seeds` shows of a song that was practised first: the practice (another day's singing of the same
/// arrangement, a little faster or slower, the slides put up by a hand that is up to half a second out) teaches
/// the order and the times, and Sunday is followed with them.
pub fn run_practised(slides: &[&str], order: &[usize], tempo: f64, room: Room, seeds: std::ops::Range<u64>, settings: &Settings, sunday: Sunday) -> Marks {
    let mut all = Marks::default();
    for seed in seeds {
        let mut hand = Rng::new(seed + 77);
        let mut practice = perform_as(slides, order, tempo * hand.between(0.93, 1.07), room, seed + 5000, Some(seed)).take;
        for s in practice.screen.iter_mut().skip(1) { s.at += hand.between(-0.5, 0.5) }
        let (learned, secs) = super::order::learn_timed(&practice).unwrap_or_default();
        let show = perform_as(slides, order, tempo, room, seed, Some(if sunday == Sunday::TimedOtherwise { seed + 900 } else { seed }));
        let (auto, _) = review::replay_timed(&show.take, settings, &learned, if sunday == Sunday::Order { &[] } else { &secs });
        all.add(&mark(&show, &auto));
    }
    all
}

fn cases() -> Vec<(&'static str, Vec<&'static str>, Vec<usize>)> {
    vec![
        ("hymn, as planned", HYMN.to_vec(), in_order(HYMN.len())),
        ("hymn, surprises", HYMN.to_vec(), HYMN_SURPRISES.to_vec()),
        ("modern, as planned", MODERN.to_vec(), in_order(MODERN.len())),
        ("modern, surprises", MODERN.to_vec(), SURPRISES.to_vec()),
    ]
}

/// The table: `cargo test sim_report -- --ignored --nocapture`.
#[test]
#[ignore]
fn sim_report() {
    let settings = Settings::default();
    let mut total = Marks::default();
    // RP_SIM_PRACTISED: only the practised rows.
    let all = std::env::var("RP_SIM_PRACTISED").is_err();
    for room in [CLEAN, ROUGH].into_iter().filter(|_| all) {
        for (name, slides, order) in cases() {
            let mut m = Marks::default();
            for tempo in [2.5, 3.5, 4.5] { m.add(&run(&slides, &order, tempo, room, 0..40, &settings)) }
            println!("{:<6} {:<19} {}", room.name, name, m.row());
            total.add(&m);
        }
    }
    if all { println!("{:<6} {:<19} {}", "", "all", total.row()) }
    // The order learned at practice: sung in that order, and sung as listed after all.
    for room in [CLEAN, ROUGH].into_iter().filter(|_| all) {
        for (name, slides, learned) in [("hymn", HYMN.to_vec(), HYMN_SURPRISES.to_vec()), ("modern", MODERN.to_vec(), SURPRISES.to_vec())] {
            for (what, order) in [("learned, sung so", learned.clone()), ("learned, as listed", in_order(slides.len()))] {
                let mut m = Marks::default();
                for tempo in [2.5, 3.5, 4.5] { m.add(&run_expecting(&slides, &order, &learned, tempo, room, 0..40, &settings)) }
                println!("{:<6} {:<26} {}", room.name, format!("{name}, {what}"), m.row());
            }
        }
    }
    // Practised first: the order alone, the times too, and the times when Sunday is played otherwise.
    for room in [CLEAN, ROUGH] {
        for (name, slides, order) in cases() {
            for (what, sunday) in [("order", Sunday::Order), ("timed", Sunday::Timed), ("timed, otherwise", Sunday::TimedOtherwise)] {
                let mut m = Marks::default();
                for tempo in [2.5, 3.5, 4.5] { m.add(&run_practised(&slides, &order, tempo, room, 0..40, &settings, sunday)) }
                println!("{:<6} {:<36} {}", room.name, format!("{name}: {what}"), m.row());
            }
        }
    }
    // One show in full, to read.
    if let Ok(seed) = std::env::var("RP_SIM_SEED").map(|s| s.parse::<u64>().unwrap_or(0)) {
        let which = std::env::var("RP_SIM_CASE").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(3);
        let room = if std::env::var("RP_SIM_ROOM").is_ok_and(|r| r == "rough") { ROUGH } else { CLEAN };
        let (name, slides, order) = cases().swap_remove(which);
        // RP_SIM_TIMED: practised first (`run_practised`), followed with the order and the times.
        let timed = std::env::var("RP_SIM_TIMED").is_ok();
        let show = if timed { perform_as(&slides, &order, 3.5, room, seed, Some(seed)) } else { perform(&slides, &order, 3.5, room, seed) };
        let learned = if std::env::var("RP_SIM_LEARNED").is_ok() { order.clone() } else { vec![] };
        let (auto, trace) = if timed {
            let mut hand = Rng::new(seed + 77);
            let mut practice = perform_as(&slides, &order, 3.5 * hand.between(0.93, 1.07), room, seed + 5000, Some(seed)).take;
            for s in practice.screen.iter_mut().skip(1) { s.at += hand.between(-0.5, 0.5) }
            let (learned, secs) = super::order::learn_timed(&practice).unwrap_or_default();
            println!("practice: {}", secs.iter().map(|t| format!("{t:.1}")).collect::<Vec<_>>().join(" "));
            review::replay_timed(&show.take, &settings, &learned, &secs)
        } else { review::replay_expecting(&show.take, &settings, &learned) };
        println!("== {name}, seed {seed}, {}", room.name);
        for p in &show.passes { println!("sung: slide {:>2} from {:6.1} (last line {:6.1}) to {:6.1}", p.slide + 1, p.start, p.last_line, p.end) }
        let brief = std::env::var("RP_SIM_BRIEF").is_ok();
        for l in trace { if !brief || !l.contains("heard ") { println!("{l}") } }
        for (p, pass) in show.passes.iter().enumerate().skip(1) {
            let from = show.passes[p - 1].start;
            let up = if slide_at(&auto, from) == Some(pass.slide) { Some(from) } else { auto.iter().find(|s| s.at > from && s.at <= pass.end && s.words && s.slide == pass.slide).map(|s| s.at) };
            println!("slide {:>2} first word {:6.1}: {}", pass.slide + 1, pass.start, up.map_or("never up".to_string(), |u| format!("up {:.1} s before", pass.start - u)));
        }
        println!("{}", mark(&show, &auto).row());
    }
}

/// Sung as planned through the lead singer's own microphone, a slide is all but never late.
#[test]
fn sung_as_planned_on_a_clean_feed_the_slides_are_on_time() {
    let settings = Settings::default();
    for (slides, limit) in [(HYMN.to_vec(), 0.02), (MODERN.to_vec(), 0.06)] {
        let m = run(&slides, &in_order(slides.len()), 3.5, CLEAN, 0..30, &settings);
        assert!(m.late_share() <= limit, "{}", m.row());
        assert!(m.wrong_share() <= 0.04 && m.false_moves / m.shows <= 0.3, "{}", m.row());
        assert!(m.on_screen_share() >= 0.75, "{}", m.row());
    }
}

/// Through the main mix, where the ear loses a word in four: still mostly on time, and no slide of its own making.
#[test]
fn sung_as_planned_in_a_rough_room_most_slides_are_on_time() {
    let settings = Settings::default();
    for (slides, late, never) in [(HYMN.to_vec(), 0.22, 0.04), (MODERN.to_vec(), 0.32, 0.05)] {
        let m = run(&slides, &in_order(slides.len()), 3.5, ROUGH, 0..30, &settings);
        assert!(m.late_share() <= late && m.never / m.changes <= never, "{}", m.row());
        assert!(m.false_moves / m.shows <= 0.6, "{}", m.row());
    }
}

/// Out of the planned order, every slide the singers go to still comes up, within a line.
#[test]
fn out_of_the_planned_order_the_singers_are_found_within_a_line() {
    let m = run(&HYMN, &HYMN_SURPRISES, 3.5, CLEAN, 0..30, &Settings::default());
    assert!(m.never / m.changes <= 0.03, "{}", m.row());
    assert!(m.lateness / m.late_up.max(1.0) <= 2.8, "{}", m.row());
    let m = run(&MODERN, &SURPRISES, 3.5, CLEAN, 0..30, &Settings::default());
    assert!(m.never / m.changes <= 0.10 && m.late_share() <= 0.34, "{}", m.row());
}

/// With the sung order learned at practice (`order`), a song that leaves its slide list is followed as well as one
/// that keeps to it: no jump to wait for.
#[test]
fn sung_in_the_order_learned_at_practice_the_slides_are_on_time() {
    let settings = Settings::default();
    let m = run_expecting(&HYMN, &HYMN_SURPRISES, &HYMN_SURPRISES, 3.5, CLEAN, 0..30, &settings);
    assert!(m.late_share() <= 0.03 && m.never == 0.0 && m.false_moves / m.shows <= 0.1, "{}", m.row());
    let m = run_expecting(&MODERN, &SURPRISES, &SURPRISES, 3.5, CLEAN, 0..30, &settings);
    assert!(m.late_share() <= 0.10 && m.never / m.changes <= 0.01, "{}", m.row());
    let m = run_expecting(&HYMN, &HYMN_SURPRISES, &HYMN_SURPRISES, 3.5, ROUGH, 0..30, &settings);
    assert!(m.late_share() <= 0.20 && m.never / m.changes <= 0.04, "{}", m.row());
}

/// Sung in another order than the one learned, the singers are still found (as any song off its list is).
#[test]
fn off_the_learned_order_every_slide_still_comes_up() {
    let m = run_expecting(&HYMN, &in_order(HYMN.len()), &HYMN_SURPRISES, 3.5, CLEAN, 0..30, &Settings::default());
    assert!(m.never / m.changes <= 0.03 && m.lateness / m.late_up.max(1.0) <= 2.8, "{}", m.row());
}

/// With the times learned at practice too (`clock`), a rough room loses far fewer slides: the next one comes up
/// about when it did at practice even where the ear cannot make out the words. And it does no harm: no more
/// slides of the follower's own making, on a clean feed or when Sunday is played otherwise than practice.
#[test]
fn with_the_times_from_practice_a_rough_room_is_followed_better() {
    let settings = Settings::default();
    for (slides, order) in [(HYMN.to_vec(), HYMN_SURPRISES.to_vec()), (MODERN.to_vec(), in_order(MODERN.len()))] {
        let plain = run_practised(&slides, &order, 3.5, ROUGH, 0..30, &settings, Sunday::Order);
        let timed = run_practised(&slides, &order, 3.5, ROUGH, 0..30, &settings, Sunday::Timed);
        assert!(timed.very_late_share() <= plain.very_late_share() * 0.6, "{}
{}", plain.row(), timed.row());
        assert!(timed.wrong_share() <= plain.wrong_share() && timed.never <= plain.never, "{}
{}", plain.row(), timed.row());
        assert!(timed.false_moves / timed.shows <= plain.false_moves / plain.shows + 0.15, "{}
{}", plain.row(), timed.row());
        let other = run_practised(&slides, &order, 3.5, ROUGH, 0..30, &settings, Sunday::TimedOtherwise);
        assert!(other.late_share() <= plain.late_share() && other.wrong_share() <= plain.wrong_share() + 0.01, "{}
{}", plain.row(), other.row());
        assert!(other.early_share() <= plain.early_share() + 0.03, "{}
{}", plain.row(), other.row());
        let clean = run_practised(&slides, &order, 3.5, CLEAN, 0..30, &settings, Sunday::TimedOtherwise);
        assert!(clean.late_share() <= 0.05 && clean.wrong_share() <= 0.03 && clean.false_moves / clean.shows <= 0.3, "{}", clean.row());
    }
}
