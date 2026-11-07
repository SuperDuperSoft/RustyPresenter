import { initDraw, openDraw, onView, onMine } from '/static/draw.js';

const $ = id => document.getElementById(id);
// The words in the computer's language, from /i18n.json (listed in src/i18n/remote_strings.rs). `t` takes the English word and
// gives the shown text; `{name}` gaps are filled from `args`. A word not there stays English.
let T = {}, lang = null;
const t = (en, args = {}) => (T[en] || en).replace(/\{(\w+)\}/g, (m, k) => k in args ? args[k] : m);
const rid = () => Math.random().toString(36).slice(2) + Date.now().toString(36) + Math.random().toString(36).slice(2);
let es = null, V = null;

function toast(msg, good = false) { const el = $('toast'); el.textContent = msg; el.classList.toggle('good', good); el.style.display = 'block'; clearTimeout(el._h); el._h = setTimeout(() => el.style.display = 'none', 2500); }

// Reads the words again (at start, and when the computer's language changes) and puts them on the page.
async function loadWords() {
  const r = await fetch('/i18n.json').catch(() => null);
  if (r && r.ok) { T = await r.json().catch(() => T); lang = r.headers.get('content-language') || lang; }
  document.documentElement.lang = lang || 'en';
  // Elements marked `data-t`: their first text is the English word.
  document.querySelectorAll('[data-t]').forEach(el => {
    const node = el.firstChild;
    if (!node || node.nodeType !== Node.TEXT_NODE) return;
    if (el._en === undefined) el._en = node.nodeValue;
    const m = /^(\s*)(.*?)(\s*)$/s.exec(el._en);
    node.nodeValue = m[1] + t(m[2]) + m[3];
  });
  const b = s => { const e = document.createElement('b'); e.textContent = s; return e; };
  $('pairHelp').replaceChildren(...t('On the presentation computer, click {phone} → {pair}, then type the 6-digit code here.')
    .split(/(\{phone\}|\{pair\})/).map(p => p === '{phone}' ? b(t('Phone')) : p === '{pair}' ? b(t('Pair a phone')) : p));
  $('prev').textContent = '‹ ' + t('Back');
  $('next').textContent = t('Next') + ' ›';
  if (V) render();
}

async function post(env) {
  try {
    const r = await fetch('/remote/command', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(env) });
    if (r.status === 401) return start();
    const out = await r.json();
    if (!out.ok && out.error) {
      if (!out.error.startsWith('stale')) toast(out.error);
      else if (env.cmd === 'show') toast(t('The screen changed just now — tap the slide again.'));
      else if (env.cmd === 'jump' || env.cmd === 'run_macro') toast(t('The screen changed — try again.'));
    } else if (out.ok && env.cmd === 'run_macro') toast(t('Done'), true);
  } catch { toast(t('Not connected')); }
}
function send(cmd, extra = {}, withBase = false) {
  const env = { id: rid(), cmd, ...extra };
  if (withBase && V) env.base = V.version;
  if (navigator.vibrate) navigator.vibrate(12);
  post(env);
}

const phoneName = () => /iPhone/.test(navigator.userAgent) ? 'iPhone' : /Android/.test(navigator.userAgent) ? t('Android phone') : t('Phone');

// The QR code opens /remote#pair=<one-time secret>. Take it out of the address bar straight away (so it is not
// bookmarked, shared or reused), then pair with it: no typing.
async function pairFromLink() {
  const m = /^#pair=([A-Za-z0-9_-]+)$/.exec(location.hash);
  if (!location.hash) return false;
  history.replaceState(null, '', location.pathname + location.search);
  if (!m) return false;
  const st = await fetch('/remote/status').then(r => r.json()).catch(() => ({ paired: false }));
  if (st.paired) return false;
  const r = await fetch('/remote/pair', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ pair: m[1], name: phoneName() }) }).catch(() => null);
  if (r && r.ok) { toast(t('Connected'), true); return false; }
  await start();
  $('pairErr').textContent = t('That QR code has expired. On the computer, click Pair a phone again.');
  return true;
}

