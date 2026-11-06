//! Renders slides on one worker thread and keeps recent results, so going back to a slide or
//! redrawing a thumbnail costs nothing and the UI thread never waits on text rendering.
use super::{Fonts, Premul, Rendered, TextSlide, render_text};
use crate::model::{PassageSlide, Theme};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, atomic::{AtomicU64, Ordering}};
use tokio::sync::oneshot;

/// Everything that decides how a text slide looks.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderKey {
    pub text: String,
    pub footer: Option<String>,
    pub reference: Option<String>,
    pub scripture: bool,
    /// Largest text size; 0 = the theme's (`TextSlide::size`).
    pub size: u32,
    /// The passage whose slides share one size (`TextSlide::passage`).
    pub passage: Vec<PassageSlide>,
    pub theme: Theme,
    pub canvas: (u32, u32),
}

/// Default memory cap for cached pixels: 256 MB, about 32 full-HD slides.
const DEFAULT_CAP: usize = 256 * 1024 * 1024;

struct Lru { map: HashMap<RenderKey, Arc<Rendered>>, order: VecDeque<RenderKey>, bytes: usize, cap: usize }

impl Lru {
    fn get(&mut self, k: &RenderKey) -> Option<Arc<Rendered>> {
        let r = self.map.get(k)?.clone();
        if let Some(i) = self.order.iter().position(|x| x == k) { let k = self.order.remove(i).unwrap(); self.order.push_back(k); }
        Some(r)
    }

    fn put(&mut self, k: RenderKey, r: Arc<Rendered>) {
        if self.map.contains_key(&k) { return; }
        self.bytes += r.rgba_premul.len();
        self.map.insert(k.clone(), r);
        self.order.push_back(k);
        while self.bytes > self.cap && self.order.len() > 1 {
            let old = self.order.pop_front().unwrap();
            if let Some(r) = self.map.remove(&old) { self.bytes -= r.rgba_premul.len(); }
        }
    }
}

/// A counter of what is wanted: each `next` makes every earlier `Wanted` stale. The room bumps it for each new
/// live view, so a slide that is no longer wanted is dropped before it is drawn (rapid Next presses).
#[derive(Clone, Default)]
pub struct Generation(Arc<AtomicU64>);

impl Generation {
    /// A new generation; earlier ones are stale from now on.
    pub fn next(&self) -> Wanted { Wanted { current: self.0.clone(), mine: self.0.fetch_add(1, Ordering::AcqRel) + 1 } }
}

/// Whether the work of one generation is still wanted.
#[derive(Clone)]
pub struct Wanted { current: Arc<AtomicU64>, mine: u64 }

impl Wanted {
    pub fn still(&self) -> bool { self.current.load(Ordering::Acquire) == self.mine }
}

/// Most slides the idle queue holds; older ones are dropped first.
const IDLE_CAP: usize = 4;

struct Job { key: RenderKey, reply: oneshot::Sender<Arc<Rendered>>, wanted: Option<Wanted> }

/// Slides to draw: `urgent` ones someone waits for, in order; `idle` ones only fill the cache (prefetch) when
/// nothing else waits.
#[derive(Default)]
struct Queue { urgent: VecDeque<Job>, idle: VecDeque<RenderKey>, closed: bool }

type Shared = Arc<(Mutex<Queue>, Condvar)>;

fn blank(canvas: (u32, u32)) -> Rendered {
    let (w, h) = crate::render::layout::clamp_canvas(canvas);
    Rendered { width: w, height: h, rgba_premul: Premul::zeroed(w, h), font_px: 0.0, font_fallback: false }
}

pub struct Renderer { cache: Arc<Mutex<Lru>>, queue: Shared }

impl Renderer {
    pub fn new() -> Arc<Self> { Self::with_cap(DEFAULT_CAP) }

