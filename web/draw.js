// Drawing on the live slide from the phone or tablet, like a sports telestrator: circle a word, underline a line.
// One tap on Draw (or on the live picture) fills the screen with the slide, at the room screen's own shape, and the
// finger draws straight away. The page can't scroll, zoom or reload under the finger while this is open.
// Finger, mouse and pen all come as Pointer Events. While drawing, the points go to the computer in small batches
// (every 40 ms) as 0–1 of the slide, so the room screen shows the line as it is drawn, at any screen size. The
// computer checks everything again.
// Ink either stays, or fades like a laser pointer: a few seconds after the finger lifts it fades out and is gone.

// The same six colours, sizes and highlighter as the computer (src/live/drawing.rs).
export const PALETTE = ['#ffd60a', '#ff3b30', '#34c759', '#0a84ff', '#ffffff', '#111111'];
const SIZES = [6, 12, 22];
const HIGHLIGHTER = 44;
const HIGHLIGHTER_ALPHA = 110 / 255;
const BATCH_MS = 40;
const FADE_OUT_MS = 500;
// A line may start this far (CSS px) outside the slide, kept on its edge; further out is a stray touch.
const EDGE_PX = 28;
// After a pen lifts, a hand resting on the glass doesn't draw for this long.
const PALM_MS = 800;
// Points closer than this (CSS px) to the last one add nothing.
const MIN_STEP_PX = 1.2;
const SAVED = 'rp-draw';

const $ = id => document.getElementById(id);
let app = null;                       // { t, toast, relogin } from remote.js
const t = (en, args) => app.t(en, args);

// The tool choice: size 0–2 is a pen, 3 the highlighter. Remembered on this phone.
let color = PALETTE[0], size = 1, fades = false, drewBefore = false;
let fadeMs = 3000;                    // Settings → "Fading ink disappears after", from the remote view
let aspect = 16 / 9;                  // the room screen's shape (the remote view, then the picture itself)
let strokes = [];                     // my lines on screen: { id, color, width, pen, ink, pts, sent, acked, ended }
let cur = null;                       // the line under the finger
let nextId = Date.now() % 1000000;    // stroke numbers, fresh after a reload
const fingers = new Set();            // pointers down: two or more is a pinch, which never draws
let pinched = false, penUp = -Infinity, penDown = false;
const queue = [];                     // commands waiting to go, one request at a time (so they arrive in order)
let busy = false, version = null, open = false, painting = false;

function load() {
  try {
    const s = JSON.parse(localStorage.getItem(SAVED) || '{}');
    if (PALETTE.includes(s.color)) color = s.color;
    if (Number.isInteger(s.size) && s.size >= 0 && s.size <= SIZES.length) size = s.size;
    if (typeof s.fades === 'boolean') fades = s.fades;
    drewBefore = !!s.drew;
  } catch { /* private window: the defaults */ }
}
function save() {
  try { localStorage.setItem(SAVED, JSON.stringify({ color, size, fades, drew: drewBefore })); } catch { /* private window */ }
}

