import { initDraw, openDraw, onView, onMine } from '/static/draw.js';

const $ = id => document.getElementById(id);
// The words in the computer's language, from /i18n.json (listed in src/i18n/remote_strings.rs). `t` takes the English word and
// gives the shown text; `{name}` gaps are filled from `args`. A word not there stays English.
let T = {}, lang = null, words = null;
const t = (en, args = {}) => (T[en] || en).replace(/\{(\w+)\}/g, (m, k) => k in args ? args[k] : m);
const rid = () => Math.random().toString(36).slice(2) + Date.now().toString(36) + Math.random().toString(36).slice(2);
let es = null, V = null;

function toast(msg, good = false) { const el = $('toast'); el.textContent = msg; el.classList.toggle('good', good); el.style.display = 'block'; clearTimeout(el._h); el._h = setTimeout(() => el.style.display = 'none', 2500); }

// Reads the words again (at start, and when the computer's language or a translation changes) and puts them on the page.
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
    .split(/(\{phone\}|\{pair\})/).map(p => p === '{phone}' ? b(t('Phone')) : p === '{pair}' ? b(t('Pair a phone or tablet')) : p));
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

async function requestPair(code, password = '') {
  try {
    const r = await fetch('/remote/pair', {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ ...code, password, name: phoneName() }),
    });
    if (r.ok) return { ok: true };
    const body = await r.json().catch(() => ({}));
    return { error: body.error || t('Could not connect.'), retryWithPin: body.retry_with_pin === true };
  } catch {
    return { error: t('Not connected') };
  }
}

// The QR code opens /remote#pair=<one-time secret>. Take it out of the address bar straight away (so it is not
// bookmarked, shared or reused), then pair with it: no typing.
async function pairFromLink() {
  const m = /^#pair=([A-Za-z0-9_-]+)$/.exec(location.hash);
  if (!location.hash) return false;
  history.replaceState(null, '', location.pathname + location.search);
  if (!m) return false;
  const st = await fetch('/remote/status').then(r => r.json()).catch(() => ({ paired: false }));
  if (st.paired) return false;
  // N31: with a password the QR code still saves typing the code; the password is asked first.
  if (st.password) { qrSecret = m[1]; return false; }
  const result = await requestPair({ pair: m[1] });
  if (result.ok) { toast(t('Connected'), true); return false; }
  if (!result.retryWithPin) qrSecret = m[1];
  await start();
  $('pairErr').textContent = result.error;
  return true;
}

let qrSecret = null;

async function start() {
  const st = await fetch('/remote/status').then(r => r.json()).catch(() => ({ paired: false }));
  if (st.paired) { $('pair').style.display = 'none'; $('app').style.display = 'flex'; connect(); }
  else {
    if (es) { es.close(); es = null; }
    $('pair').style.display = 'block'; $('app').style.display = 'none';
    // N31: the password field when the church set one; the code field unless the QR code gave the secret.
    for (const id of ['pw', 'pwHelp']) $(id).style.display = st.password ? 'block' : 'none';
    $('pin').style.display = qrSecret ? 'none' : 'block';
    $('pw').placeholder = t('Password');
    const first = qrSecret ? (st.password ? 'pw' : 'pairBtn') : 'pin';
    setTimeout(() => $(first).focus(), 50);
  }
}

$('notesBtn').onclick = toggleNotes;
$('pairBtn').onclick = async () => {
  const button = $('pairBtn');
  if (button.disabled) return;
  button.disabled = true;
  $('pairErr').textContent = '';
  try {
    const code = qrSecret ? { pair: qrSecret } : { pin: $('pin').value.trim() };
    const result = await requestPair(code, $('pw').value);
    if (!result.ok) {
      $('pairErr').textContent = result.error;
      if (qrSecret && result.retryWithPin) { qrSecret = null; await start(); }
      return;
    }
    qrSecret = null;
    $('pw').value = '';
    await start();
  } finally {
    button.disabled = false;
  }
};
$('pin').addEventListener('keydown', e => { if (e.key === 'Enter') $('pairBtn').click(); });
$('pw').addEventListener('keydown', e => { if (e.key === 'Enter') $('pairBtn').click(); });