async function start() {
  const st = await fetch('/remote/status').then(r => r.json()).catch(() => ({ paired: false }));
  if (st.paired) { $('pair').style.display = 'none'; $('app').style.display = 'flex'; connect(); }
  else { $('pair').style.display = 'block'; $('app').style.display = 'none'; setTimeout(() => $('pin').focus(), 50); }
}

$('pairBtn').onclick = async () => {
  $('pairErr').textContent = '';
  const r = await fetch('/remote/pair', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ pin: $('pin').value.trim(), name: phoneName() }) });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) { $('pairErr').textContent = j.error || t('Could not connect.'); return; }
  start();
};
$('pin').addEventListener('keydown', e => { if (e.key === 'Enter') $('pairBtn').click(); });

function connect() {
  if (es) es.close();
  es = new EventSource('/remote/events');
  es.onopen = () => $('dot').classList.add('on');
  es.onmessage = e => {
    const m = JSON.parse(e.data);
    // FEAT-2: a tick sends only the timer lines.
    if (m.type === 'timers') { if (V) { V.timers = m.timers; renderAuto(); } return; }
    // Drawing: which of this phone's lines are still on the slide.
    if (m.type === 'drawing') { onMine(m.mine); return; }
    if (m.type !== 'remote') return;
    V = m;
    render();
    if (m.lang && m.lang !== lang) { lang = m.lang; loadWords(); }
  };
  es.onerror = async () => {
    $('dot').classList.remove('on');
    const st = await fetch('/remote/status').then(r => r.json()).catch(() => null);
    if (st && !st.paired) { es.close(); es = null; start(); }
  };
}

function render() {
  $('svc').textContent = V.service || t('Service');
  const air = V.on_air;
  $('now').textContent = air.logo ? t('(Logo)') : V.current && (air.slide || air.media) ? V.current.text : air.audio ? t('(Music playing)') : t('(Clear)');
  $('nowLabel').textContent = V.current && (air.slide || air.media) ? V.current.label : '';
  renderSections();
  renderAuto();
  $('nextText').textContent = V.next ? V.next.text : t('End of service');
  $('nextLabel').textContent = V.next ? `${V.next.title} · ${V.next.label}` : '';
  $('logoBtn').classList.toggle('on', air.logo);
  $('media').classList.toggle('on', !!V.media);
  $('drawBtn').classList.toggle('on', !!V.may_draw);
  onView(V);
  if (V.media) $('mediaBtn').textContent = V.media.paused ? '▶ ' + t('Play video') : '❚❚ ' + t('Pause video');
  const liveId = V.cursor && V.cursor.item_id;
  $('items').replaceChildren(...V.items.map((it, i) => {
    const row = document.createElement('div');
    row.className = 'it' + (it.id === liveId ? ' live' : '');
    row.innerHTML = `<span class="n">${i + 1}</span><span class="t"></span><span class="k"></span>`;
    row.querySelector('.t').textContent = it.title;
    row.querySelector('.k').textContent = it.count;
    row.onclick = () => openSheet(it);
    return row;
  }));
}

// The live song's sections: a tap jumps the screen to exactly that one.
function renderSections() {
  const secs = V.sections || [];
  $('secs').style.display = secs.length ? 'flex' : 'none';
  $('secs').replaceChildren(...secs.map(o => {
    const b = document.createElement('button');
    b.className = 'sec' + (o.live ? ' live' : '');
    b.style.setProperty('--c', o.color);
    b.textContent = o.short;
    b.title = o.label;
    b.onclick = () => send('jump', { item_id: V.cursor.item_id, kind: o.kind, occurrence: o.occurrence }, true);
    return b;
  }));
}