export function initDraw(helpers) {
  app = helpers;
  load();
  $('swatches').replaceChildren(...PALETTE.map(c => {
    const b = document.createElement('button');
    b.className = 'sw'; b.dataset.c = c; b.style.setProperty('--c', c);
    b.append(document.createElement('i'));
    b.onclick = () => { color = c; tools(); };
    return b;
  }));
  $('sizes').replaceChildren(...[...SIZES, HIGHLIGHTER].map((w, i) => {
    const b = document.createElement('button');
    b.className = 'size' + (i === SIZES.length ? ' hl' : '');
    const dot = document.createElement('i');
    if (i < SIZES.length) dot.style.width = dot.style.height = (6 + i * 7) + 'px';
    b.append(dot);
    b.onclick = () => { size = i; tools(); };
    return b;
  }));
  $('inkStays').onclick = () => { fades = false; tools(); };
  $('inkFades').onclick = () => { fades = true; tools(); };
  $('undoBtn').onclick = undo;
  $('clearBtn').onclick = clearAll;
  $('doneBtn').onclick = () => { if (history.state && history.state.rpDraw) history.back(); else closeDraw(); };
  addEventListener('popstate', () => { if (open) closeDraw(); });
  // The whole black area takes the finger, so a line that starts just off the slide still draws.
  const board = $('board');
  board.addEventListener('pointerdown', down);
  board.addEventListener('pointermove', move);
  board.addEventListener('pointerup', up);
  board.addEventListener('pointercancel', up);
  // No page scrolling, pull-to-refresh, zooming (double tap or pinch) or long-press menu while drawing.
  const draw = $('draw');
  const stop = e => e.preventDefault();
  board.addEventListener('touchstart', stop, { passive: false });
  draw.addEventListener('touchmove', stop, { passive: false });
  draw.addEventListener('dblclick', stop);
  draw.addEventListener('gesturestart', stop);
  draw.addEventListener('contextmenu', stop);
  draw.addEventListener('wheel', stop, { passive: false });
  $('slideImg').addEventListener('load', () => {
    const img = $('slideImg');
    if (img.naturalWidth && img.naturalHeight) setAspect(img.naturalWidth / img.naturalHeight);
  });
  addEventListener('resize', () => { if (open) fit(); });
  setInterval(flush, BATCH_MS);
  tools();
}

function tools() {
  document.querySelectorAll('#swatches .sw').forEach(b => { b.classList.toggle('sel', b.dataset.c === color); b.title = b.ariaLabel = t('Colour'); });
  document.querySelectorAll('#sizes .size').forEach((b, i) => {
    b.classList.toggle('sel', i === size);
    b.style.setProperty('--c', color);
    b.title = b.ariaLabel = i === SIZES.length ? t('Highlighter') : t('Pen size');
  });
  $('inkStays').classList.toggle('sel', !fades);
  $('inkFades').classList.toggle('sel', fades);
  save();
}

export function openDraw(view) {
  if (open) return;
  open = true;
  document.documentElement.classList.add('drawing');
  $('draw').classList.add('on');
  // The phone's Back button closes the drawing view, not the remote.
  try { history.pushState({ rpDraw: true }, ''); } catch { /* ignore */ }
  // Full screen where the phone allows it (Android, iPad): a bigger slide, no address bar.
  const el = document.documentElement;
  if (el.requestFullscreen && !document.fullscreenElement) el.requestFullscreen({ navigationUI: 'hide' }).catch(() => {});
  version = view ? view.version : null;
  if (view && view.aspect) setAspect(view.aspect[0] / view.aspect[1]);
  $('drawHint').classList.toggle('gone', drewBefore);
  tools();
  fit();
  loadSlide();
}

function closeDraw() {
  end();
  open = false;
  document.documentElement.classList.remove('drawing');
  $('draw').classList.remove('on');
  if (document.fullscreenElement && document.exitFullscreen) document.exitFullscreen().catch(() => {});
}

// A new remote view: a new slide means a new picture. `my_strokes` says which of my lines are still up.
export function onView(view) {
  if (view.fade_secs) fadeMs = view.fade_secs * 1000;
  if (view.aspect && view.aspect[0] > 0 && view.aspect[1] > 0) setAspect(view.aspect[0] / view.aspect[1]);
  if (open && view.version !== version) { version = view.version; loadSlide(); }
  keepOnly(view.my_strokes || []);
}

// The computer took some of my lines away (undo elsewhere, Clear, or a new slide).
export function onMine(ids) { keepOnly(ids); }

function keepOnly(ids) {
  const before = strokes.length;
  strokes = strokes.filter(s => !s.acked || ids.includes(s.id));
  if (cur && !strokes.includes(cur)) cur = null;
  if (strokes.length !== before) paint();
}

function setAspect(a) {
  if (!isFinite(a) || a <= 0 || Math.abs(a - aspect) < 0.005) return;
  aspect = a;
  $('turnHint').hidden = aspect < 1.1;
  if (open) fit();
}

// The slide box: as big as fits at the room's shape (black bars round it).
export function fitSize(boardW, boardH, a) {
  const w = Math.min(boardW, boardH * a);
  return [Math.max(1, Math.floor(w)), Math.max(1, Math.floor(w / a))];
}