function connect() {
  if (es) es.close();
  const source = es = new EventSource('/remote/events');
  source.onopen = () => { if (source === es) $('dot').classList.add('on'); };
  source.onmessage = e => {
    if (source !== es) return;
    const m = JSON.parse(e.data);
    // FEAT-2: a tick sends only the timer lines.
    if (m.type === 'timers') { if (V) { V.timers = m.timers; V.music = m.music; renderAuto(); } return; }
    // Drawing: which of this phone's lines are still on the slide.
    if (m.type === 'drawing') { onMine(m.mine); return; }
    if (m.type !== 'remote') return;
    V = m;
    render();
    if (m.lang && (m.lang !== lang || m.words !== words)) { lang = m.lang; words = m.words; loadWords(); }
  };
  source.onerror = async () => {
    if (source !== es) return;
    $('dot').classList.remove('on');
    const st = await fetch('/remote/status').then(r => r.json()).catch(() => null);
    if (source === es && st && !st.paired) { source.close(); es = null; start(); }
  };
}

// Presenter notes on or off, remembered on this phone.
let notesOn = true;
try { notesOn = localStorage.getItem('rp-notes') !== 'off'; } catch (e) {}
function renderNotesBtn() { $('notesBtn').textContent = notesOn ? t('Hide presenter notes') : t('Show presenter notes'); $('notesBtn').classList.toggle('on', notesOn); }
function toggleNotes() {
  notesOn = !notesOn;
  try { localStorage.setItem('rp-notes', notesOn ? 'on' : 'off'); } catch (e) {}
  if (V) render(); else renderNotesBtn();
}

// The live picture's slide ("item:slide") and whether it was a picture, so only a new slide plays a transition.
let shownKey = '', shownPic = false;
// Each way's step, as the room has it (src/design/mod.rs `Dir::step`).
const STEP = { left: [-1, 0], right: [1, 0], up: [0, -1], down: [0, 1] };
// How the old picture is cut as a wipe's edge goes each way (top right bottom left, the room's `geo`).
const WIPE = { left: p => `inset(0 ${p}% 0 0)`, right: p => `inset(0 0 0 ${p}%)`, up: p => `inset(0 0 ${p}% 0)`, down: p => `inset(${p}% 0 0 0)` };

// Puts `src` on the live picture, playing `tr` ({ kind, ms, dir }: fade, cut, black, push, wipe, cover or uncover; the
// rest fade) from the one shown.
function showPicture(src, tr) {
  stopAnim();
  const img = $('nowImg'), box = $('picBox');
  box.querySelectorAll('img.old').forEach(o => o.remove());
  img.getAnimations().forEach(a => a.cancel());
  img.style.position = img.style.zIndex = '';
  const ms = tr ? Math.max(0, Math.min(tr.ms || 0, 5000)) : 0;
  if (!tr || tr.kind === 'cut' || !ms || !img.complete || !img.naturalWidth || !img.animate) { img.src = src; return; }
  const old = img.cloneNode(); old.removeAttribute('id'); old.className = 'old'; old.alt = '';
  box.append(old);
  img.src = src;
  const go = () => {
    if (!old.isConnected) return;
    const opts = { duration: ms, easing: 'ease-in-out', fill: 'forwards' };
    let a;
    if (tr.kind === 'push') {
      const [x, y] = STEP[tr.dir] || STEP.left;
      a = old.animate([{ transform: 'none' }, { transform: `translate(${x * 100}%, ${y * 100}%)` }], opts);
      img.animate([{ transform: `translate(${-x * 100}%, ${-y * 100}%)` }, { transform: 'none' }], opts);
    } else if (tr.kind === 'cover') {
      const [x, y] = STEP[tr.dir] || STEP.left;
      img.style.position = 'relative'; img.style.zIndex = 1;
      a = img.animate([{ transform: `translate(${-x * 100}%, ${-y * 100}%)` }, { transform: 'none' }], opts);
      const finish = () => {
        if (!old.isConnected) return;
        img.style.position = img.style.zIndex = '';
        old.remove();
      };
      a.onfinish = finish;
      setTimeout(finish, ms + 50);
      return;
    } else if (tr.kind === 'uncover') {
      const [x, y] = STEP[tr.dir] || STEP.left;
      a = old.animate([{ transform: 'none' }, { transform: `translate(${x * 100}%, ${y * 100}%)` }], opts);
    } else if (tr.kind === 'wipe') {
      const cut = WIPE[tr.dir] || WIPE.left;
      a = old.animate([{ clipPath: cut(0) }, { clipPath: cut(100) }], opts);
    } else if (tr.kind === 'black') {
      a = old.animate([{ opacity: 1 }, { opacity: 0 }], { ...opts, duration: ms / 2 });
      img.animate([{ opacity: 0 }, { opacity: 0, offset: 0.5 }, { opacity: 1 }], { duration: ms, easing: 'ease-in-out' });
    } else {
      a = old.animate([{ opacity: 1 }, { opacity: 0 }], opts);
    }
    setTimeout(() => old.remove(), ms + 50);
    if (a) a.onfinish = () => old.remove();
  };
  (img.decode ? img.decode() : Promise.resolve()).then(go, go);
}