    /// A renderer whose cache holds at most `bytes` of pixels (the newest result is always kept).
    pub fn with_cap(bytes: usize) -> Arc<Self> {
        let cache = Arc::new(Mutex::new(Lru { map: HashMap::new(), order: VecDeque::new(), bytes: 0, cap: bytes }));
        let queue: Shared = Arc::default();
        let (worker_cache, worker_queue) = (cache.clone(), queue.clone());
        std::thread::Builder::new().name("slide-renderer".into()).spawn(move || {
            let mut fonts = Fonts::new();
            while let Some((key, reply)) = next_job(&worker_queue) {
                // Another request may have rendered this key while the job waited in line.
                let hit = worker_cache.lock().unwrap().get(&key);
                let r = hit.unwrap_or_else(|| {
                    let slide = TextSlide {
                        text: &key.text, footer: key.footer.as_deref(), reference: key.reference.as_deref(), scripture: key.scripture,
                        size: key.size, passage: &key.passage,
                    };
                    // A panic while drawing one slide must not stop the renderer for every screen: show it blank.
                    let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render_text(&mut fonts, &slide, &key.theme, key.canvas)));
                    let r = Arc::new(drawn.unwrap_or_else(|_| {
                        eprintln!("Could not draw a slide ({:?}); showing it blank", key.text.chars().take(40).collect::<String>());
                        fonts = Fonts::new();
                        blank(key.canvas)
                    }));
                    worker_cache.lock().unwrap().put(key, r.clone());
                    r
                });
                if let Some(reply) = reply { let _ = reply.send(r); }
            }
        }).expect("start the slide renderer thread");
        Arc::new(Self { cache, queue })
    }

    /// The rendered slide, from the cache or freshly drawn on the worker thread.
    pub async fn render(&self, key: RenderKey) -> Arc<Rendered> {
        self.ask(key, None).await.expect("slide renderer thread stopped")
    }

    /// As `render`, unless `wanted` goes stale before the slide is drawn: then `None`, and the work is skipped.
    pub async fn render_if(&self, key: RenderKey, wanted: Wanted) -> Option<Arc<Rendered>> {
        if !wanted.still() { return None }
        self.ask(key, Some(wanted)).await
    }

    async fn ask(&self, key: RenderKey, wanted: Option<Wanted>) -> Option<Arc<Rendered>> {
        if let Some(r) = self.cached(&key) { return Some(r); }
        let (tx, rx) = oneshot::channel();
        {
            let (q, cv) = &*self.queue;
            q.lock().unwrap().urgent.push_back(Job { key, reply: tx, wanted });
            cv.notify_one();
        }
        rx.await.ok()
    }

    /// Draws `key` into the cache when nothing else is waiting (the slides either side of the live one), so the
    /// next Next finds it ready. Already cached or queued: nothing happens.
    pub fn prefetch(&self, key: RenderKey) {
        if self.cache.lock().unwrap().map.contains_key(&key) { return }
        let (q, cv) = &*self.queue;
        let mut q = q.lock().unwrap();
        if q.idle.contains(&key) { return }
        q.idle.push_back(key);
        while q.idle.len() > IDLE_CAP { q.idle.pop_front(); }
        cv.notify_one();
    }

    pub fn cached(&self, key: &RenderKey) -> Option<Arc<Rendered>> { self.cache.lock().unwrap().get(key) }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let (q, cv) = &*self.queue;
        q.lock().unwrap().closed = true;
        cv.notify_all();
    }
}

