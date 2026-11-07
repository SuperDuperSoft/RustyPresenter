// Drawing on the live slide from the phone or tablet, like a sports telestrator: circle a word, underline a line.
// The live slide is a picture (/remote/live-slide) with a canvas on top. Finger, mouse and pen all come as Pointer
// Events. While drawing, the points go to the computer in small batches (every 40 ms) as 0–1 of the slide, so the
// room screen shows the line as it is drawn, at any screen size. The computer checks everything again.
// Ink either stays, or fades like a laser pointer: a few seconds after the finger lifts it fades out and is gone.

// The same six colours, sizes and highlighter as the computer (src/live/drawing.rs).
export const PALETTE = ['#ffd60a', '#ff3b30', '#34c759', '#0a84ff', '#ffffff', '#111111'];
const SIZES = [6, 12, 22];
const HIGHLIGHTER = 44;
const HIGHLIGHTER_ALPHA = 110 / 255;
const BATCH_MS = 40;
const FADE_OUT_MS = 500;

const $ = id => document.getElementById(id);
let app = null;                       // { t, toast, relogin } from remote.js
const t = (en, args) => app.t(en, args);

let color = PALETTE[0], size = 1, highlighter = false, fades = false;
let fadeMs = 3000;                    // Settings → "Fading ink disappears after", from the remote view
let strokes = [];                     // my lines on screen: { id, color, width, pen, ink, pts, sent, acked, ended }
let cur = null;                       // the line under the finger
let nextId = Date.now() % 1000000;    // stroke numbers, fresh after a reload
const fingers = new Set();            // pointers down: two or more is a pinch, which never draws
let pinched = false;
const queue = [];                     // commands waiting to go, one request at a time (so they arrive in order)
let busy = false, version = null, open = false;

export function initDraw(helpers) {
  app = helpers;
  $('swatches').replaceChildren(...PALETTE.map(c => {
    const b = document.createElement('button');
    b.className = 'sw'; b.style.background = c; b.dataset.c = c;
    b.onclick = () => { color = c; tools(); };
    return b;
  }));
  $('sizes').replaceChildren(...SIZES.map((w, i) => {
    const b = document.createElement('button');
    b.className = 'size'; b.title = t('Pen size');
    const dot = document.createElement('i'); dot.style.width = dot.style.height = (6 + i * 6) + 'px';
    b.append(dot);
    b.onclick = () => { size = i; highlighter = false; tools(); };
    return b;
  }));
  $('hlBtn').onclick = () => { highlighter = !highlighter; tools(); };
  $('inkStays').onclick = () => { fades = false; tools(); };
  $('inkFades').onclick = () => { fades = true; tools(); };
  $('undoBtn').onclick = undo;
  $('clearBtn').onclick = clearAll;
  $('doneBtn').onclick = closeDraw;
  const pad = $('pad');
  pad.addEventListener('pointerdown', down);
  pad.addEventListener('pointermove', move);
  pad.addEventListener('pointerup', up);
  pad.addEventListener('pointercancel', up);
  // No page scrolling, zooming or long-press menu while drawing.
  pad.addEventListener('touchstart', e => e.preventDefault(), { passive: false });
  pad.addEventListener('contextmenu', e => e.preventDefault());
  pad.addEventListener('wheel', e => e.preventDefault(), { passive: false });
  addEventListener('resize', () => { if (open) fit(); });
  setInterval(flush, BATCH_MS);
  tools();
}

function tools() {
  document.querySelectorAll('#swatches .sw').forEach(b => b.classList.toggle('sel', b.dataset.c === color));
  document.querySelectorAll('#sizes .size').forEach((b, i) => { b.classList.toggle('sel', !highlighter && i === size); b.style.color = color; });
  $('hlBtn').classList.toggle('sel', highlighter);
  $('inkStays').classList.toggle('sel', !fades);
  $('inkFades').classList.toggle('sel', fades);
}

export function openDraw(view) {
  open = true;
  $('draw').classList.add('on');
  version = view ? view.version : null;
  fit();
  loadSlide();
}

function closeDraw() {
  end();
  open = false;
  $('draw').classList.remove('on');
}

// A new remote view: a new slide means a new picture. `my_strokes` says which of my lines are still up.
export function onView(view) {
  if (view.fade_secs) fadeMs = view.fade_secs * 1000;
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

// The slide box: as big as fits, 16:9, the canvas at the screen's real pixels.
function fit() {
  const board = $('board'), box = $('slideBox'), pad = $('pad');
  const w = Math.min(board.clientWidth, board.clientHeight * 16 / 9);
  box.style.width = w + 'px';
  box.style.height = (w * 9 / 16) + 'px';
  const dpr = devicePixelRatio || 1;
  pad.width = Math.round(w * dpr);
  pad.height = Math.round(w * 9 / 16 * dpr);
  paint();
}

function loadSlide() {
  const w = Math.round($('pad').width) || 960;
  $('slideImg').src = `/remote/live-slide?w=${w}&v=${version}`;
}

// Where a pointer is, as 0–1 of the slide (kept on it when the finger slides off the edge).
function point(e) {
  const r = $('pad').getBoundingClientRect();
  const c = v => Math.min(1, Math.max(0, v));
  return [c((e.clientX - r.left) / r.width), c((e.clientY - r.top) / r.height)];
}

function down(e) {
  fingers.add(e.pointerId);
  if (fingers.size > 1) { pinched = true; return cancel(); }
  if (pinched || (e.pointerType === 'mouse' && e.button !== 0)) return;
  e.target.setPointerCapture(e.pointerId);
  cur = { id: nextId++, color, width: highlighter ? HIGHLIGHTER : SIZES[size], pen: highlighter ? 'highlighter' : 'pen', ink: fades ? 'fades' : 'stays', pts: [point(e)], sent: 0, acked: false, ended: null };
  strokes.push(cur);
  paint();
}

function move(e) {
  if (!cur || pinched || !fingers.has(e.pointerId)) return;
  // A pen gives more points than one per frame: take them all, for a smooth line.
  const all = e.getCoalescedEvents ? e.getCoalescedEvents() : [e];
  for (const ev of all.length ? all : [e]) cur.pts.push(point(ev));
  paint();
}

function up(e) {
  fingers.delete(e.pointerId);
  if (fingers.size === 0) pinched = false;
  end();
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
  paint();
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

// Draws my lines the way the room does: smooth curves through the points, round ends and corners, the highlighter
// see-through and even where it crosses itself (one path per line).
function paint() {
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