// A designed slide's click group playing on the live picture ({ layer, old }), as the room plays it.
let animRun = null;
function stopAnim() {
  if (!animRun) return;
  animRun.layer.remove(); animRun.old.remove();
  $('nowImg').style.visibility = '';
  animRun = null;
}

// Plays the live designed slide's click group from the room's pictures: the picture under it and one per layer
// (`/remote/live-anim/pic`), each moving layer by the frames the server sampled (`src/network/live_anim.rs`). A
// new slide's pictures first come in with its transition over the old picture. `src` (the slide once played)
// shows when it ends; if the group can't be fetched, `src` comes in with `tr` as usual.
async function playAnim(src, tr, w) {
  const img = $('nowImg'), box = $('picBox');
  const still = () => img._src === src;
  let a = null;
  try { const r = await fetch(`/remote/live-anim?w=${w}`, { cache: 'no-store' }); if (r.ok) a = await r.json(); } catch (e) {}
  if (!still()) return;
  if (!a || !a.layers || !img.animate) { showPicture(src, tr); return; }
  const layer = document.createElement('div'); layer.className = 'anim';
  const pics = [], moves = [];
  const add = (parent, i, at) => {
    const p = new Image(); p.alt = ''; p.src = `/remote/live-anim/pic?v=${a.v}&w=${a.w}&i=${i}`;
    if (at) Object.assign(p.style, { left: at[0] + '%', top: at[1] + '%', width: at[2] + '%', height: at[3] + '%' });
    parent.append(p); pics.push(p); return p;
  };
  add(layer, 0);
  for (const l of a.layers) {
    const wrap = document.createElement('div'); wrap.className = 'lay'; layer.append(wrap);
    const p = add(wrap, l.i, l.at);
    if (!l.f) continue;
    p.style.transformOrigin = `${l.o[0]}% ${l.o[1]}%`;
    moves.push([p, l.f.map(([offset, opacity, x, y, r, s]) => ({ offset, opacity, transform: `translate(${x}%, ${y}%) rotate(${r}deg) scale(${s})` }))]);
    if (l.c) moves.push([wrap, l.c.map(([offset, t, r, b, lf]) => ({ offset, clipPath: `inset(${t}% ${r}% ${b}% ${lf}%)` }))]);
  }
  await Promise.all(pics.map(p => (p.decode ? p.decode() : Promise.resolve()).catch(() => {})));
  if (!still()) return;
  const old = img.cloneNode(); old.removeAttribute('id'); old.className = 'old'; old.alt = '';
  showPicture(src, null);
  const lead = Math.max(0, Math.min(a.lead || 0, 5000));
  // The old picture stays under the new one's pictures while they come in (only a new slide has a lead).
  if (!lead) old.style.display = 'none';
  img.style.visibility = 'hidden';
  box.append(old, layer);
  animRun = { layer, old };
  if (lead) layer.animate([{ opacity: 0 }, { opacity: 1 }], { duration: lead, easing: 'ease-in-out' });
  // The frames are already eased: linear between them.
  for (const [el, k] of moves) el.animate(k, { duration: Math.max(1, a.ms), delay: lead, fill: 'both' });
  const run = animRun;
  setTimeout(() => { if (animRun === run) stopAnim(); }, lead + a.ms + 50);
}