/// The worker's next slide: the oldest urgent one still wanted, else an idle one; `None` once the renderer is
/// gone. A stale job is dropped here, which tells its waiter (`render_if` gives `None`).
fn next_job(queue: &Shared) -> Option<(RenderKey, Option<oneshot::Sender<Arc<Rendered>>>)> {
    let (q, cv) = &**queue;
    let mut q = q.lock().unwrap();
    loop {
        if q.closed { return None }
        while let Some(job) = q.urgent.pop_front() {
            if job.wanted.as_ref().is_none_or(Wanted::still) { return Some((job.key, Some(job.reply))) }
        }
        if let Some(key) = q.idle.pop_front() { return Some((key, None)) }
        q = cv.wait(q).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str, canvas: (u32, u32)) -> RenderKey {
        RenderKey { text: text.into(), footer: None, reference: None, scripture: false, size: 0, passage: vec![], theme: Theme { font: "Noto Sans".into(), ..Theme::default() }, canvas }
    }

    #[tokio::test]
    async fn second_request_is_served_from_cache() {
        let r = Renderer::new();
        let k = key("Amazing grace", (1920, 1080));
        let a = r.render(k.clone()).await;
        let b = r.render(k).await;
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[tokio::test]
    async fn cache_evicts_oldest_past_the_cap() {
        let r = Renderer::with_cap(2 * 1920 * 1080 * 4);
        for t in ["a", "b", "c"] { r.render(key(t, (1920, 1080))).await; }
        assert!(r.cached(&key("a", (1920, 1080))).is_none());
        assert!(r.cached(&key("c", (1920, 1080))).is_some());
    }

    #[tokio::test]
    async fn renderer_keeps_working_after_a_degenerate_canvas() {
        let r = Renderer::new();
        r.render(key("zero", (0, 0))).await;
        let ok = r.render(key("Amazing grace", (480, 270))).await;
        assert!(ok.rgba_premul.chunks(4).any(|p| p[3] > 0));
    }

    #[tokio::test]
    async fn many_concurrent_requests_all_complete() {
        let r = Renderer::new();
        let all = futures_util::future::join_all((0..64).map(|i| r.render(key(&format!("line {i}"), (480, 270))))).await;
        assert_eq!(all.len(), 64);
    }

    #[tokio::test]
    async fn a_stale_render_is_skipped_and_a_wanted_one_is_drawn() {
        let r = Renderer::new();
        let generation = Generation::default();
        let old = generation.next();
        let new = generation.next();
        assert!(!old.still() && new.still());
        assert!(r.render_if(key("old", (480, 270)), old).await.is_none(), "stale before it starts: not drawn");
        assert!(r.cached(&key("old", (480, 270))).is_none());
        assert!(r.render_if(key("new", (480, 270)), new).await.is_some());
    }

    #[test]
    fn rapid_requests_draw_only_the_newest() {
        // Ten Next presses while the renderer is busy: those that went stale in the queue are never drawn.
        let r = Renderer::new();
        let generation = Generation::default();
        let first = generation.next();
        // Holds the worker while the presses queue up behind it (a fresh renderer loads its fonts first).
        let busy = { let (r, k) = (r.clone(), key("busy", (1920, 1080))); std::thread::spawn(move || crate::ui::block_on(r.render_if(k, first))) };
        std::thread::sleep(std::time::Duration::from_millis(20));
        let waits: Vec<_> = (0..10).map(|i| {
            let (r, w) = (r.clone(), generation.next());
            std::thread::spawn(move || crate::ui::block_on(r.render_if(key(&format!("slide {i}"), (1920, 1080)), w)).is_some())
        }).collect();
        let _ = busy.join().unwrap();
        let drawn: Vec<bool> = waits.into_iter().map(|t| t.join().unwrap()).collect();
        assert!(drawn[9], "the newest is drawn");
        let skipped = drawn.iter().filter(|d| !**d).count();
        assert!(skipped >= 5, "most stale ones skipped: {drawn:?}");
        let cached = (0..9).filter(|i| r.cached(&key(&format!("slide {i}"), (1920, 1080))).is_some()).count();
        assert_eq!(cached + skipped, 9, "a skipped slide was never drawn");
    }

    #[test]
    fn prefetch_fills_the_cache_when_idle() {
        let r = Renderer::new();
        let k = key("next slide", (480, 270));
        r.prefetch(k.clone());
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while r.cached(&k).is_none() && std::time::Instant::now() < end { std::thread::sleep(std::time::Duration::from_millis(10)) }
        assert!(r.cached(&k).is_some());
    }

    #[test]
    fn the_worker_stops_with_the_renderer() {
        let r = Renderer::new();
        let queue = r.queue.clone();
        drop(r);
        assert!(next_job(&queue).is_none());
    }
}