// FEAT-2: messages and macros marked for the phone, and the timers marked "Show on phone".
function renderAuto() {
  const msgs = V.messages || [], timers = V.timers || [], macros = V.macros || [];
  $('auto').classList.toggle('on', !!(msgs.length || timers.length || macros.length || V.message_shown));
  $('msgHead').style.display = msgs.length || V.message_shown ? '' : 'none';
  const mb = msgs.map(m => { const b = document.createElement('button'); b.textContent = m.name; b.onclick = () => askMessage(m); return b; });
  if (V.message_shown) { const h = document.createElement('button'); h.className = 'hide'; h.textContent = t('Hide message'); h.onclick = () => send('hide_message'); mb.push(h); }
  $('msgs').replaceChildren(...mb);
  $('timerHead').style.display = timers.length ? '' : 'none';
  $('timers').replaceChildren(...timers.map(x => {
    const d = document.createElement('div'); d.className = 'timer';
    const n = document.createElement('span'); n.textContent = x.name;
    const v = document.createElement('span'); v.textContent = x.text; if (x.over) v.className = 'over';
    d.append(n, v); return d;
  }));
  $('macroHead').style.display = macros.length ? '' : 'none';
  const colors = { amber: '#f5b94a', red: '#ff6b6b', orange: '#ff9f43', green: '#3ccf91', teal: '#2ec4b6', blue: '#5aa9ff', purple: '#b18cff', gray: '#a0a4ab' };
  $('macros').replaceChildren(...macros.map(m => {
    const b = document.createElement('button'); b.className = 'macro'; b.style.background = colors[m.color] || colors.amber; b.textContent = m.name;
    b.onclick = () => ask(t('Run “{name}”?', { name: m.name }), [], t('Run'), () => {
      const env = { id: rid(), cmd: 'run_macro', macro: m.id, seen_clear: V.clear_epoch };
      if (m.relative) env.base = V.version;
      post(env);
    });
    return b;
  }));
}
function askMessage(m) {
  ask(m.name, m.fields, t('Show'), vals => send('show_message', { template: m.id, values: vals, seen_clear: V.clear_epoch }));
}
// A small sheet: a title, one box per field, a button.
function ask(title, fields, go, done) {
  $('askTitle').textContent = title;
  $('askFields').replaceChildren(...fields.map(f => { const i = document.createElement('input'); i.placeholder = f; i.dataset.f = f; return i; }));
  $('askGo').textContent = go;
  $('askGo').onclick = () => {
    const vals = {}; $('askFields').querySelectorAll('input').forEach(i => vals[i.dataset.f] = i.value);
    $('ask').classList.remove('on'); done(vals);
  };
  $('ask').classList.add('on');
  const first = $('askFields').querySelector('input'); if (first) setTimeout(() => first.focus(), 50);
}
$('askCancel').onclick = () => $('ask').classList.remove('on');

function openSheet(it) {
  $('sheetTitle').textContent = it.title;
  $('sheetSlides').replaceChildren(...it.slides.map((s, idx) => {
    const b = document.createElement('button');
    b.innerHTML = '<small></small>';
    b.querySelector('small').textContent = s.label;
    b.append(s.text);
    b.onclick = () => { closeSheet(); post({ id: rid(), base: V.version, cmd: 'show', item_id: it.id, slide: idx }); };
    return b;
  }));
  $('sheet').classList.add('on');
}
function closeSheet() { $('sheet').classList.remove('on'); }
$('sheetCancel').onclick = closeSheet;
$('sheet').onclick = e => { if (e.target.id === 'sheet') closeSheet(); };
$('next').onclick = () => send('next', {}, true);
$('prev').onclick = () => send('prev', {}, true);
document.querySelectorAll('.clears button').forEach(b => b.onclick = () => send(b.dataset.c));
$('drawBtn').onclick = () => openDraw(V);
initDraw({ t, toast, relogin: start });
$('mediaBtn').onclick = () => V && V.media && send('media_pause', { inst: V.media.inst, paused: !V.media.paused });

loadWords().then(() => pairFromLink()).catch(() => false).then(shown => { if (!shown) start(); });