// The canvas at the screen's real pixels.
function fit() {
  const board = $('board'), box = $('slideBox'), pad = $('pad');
  const [w, h] = fitSize(board.clientWidth, board.clientHeight, aspect);
  box.style.width = w + 'px';
  box.style.height = h + 'px';
  const dpr = devicePixelRatio || 1;
  pad.width = Math.round(w * dpr);
  pad.height = Math.round(h * dpr);
  draw();
}

function loadSlide() {
  const w = Math.min(1920, Math.round($('pad').width) || 960);
  $('slideImg').src = `/remote/live-slide?w=${w}&v=${version}`;
}

// Where a pointer is, as 0–1 of the slide (kept on it when the finger slides off the edge), and whether it is on
// the slide or within `edge` CSS pixels of it.
export function toSlide(x, y, r, edge = 0) {
  const c = v => Math.min(1, Math.max(0, v));
  const near = x >= r.left - edge && x <= r.right + edge && y >= r.top - edge && y <= r.bottom + edge;
  return { p: [c((x - r.left) / r.width), c((y - r.top) / r.height)], near };
}
const rect = () => $('pad').getBoundingClientRect();

// A finger resting on the glass while a pen draws (or just after) is the hand, not a line.
const palm = e => e.pointerType === 'touch' && (penDown || performance.now() - penUp < PALM_MS);

function down(e) {
  if (e.pointerType === 'pen') penDown = true;
  if (palm(e)) return;
  fingers.add(e.pointerId);
  if (fingers.size > 1) { pinched = true; return cancel(); }
  if (pinched || (e.pointerType === 'mouse' && e.button !== 0)) return;
  const at = toSlide(e.clientX, e.clientY, rect(), EDGE_PX);
  if (!at.near) return;
  e.preventDefault();
  try { $('board').setPointerCapture(e.pointerId); } catch { /* ignore */ }
  const hl = size === SIZES.length;
  cur = { id: nextId++, pointer: e.pointerId, color, width: hl ? HIGHLIGHTER : SIZES[size], pen: hl ? 'highlighter' : 'pen', ink: fades ? 'fades' : 'stays', pts: [at.p], sent: 0, acked: false, ended: null };
  strokes.push(cur);
  if (!drewBefore) { drewBefore = true; save(); $('drawHint').classList.add('gone'); }
  paint();
}

function move(e) {
  if (!cur || pinched || e.pointerId !== cur.pointer) return;
  // A pen gives more points than one per frame: take them all, for a smooth line.
  const all = e.getCoalescedEvents ? e.getCoalescedEvents() : [];
  const r = rect(), minX = MIN_STEP_PX / r.width, minY = MIN_STEP_PX / r.height;
  let added = false;
  for (const ev of all.length ? all : [e]) {
    const p = toSlide(ev.clientX, ev.clientY, r).p, last = cur.pts[cur.pts.length - 1];
    if (Math.abs(p[0] - last[0]) < minX && Math.abs(p[1] - last[1]) < minY) continue;
    cur.pts.push(p);
    added = true;
  }
  if (added) paint();
}

function up(e) {
  if (e.pointerType === 'pen') { penDown = false; penUp = performance.now(); }
  if (!fingers.delete(e.pointerId)) return;
  if (fingers.size === 0) pinched = false;
  if (cur && e.pointerId === cur.pointer) end();
}

// The finger lifted: send what is left, then say the line is done.
function end() {
  if (!cur) return;
  flush();
  if (cur.sent) send({ op: 'end', stroke: cur.id });
  cur.ended = performance.now();
  if (cur.ink === 'fades') requestAnimationFrame(fadeFrame);
  cur = null;
}

// Fading ink on this screen: redraw each frame while some is on, and let it go once it has faded.
function fadeFrame() {
  const now = performance.now();
  strokes = strokes.filter(s => opacity(s, now) > 0);
  draw();
  if (strokes.some(s => s.ink === 'fades' && s.ended !== null)) requestAnimationFrame(fadeFrame);
}