function render() {
  $('svc').textContent = V.service || t('Service');
  const air = V.on_air;
  $('now').textContent = air.logo ? t('(Logo)') : V.current && (air.slide || air.media) ? V.current.text : air.audio ? t('(Music playing)') : t('(Clear)');
  $('nowLabel').textContent = V.current && (air.slide || air.media) ? V.current.label : '';
  // A picture slide (PowerPoint, PDF) shows itself; its presenter notes show below when the phone has them on.
  const shown = V.current && (air.slide || air.media) && !air.logo;
  const pic = !!(shown && V.current.picture);
  $('nowImg').closest('.now').classList.toggle('pic', pic);
  const slideKey = shown && V.cursor ? `${V.cursor.item_id}:${V.cursor.slide}` : '';
  if (pic) {
    const w = Math.min(1920, Math.round(innerWidth * (devicePixelRatio || 1)));
    const src = `/remote/live-slide?w=${w}&v=${V.version}`;
    // A new slide plays its own transition (from PowerPoint or the slide editor), as the room does.
    const tr = slideKey !== shownKey && shownPic ? V.current.transition : null;
    if ($('nowImg')._src !== src) {
      $('nowImg')._src = src;
      // A designed slide's click group plays as the room plays it.
      if (V.current.anim) playAnim(src, tr, w); else showPicture(src, tr);
    }
  }
  shownKey = slideKey; shownPic = pic;
  const notes = shown ? (V.current.notes || '') : '';
  $('notesText').textContent = notes;
  $('pnotes').classList.toggle('on', notesOn && notes !== '');
  renderNotesBtn();
  renderSections();
  renderFollow();
  renderAuto();
  // N29: a slide on the stage screen only.
  $('stage').classList.toggle('on', !!V.stage_only);
  $('stageText').textContent = V.stage_only ? V.stage_only.text : '';
  $('nextText').textContent = V.next ? V.next.text : t('End of service');
  $('nextLabel').textContent = V.next ? `${V.next.title} · ${V.next.label}` : '';
  $('logoBtn').classList.toggle('on', air.logo);
  $('media').classList.toggle('on', !!V.media);
  $('drawBtn').classList.toggle('on', !!V.may_draw);
  $('nowImg').closest('.now').classList.toggle('candraw', !!V.may_draw);
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
  syncSheet();
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

// 1.3: the live song follows the singers. The chip goes Manual → Suggest → Auto; Hold keeps the slide still.
const FOLLOW_NEXT = { manual: 'suggest', suggest: 'auto', auto: 'manual' };
const FOLLOW_WORDS = { manual: 'Manual', suggest: 'Suggest', auto: 'Auto' };
const LIGHT_WORDS = { off: 'Not listening', listening: 'Listening…', sure: 'Following the singers', unsure: 'Not sure — staying on this slide', holding: 'Holding — the slides stay put', blank: 'Nobody singing — words off' };
function renderFollow() {
  const f = V.follow;
  $('follow').classList.toggle('on', !!(f && V.cursor));
  if (!f || !V.cursor) return;
  const chip = $('followChip');
  chip.textContent = t(FOLLOW_WORDS[f.mode] || 'Manual');
  chip.className = f.mode;
  chip.onclick = () => send('follow_mode', { item_id: V.cursor.item_id, mode: FOLLOW_NEXT[f.mode] || 'manual' });
  $('followLine').textContent = f.mode === 'manual' ? '' : (f.line || t(LIGHT_WORDS[f.light] || ''));
  const hold = $('followHold');
  hold.style.display = f.mode !== 'manual' || f.holding ? '' : 'none';
  hold.textContent = f.holding ? t('Holding') : t('Hold');
  hold.classList.toggle('on', !!f.holding);
  hold.onclick = () => send('follow_hold', { on: !f.holding });
}

// FEAT-2: messages and macros marked for the phone, and the timers marked "Show on phone".
function renderAuto() {
  const msgs = V.messages || [], timers = V.timers || [], macros = V.macros || [];
  $('auto').classList.toggle('on', !!(msgs.length || timers.length || macros.length || V.message_shown || V.music));
  renderMusic();
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
// N13: the walk-in music, playing apart from the slides.
function renderMusic() {
  const m = V.music;
  $('musicHead').style.display = m ? '' : 'none';
  $('music').classList.toggle('on', !!m);
  if (!m) return;
  const playing = m.state === 'playing';
  $('musicTitle').textContent = m.state === 'stopped' ? t('Not playing') : m.title;
  $('musicTime').textContent = m.state === 'stopped' ? '' : t('{time} left', { time: clock(m.left_ms) });
  $('musicPlay').textContent = playing ? '❚❚ ' + t('Pause') : '▶ ' + t('Play');
  $('musicNext').disabled = $('musicStop').disabled = m.state === 'stopped';
  if (document.activeElement !== $('musicVol')) $('musicVol').value = m.volume;
}
const clock = ms => { const s = Math.floor(ms / 1000); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };
async function music(op, body = {}) {
  try {
    const r = await fetch('/remote/music', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ op, ...body }) });
    if (r.status === 401) return start();
    const out = await r.json();
    if (!out.ok && out.error) toast(out.error);
  } catch { toast(t('Not connected')); }
}
$('musicPlay').onclick = () => music('toggle');
$('musicNext').onclick = () => music('next');
$('musicStop').onclick = () => music('stop');
$('musicVol').oninput = e => music('volume', { volume: Number(e.target.value) });

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

