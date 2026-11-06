//! Renders slides on one worker thread and keeps recent results, so going back to a slide or
//! redrawing a thumbnail costs nothing and the UI thread never waits on text rendering.
use super::{Fonts, Rendered, TextSlide, render_text};
use crate::model::{PassageSlide, Theme};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, mpsc};
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

type Job = (RenderKey, oneshot::Sender<Arc<Rendered>>);

fn blank(canvas: (u32, u32)) -> Rendered {
    let (w, h) = crate::render::layout::clamp_canvas(canvas);
    Rendered { width: w, height: h, rgba_premul: vec![0; (w * h * 4) as usize], font_px: 0.0, font_fallback: false }
}

pub struct Renderer { cache: Arc<Mutex<Lru>>, jobs: Mutex<mpsc::Sender<Job>> }

impl Renderer {
    pub fn new() -> Arc<Self> { Self::with_cap(DEFAULT_CAP) }

    /// A renderer whose cache holds at most `bytes` of pixels (the newest result is always kept).
    pub fn with_cap(bytes: usize) -> Arc<Self> {
        let cache = Arc::new(Mutex::new(Lru { map: HashMap::new(), order: VecDeque::new(), bytes: 0, cap: bytes }));
        let (tx, rx) = mpsc::channel::<Job>();
        let worker_cache = cache.clone();
        std::thread::Builder::new().name("slide-renderer".into()).spawn(move || {
            let mut fonts = Fonts::new();
            for (key, reply) in rx {
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
                let _ = reply.send(r);
            }
        }).expect("start the slide renderer thread");
        Arc::new(Self { cache, jobs: Mutex::new(tx) })
    }

    /// The rendered slide, from the cache or freshly drawn on the worker thread.
    pub async fn render(&self, key: RenderKey) -> Arc<Rendered> {
        if let Some(r) = self.cached(&key) { return r; }
        let (tx, rx) = oneshot::channel();
        self.jobs.lock().unwrap().send((key, tx)).expect("slide renderer thread stopped");
        rx.await.expect("slide renderer thread stopped")
    }

    pub fn cached(&self, key: &RenderKey) -> Option<Arc<Rendered>> { self.cache.lock().unwrap().get(key) }
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
}
