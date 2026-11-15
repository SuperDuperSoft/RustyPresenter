// N31: the stage screen on a phone or tablet, read only. The computer sends the stage layout's boxes filled in
// (`src/network/stage_view.rs`); this draws them like the stage window does and shrinks words until they fit.
const $ = id => document.getElementById(id);
let T = {};
const t = en => T[en] || en;
let es = null, version = -1;

async function loadWords() {
  const r = await fetch('/i18n.json').catch(() => null);
  if (r && r.ok) { T = await r.json().catch(() => T); document.documentElement.lang = r.headers.get('content-language') || 'en'; }
  document.querySelectorAll('[data-t]').forEach(el => { el._en ??= el.textContent; el.textContent = t(el._en.trim()); });
  $('pw').placeholder = t('Password');
}

function show(which) {
  for (const id of ['login', 'off']) $(id).style.display = which === id ? 'block' : 'none';
  $('screen').style.display = which === 'screen' ? 'block' : 'none';
}

async function start() {
  const st = await fetch('/stage-view/status').then(r => r.json()).catch(() => null);
  if (!st) { $('lost').style.display = 'block'; return setTimeout(start, 3000); }
  if (!st.on) { $('offText').textContent = st.off_text; show('off'); return setTimeout(start, 10000); }
  if (!st.allowed) { show('login'); setTimeout(() => $('pw').focus(), 50); return; }
  show('screen');
  connect();
}

$('open').onclick = async () => {
  $('err').textContent = '';
  const r = await fetch('/stage-view/login', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ password: $('pw').value }) }).catch(() => null);
  const j = r ? await r.json().catch(() => ({})) : {};
  if (!r || !r.ok) { $('err').textContent = j.error || t('Not connected'); return; }
  $('pw').value = '';
  start();
};
$('pw').addEventListener('keydown', e => { if (e.key === 'Enter') $('open').click(); });

function connect() {
  if (es) es.close();
  es = new EventSource('/stage-view/events');
  es.onopen = () => { $('lost').style.display = 'none'; };
  es.onmessage = e => draw(JSON.parse(e.data));
  // Turned off, signed out or the computer gone: ask again.
  es.onerror = () => { $('lost').style.display = 'block'; es.close(); es = null; setTimeout(start, 3000); };
}

const VALIGN = { top: 'flex-start', middle: 'center', bottom: 'flex-end' };

function draw(v) {
  document.body.style.background = v.background;
  $('screen').style.background = v.background;
  const host = $('cells');
  host.replaceChildren(...v.cells.map(c => cell(c, v.version)));
  // 1.3: "Auto: following — Chorus 2" while the song follows the singers.
  const f = $('follow');
  f.textContent = v.follow || '';
  f.style.display = v.follow ? 'block' : 'none';
  const b = $('banner');
  b.textContent = v.banner || '';
  b.style.display = v.banner ? 'flex' : 'none';
  requestAnimationFrame(fitAll);
}

function cell(c, ver) {
  const el = document.createElement('div');
  el.className = 'cell' + (c.kind === 'message' ? ' message' : '');
  Object.assign(el.style, { left: c.x + '%', top: c.y + '%', width: c.w + '%', height: c.h + '%', background: c.fill || 'transparent' });
  if (c.kind === 'shape') {
    el.style.borderRadius = c.shape === 'rounded' ? '3cqh' : c.shape === 'ellipse' ? '50%' : '0';
    if (c.shape === 'line') { el.style.background = 'transparent'; const l = document.createElement('div'); Object.assign(l.style, { margin: 'auto 0', height: 'max(2px, .6cqh)', background: c.color }); el.style.justifyContent = 'center'; el.append(l); }
    return el;
  }
  if (c.caption) { const cap = document.createElement('div'); cap.className = 'cap'; cap.textContent = c.caption; cap.style.textAlign = c.align; el.append(cap); }
  if (c.kind === 'picture') {
    const img = document.createElement('img');
    // The live slide or the next one, drawn by the computer; asked again when the slide changes.
    img.src = '/stage-view/' + (c.src === 'live' ? 'live' : 'next') + '.jpg?w=640&v=' + ver;
    img.alt = '';
    el.append(img);
    return el;
  }
  const w = document.createElement('div');
  w.className = 'words';
  w.textContent = c.text;
  Object.assign(w.style, {
    color: c.red ? '#ff4f55' : c.kind === 'message' ? '#1a1405' : c.color, opacity: c.dim ? .35 : 1,
    fontWeight: c.bold || c.kind === 'message' ? 650 : 500, textAlign: c.align, justifyContent: VALIGN[c.valign] || 'center',
  });
  w.dataset.max = c.max; w.dataset.min = c.min;
  el.append(w);
  return el;
}

// The smallest words, in % of the screen's height: below a box's own smallest size, but all the words are seen
// (as the stage window, `ui::stage::FLOOR`).
const FLOOR = 1.2;

// The biggest size from the box's top size down to its smallest, in 0.4 % steps, at which the words fit; when none
// does, on down in 0.2 % steps to FLOOR, so long words (notes) are made smaller rather than cut off.
function fitAll() {
  const over = w => w.scrollHeight > w.clientHeight + 1 || w.scrollWidth > w.clientWidth + 1;
  for (const w of document.querySelectorAll('.words')) {
    const max = +w.dataset.max, min = +w.dataset.min;
    let s = max;
    w.style.fontSize = s + 'cqh';
    while (s > min && over(w)) { s = Math.max(min, s - 0.4); w.style.fontSize = s + 'cqh'; }
    const floor = Math.min(min, FLOOR);
    while (s > floor && over(w)) { s = Math.max(floor, s - 0.2); w.style.fontSize = s + 'cqh'; }
  }
}
addEventListener('resize', () => requestAnimationFrame(fitAll));

// Keep a tablet's screen on while it shows the stage.
async function stayAwake() { try { await navigator.wakeLock?.request('screen'); } catch {} }
document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'visible') stayAwake(); });
stayAwake();

loadWords().finally(start);