// The item whose slide list is open, as it was when the list was drawn.
let sheetItem = null;
const sheetKey = it => JSON.stringify([it.title, it.slides.map(s => [s.label, s.text])]);

// The open slide list follows the service: it closes when its item is gone, and is drawn again when the item's
// slides changed, so a tap always shows the slide it names.
function syncSheet() {
  if (!sheetItem) return;
  const now = V.items.find(i => i.id === sheetItem.id);
  if (!now) { closeSheet(); return; }
  if (sheetKey(now) === sheetKey(sheetItem)) return;
  const only = $('sheetOnly').checked;
  openSheet(now);
  $('sheetOnly').checked = only;
}

function openSheet(it) {
  sheetItem = it;
  $('sheetTitle').textContent = it.title;
  $('sheetSlides').replaceChildren(...it.slides.map((s, idx) => {
    const b = document.createElement('button');
    b.innerHTML = '<small></small>';
    b.querySelector('small').textContent = s.label;
    b.append(s.text);
    b.onclick = () => {
      closeSheet();
      // N29: with "Stage screen only" ticked, only the band sees it.
      if ($('sheetOnly').checked) post({ id: rid(), cmd: 'stage_only', item_id: it.id, slide: idx });
      else post({ id: rid(), base: V.version, cmd: 'show', item_id: it.id, slide: idx });
    };
    return b;
  }));
  $('sheetOnly').checked = false;
  $('sheet').classList.add('on');
}
function closeSheet() { sheetItem = null; $('sheet').classList.remove('on'); }
$('sheetCancel').onclick = closeSheet;
$('sheet').onclick = e => { if (e.target.id === 'sheet') closeSheet(); };
$('next').onclick = () => send('next', {}, true);
$('prev').onclick = () => send('prev', {}, true);
document.querySelectorAll('.clears button').forEach(b => b.onclick = () => send(b.dataset.c));
$('drawBtn').onclick = () => openDraw(V);
// A tap on the live picture draws on it too.
$('picBox').onclick = () => { if (V && V.may_draw) openDraw(V); };
initDraw({ t, toast, relogin: start, go: back => send(back ? 'prev' : 'next', {}, true) });
$('stageOff').onclick = () => send('stage_only');
$('mediaBtn').onclick = () => V && V.media && send('media_pause', { inst: V.media.inst, paused: !V.media.paused });

loadWords().then(() => pairFromLink()).catch(() => false).then(shown => { if (!shown) start(); });