function opacity(s, now) {
  if (s.ink !== 'fades' || s.ended === null) return 1;
  const t = now - s.ended - fadeMs;
  return t <= 0 ? 1 : Math.max(0, 1 - t / FADE_OUT_MS);
}

// A second finger came down: this was a pinch, not a line. Take back what was drawn.
function cancel() {
  if (!cur) return;
  if (cur.sent) send({ op: 'cancel', stroke: cur.id });
  strokes = strokes.filter(s => s !== cur);
  cur = null;
  paint();
}

// Every 40 ms: the points not sent yet, in one batch.
function flush() {
  if (!cur || cur.pts.length <= cur.sent) return;
  const pts = cur.pts.slice(cur.sent).map(([x, y]) => [+x.toFixed(4), +y.toFixed(4)]);
  const s = cur;
  if (!s.sent) send({ op: 'start', stroke: s.id, color: s.color, width: s.width, pen: s.pen, ink: s.ink, points: pts }, ok => { if (ok) s.acked = true; else { strokes = strokes.filter(x => x !== s); if (cur === s) cur = null; paint(); } });
  else send({ op: 'more', stroke: s.id, points: pts });
  s.sent = s.pts.length;
}

// Undo takes back my last line that stays (fading ink goes by itself).
function undo() {
  end();
  const i = strokes.map(s => s.ink).lastIndexOf('stays');
  if (i < 0) return;
  const [mine] = strokes.splice(i, 1);
  paint();
  if (mine.sent) send({ op: 'undo' });
}

function clearAll() {
  end();
  strokes = [];
  paint();
  send({ op: 'clear' });
}

// One request at a time, in order. Points that pile up while one is on its way go together in the next.
function send(cmd, done) {
  const last = queue[queue.length - 1];
  if (cmd.op === 'more' && last && last.cmd.op === 'more' && last.cmd.stroke === cmd.stroke) last.cmd.points.push(...cmd.points);
  else queue.push({ cmd, done });
  pump();
}

async function pump() {
  if (busy || !queue.length) return;
  busy = true;
  const { cmd, done } = queue.shift();
  let ok = false;
  try {
    const r = await fetch('/remote/draw', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(cmd) });
    if (r.status === 401) { queue.length = 0; closeDraw(); app.relogin(); return; }
    const out = await r.json().catch(() => ({}));
    ok = !!out.ok;
    // A line the computer already took away (a new slide) is not worth a message.
    if (!ok && out.error && cmd.op !== 'more' && cmd.op !== 'end') app.toast(out.error);
  } catch { app.toast(t('Not connected')); }
  finally {
    busy = false;
    if (done) done(ok);
    pump();
  }
}

// At most one redraw a frame, however many points come in.
function paint() {
  if (painting) return;
  painting = true;
  requestAnimationFrame(() => { painting = false; draw(); });
}

// Draws my lines the way the room does: smooth curves through the points, round ends and corners, the highlighter
// see-through and even where it crosses itself (one path per line).
function draw() {
  const pad = $('pad'), g = pad.getContext('2d');
  g.clearRect(0, 0, pad.width, pad.height);
  g.lineCap = 'round';
  g.lineJoin = 'round';
  const now = performance.now();
  for (const s of strokes) {
    const p = s.pts.map(([x, y]) => [x * pad.width, y * pad.height]);
    const w = Math.max(1, s.width * pad.height / 1080);
    g.globalAlpha = (s.pen === 'highlighter' ? HIGHLIGHTER_ALPHA : 1) * opacity(s, now);
    g.strokeStyle = g.fillStyle = s.color;
    g.lineWidth = w;
    g.beginPath();
    if (p.length === 1) { g.arc(p[0][0], p[0][1], w / 2, 0, Math.PI * 2); g.fill(); continue; }
    g.moveTo(p[0][0], p[0][1]);
    for (let i = 1; i < p.length - 1; i++) g.quadraticCurveTo(p[i][0], p[i][1], (p[i][0] + p[i + 1][0]) / 2, (p[i][1] + p[i + 1][1]) / 2);
    g.lineTo(p[p.length - 1][0], p[p.length - 1][1]);
    g.stroke();
  }
  g.globalAlpha = 1;
}
