// RustyPresenter operator window.
import { makeCanvas, drawStatic, slideToLayers, assetUrl } from '/static/render.js';

// ---------- tiny helpers ----------
const $ = s => document.querySelector(s);
function h(tag, attrs = {}, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k === 'html') el.innerHTML = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'style') el.style.cssText = v;
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const kid of kids.flat()) if (kid != null && kid !== false) el.append(kid.nodeType ? kid : document.createTextNode(kid));
  return el;
}
const svg = (inner, cls = '') => { const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); s.setAttribute('viewBox', '0 0 24 24'); if (cls) s.setAttribute('class', cls); s.innerHTML = inner; return s; };
const I = {
  song: '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
  scripture: '<path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20"/><path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z"/>',
  video: '<rect x="2" y="5" width="15" height="14" rx="2"/><path d="m17 10 5-3v10l-5-3z"/>',
  image: '<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-5-5L5 21"/>',
  audio: '<path d="M11 5 6 9H2v6h4l5 4z"/><path d="M15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14"/>',
  deck: '<rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/>',
  text: '<path d="M4 7V4h16v3M9 20h6M12 4v16"/>',
  edit: '<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/>',
  trash: '<path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6"/>',
  play: '<path d="M7 4v16l13-8z" fill="currentColor" stroke="none"/>',
  pause: '<rect x="6" y="4" width="4" height="16" fill="currentColor" stroke="none"/><rect x="14" y="4" width="4" height="16" fill="currentColor" stroke="none"/>',
  close: '<path d="M18 6 6 18M6 6l12 12"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  upload: '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M17 8l-5-5-5 5M12 3v12"/>',
  restart: '<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5"/>',
  dots: '<circle cx="12" cy="5" r="1.5"/><circle cx="12" cy="12" r="1.5"/><circle cx="12" cy="19" r="1.5"/>',
  phone: '<rect x="7" y="2" width="10" height="20" rx="2"/><path d="M11 18h2"/>',
  copy: '<rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>',
};
const KIND_NAME = { song: 'Song', scripture: 'Scripture', video: 'Video', image: 'Picture', audio: 'Audio', deck: 'Slides', text: 'Announcement' };
const uuid = () => (crypto.randomUUID ? crypto.randomUUID() : Math.random().toString(36).slice(2) + Date.now().toString(36));
const fmtTime = s => { s = Math.max(0, Math.floor(s || 0)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };
const ext = n => (n.split('.').pop() || '').toLowerCase();

async function api(path, body) {
  const r = await fetch('/api/do/' + path, { method: 'POST', headers: { 'content-type': 'application/json', 'x-rp': '1' }, body: JSON.stringify(body ?? {}) });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.error || 'Something went wrong.');
  return j;
}
function apiText(path, text) {
  return fetch('/api/do/' + path, { method: 'POST', headers: { 'content-type': 'text/plain', 'x-rp': '1' }, body: text })
    .then(async r => { const j = await r.json().catch(() => ({})); if (!r.ok) throw new Error(j.error || 'Import failed.'); return j; });
}

function toast(msg, err = false) {
  const t = h('div', { class: 'toast' + (err ? ' err' : '') }, msg);
  $('#toasts').append(t);
  setTimeout(() => t.remove(), err ? 6000 : 2600);
}
const guard = fn => async (...a) => { try { return await fn(...a); } catch (e) { toast(e.message || String(e), true); } };

// ---------- state ----------
let S = null;          // /api/state
let L = null;          // latest live message
let selectedId = null;
let translation = localStorage.getItem('rp.translation') || 'KJV';
let bibleArmed = null; // item id ready for Enter = take live
let gridKey = '';

async function refresh() {
  S = await (await fetch('/api/state')).json();
  if (!S.items.some(i => i.id === selectedId)) selectedId = (L && L.cursor && S.items.some(i => i.id === L.cursor.item_id)) ? L.cursor.item_id : (S.items[0] && S.items[0].id) || null;
  renderTop(); renderService(); renderGrid(); renderLive(); renderJobs();
}
let refreshTimer = null;
const refreshSoon = () => { clearTimeout(refreshTimer); refreshTimer = setTimeout(guard(refresh), 80); };

// ---------- live commands ----------
async function cmd(name, extra = {}, withBase = false) {
  const env = { id: uuid(), cmd: name, ...extra };
  if (withBase && L) env.base = L.version;
  const out = await api('command', env).catch(e => ({ ok: false, error: e.message }));
  if (!out.ok && out.error && !out.error.startsWith('stale')) toast(out.error, !/^(End|Start) of service$/.test(out.error));
  return out;
}
const next = () => cmd('next', {}, true);
const prev = () => cmd('prev', {}, true);
const show = (item_id, slide) => cmd('show', { item_id, slide });

// ---------- top bar ----------
function renderTop() {
  const svc = S.data.services.find(s => s.id === S.service_id);
  $('#serviceName').textContent = svc ? svc.name : 'Service';
  const tr = $('#translations');
  tr.replaceChildren(...S.translations.map(t => h('button', {
    type: 'button', class: (t === translation ? 'on' : '') + (S.available_translations.includes(t) ? '' : ' off'),
    title: S.available_translations.includes(t) ? t : `${t} needs a licensed text source (Settings → Bible)`,
    onclick: () => { translation = t; localStorage.setItem('rp.translation', t); renderTop(); $('#bibleInput').focus(); },
  }, t)));
  const pill = (on, label, title, warn) => h('div', { class: 'pill' + (on ? ' on' : warn ? ' warn' : ''), title }, h('i'), h('span', {}, label));
  const roomOpen = S.windows.includes('room');
  $('#status').replaceChildren(...[
    pill(S.room_connected, 'Room', S.room_connected ? 'Room screen is showing' : roomOpen ? 'Room screen is starting…' : 'Room screen is not open — click Screens', !S.room_connected),
    S.data.settings.displays.stage || S.stage_connected ? pill(S.stage_connected, 'Stage', 'Stage display') : null,
    S.remotes_connected ? pill(true, `${S.remotes_connected} phone${S.remotes_connected > 1 ? 's' : ''}`, 'Phone remotes connected') : null,
  ].filter(Boolean));
}

// ---------- service list ----------
function subtitle(it) {
  const n = it.slides.length;
  if (it.kind === 'video' || it.kind === 'image' || it.kind === 'audio') {
    const m = S.data.media.find(m => m.id === it.item.media_id);
    if (m && m.status.state === 'converting') return `${KIND_NAME[it.kind]} · converting ${m.status.percent}%`;
    if (m && m.status.state === 'unconverted') return `${KIND_NAME[it.kind]} · may not play — needs converter`;
    if (m && m.status.state === 'failed') return `${KIND_NAME[it.kind]} · conversion failed`;
    return KIND_NAME[it.kind] + (it.item.end === 'loop' || it.item.looped ? ' · loops' : '');
  }
  return `${KIND_NAME[it.kind] || ''} · ${n} slide${n === 1 ? '' : 's'}`;
}

let dragId = null;
function renderService() {
  const list = $('#items');
  const liveId = L && L.cursor ? L.cursor.item_id : null;
  $('#itemCount').textContent = S.items.length ? `${S.items.length} item${S.items.length > 1 ? 's' : ''}` : '';
  if (!S.items.length) {
    list.replaceChildren(h('div', { class: 'empty' }, h('b', {}, 'Your service is empty'), 'Click “Add to service” below, type a Bible verse up top, or drag files onto this window.'));
    return;
  }
  list.replaceChildren(...S.items.map((it, i) => {
    const li = h('li', {
      class: 'item' + (it.id === selectedId ? ' selected' : '') + (it.id === liveId ? ' live' : ''), draggable: 'true', 'data-id': it.id,
      onclick: () => { selectedId = it.id; bibleArmed = null; renderService(); renderGrid(); },
      ondblclick: () => it.slides.length && show(it.id, 0),
      ondragstart: e => { dragId = it.id; li.classList.add('dragging'); e.dataTransfer.effectAllowed = 'move'; e.dataTransfer.setData('text/x-rp-item', it.id); },
      ondragend: () => { dragId = null; li.classList.remove('dragging'); },
      ondragover: e => { if (dragId && dragId !== it.id) { e.preventDefault(); li.classList.add('drag-over'); } },
      ondragleave: () => li.classList.remove('drag-over'),
      ondrop: e => { li.classList.remove('drag-over'); if (dragId && dragId !== it.id) { e.preventDefault(); e.stopPropagation(); const id = dragId; guard(api)('service/move', { id, to: i }); } },
    },
      h('span', { class: 'num' }, String(i + 1)),
      h('span', { class: 'kind' }, svg(I[it.kind] || I.text)),
      h('div', { class: 'info' }, h('div', { class: 't' }, it.title), h('div', { class: 's' }, subtitle(it))),
      h('div', { class: 'acts' },
        h('button', { title: 'Edit', onclick: e => { e.stopPropagation(); openItemEdit(it); } }, svg(I.edit)),
        h('button', { title: 'Remove from service', onclick: e => { e.stopPropagation(); guard(api)('service/remove', { id: it.id }); } }, svg(I.trash))),
    );
    return li;
  }));
}

// ---------- slide grid ----------
function selected() { return S.items.find(i => i.id === selectedId); }
function itemBackground(it) { return it.item.background || S.data.settings.background; }

function renderGrid() {
  const it = selected();
  const grid = $('#grid');
  $('#itemIco').replaceChildren(it ? svg(I[it.kind] || I.text) : '');
  $('#itemTitle').textContent = it ? it.title : (S.items.length ? 'Pick an item on the left' : 'Welcome');
  const acts = $('#itemActions');
  acts.replaceChildren(...(it ? [
    h('button', { class: 'btn small', onclick: () => openItemEdit(it) }, svg(I.edit), 'Edit'),
  ] : []));
  const key = JSON.stringify([it && it.id, it && it.slides, it && it.item.background, S.data.settings]);
  if (key === gridKey) { markLive(); return; }
  gridKey = key;
  if (!it) {
    grid.replaceChildren(S.items.length ? h('div', { class: 'empty' }, 'Choose something from the service list.') : welcome());
    return;
  }
  if (!it.slides.length) { grid.replaceChildren(h('div', { class: 'empty' }, h('b', {}, 'Nothing to show'), 'This item has no slides yet. Click Edit to add some.')); return; }
  const theme = S.data.settings.theme;
  const bg = itemBackground(it);
  grid.replaceChildren(...it.slides.map((s, idx) => {
    const screen = h('div', { class: 'screen' }, h('span', { class: 'badge' }, 'LIVE'));
    const btn = h('button', { class: 'thumb', 'data-idx': idx, onclick: () => { bibleArmed = null; show(it.id, idx); } },
      screen, h('div', { class: 'cap' }, h('span', { class: 'n' }, String(idx + 1)), s.label));
    const canvas = makeCanvas(screen);
    const layers = slideToLayers(s, it.kind, bg);
    drawStatic(canvas, { background: layers.background, media: layers.media, slide: layers.slide, logo: false, audio: layers.audio }, theme, S.data.settings.logo);
    if (s.content.type === 'video' || s.content.type === 'audio') screen.append(h('div', { class: 'play' }, h('span', {}, svg(I.play))));
    return btn;
  }));
  markLive();
}

function markLive() {
  const it = selected();
  const live = L && L.cursor && it && L.cursor.item_id === it.id && L.item ? L.cursor.slide : -1;
  const onAir = L && L.frame && (L.frame.slide || L.frame.media || L.frame.audio);
  document.querySelectorAll('#grid .thumb').forEach(t => t.classList.toggle('live', +t.dataset.idx === live && !!onAir));
  const el = document.querySelector('#grid .thumb.live');
  if (el && !el.dataset.seen) { el.scrollIntoView({ block: 'nearest', behavior: 'smooth' }); }
}

function welcome() {
  return h('div', { class: 'welcome' },
    h('h2', {}, 'Let’s build this Sunday’s service'),
    h('div', {}, 'Everything you add shows up on the left. Click a slide to put it on the screen.'),
    h('div', { class: 'steps' },
      h('div', {}, h('b', {}, '1'), 'Click “Screens” and choose the projector or TV.'),
      h('div', {}, h('b', {}, '2'), 'Click “Add to service” for songs, videos and slides — or drag files in.'),
      h('div', {}, h('b', {}, '3'), 'Type a verse like “John 3:16” in the Bible box and press Enter.'),
      h('div', {}, h('b', {}, '4'), 'Use the big Next button (or the → key) during the service.')),
  );
}

// ---------- live panel ----------
let liveCanvas = null, liveKey = '', mirror = null;
function renderLive() {
  if (!L || !S) return;
  if (!liveCanvas) liveCanvas = makeCanvas($('#liveMonitor'));
  const f = L.frame || {};
  const flat = {
    background: f.background ? { ...f.background } : null,
    media: f.media ? { ...f.media } : null,
    slide: f.slide || null, logo: f.logo, audio: f.audio,
  };
  const key = JSON.stringify([flat.background && flat.background.inst, flat.media && flat.media.inst, flat.slide, flat.logo, !!flat.audio, L.theme, L.logo]);
  if (key !== liveKey) {
    liveKey = key;
    mirror = null;
    if (flat.media && flat.media.type === 'video') {
      mirror = document.createElement('video');
      mirror.muted = true; mirror.preload = 'auto'; mirror.src = assetUrl(flat.media.asset);
    }
    drawStatic(liveCanvas, flat, L.theme, L.logo, { mirrorVideo: mirror });
  }
  // Keep the muted mirror roughly in sync with the room.
  if (mirror && L.media && f.media && L.media.inst === f.media.inst) {
    if (Math.abs(mirror.currentTime - L.media.position) > 1.2) mirror.currentTime = L.media.position;
    if (L.media.state === 'playing' && mirror.paused) mirror.play().catch(() => {});
    if (L.media.state !== 'playing' && !mirror.paused) mirror.pause();
  }
  $('#pending').classList.toggle('on', !!L.pending);
  const what = L.item ? `${L.item.title}${L.slide ? ' · ' + L.slide.label : ''}` : '';
  $('#liveWhat').textContent = f.logo ? 'Logo' : (f.slide || f.media) ? what : f.audio ? `♪ ${f.audio.title}` : 'Clear';
  renderTransport(f);
  const n = $('#notice');
  n.textContent = L.notice || '';
  n.classList.toggle('on', !!L.notice);
  document.querySelector('.clear.logo').classList.toggle('on', !!f.logo);
  const nu = $('#nextUp');
  if (L.next) {
    const s = L.next.slide;
    const text = s.content.type === 'text' ? s.content.text : ({ image: 'Slide picture', video: 'Video', still: 'Picture', audio: 'Audio' })[s.content.type];
    nu.replaceChildren(h('small', {}, `${L.next.title} · ${s.label}`), text);
  } else nu.textContent = L.item ? 'End of service' : '—';
  markLive();
  const liveId = L.cursor ? L.cursor.item_id : null;
  document.querySelectorAll('#items .item').forEach(li => li.classList.toggle('live', li.dataset.id === liveId));
}

function renderTransport(f) {
  const t = $('#transport');
  const rows = [];
  if (f.media && f.media.type === 'video') rows.push(transportRow('video', f.media.inst, f.media.paused, L.media, 'Video'));
  if (f.audio) rows.push(transportRow('audio', f.audio.inst, f.audio.paused, L.audio, `♪ ${f.audio.title}`));
  t.replaceChildren(...rows);
  t.classList.toggle('on', rows.length > 0);
  t.style.flexDirection = 'column';
  t.style.alignItems = 'stretch';
}
function transportRow(kind, inst, paused, st, label) {
  const pos = st && st.inst === inst ? st.position : 0, dur = st && st.inst === inst ? st.duration : 0;
  const pct = dur ? Math.min(100, pos / dur * 100) : 0;
  const bar = h('div', { class: 'bar' }, h('i', { style: `width:${pct}%` }));
  return h('div', { style: 'display:flex;align-items:center;gap:10px' },
    h('button', { title: paused ? 'Play' : 'Pause', onclick: () => cmd(kind === 'video' ? 'media_pause' : 'audio_pause', { inst, paused: !paused }) }, svg(paused ? I.play : I.pause)),
    h('div', { style: 'flex:1;min-width:0' }, h('div', { style: 'font-size:12px;color:var(--muted);margin-bottom:5px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis' }, label), bar),
    h('div', { class: 'time' }, dur ? `${fmtTime(pos)} / −${fmtTime(dur - pos)}` : '—'));
}

// ---------- jobs ----------
const clientJobs = new Map();
function renderJobs() {
  const box = $('#jobs');
  const rows = [];
  for (const j of clientJobs.values()) rows.push(h('div', { class: 'job' }, h('div', {}, j.label), h('div', { class: 'bar' }, h('i', { style: `width:${j.percent}%` }))));
  for (const j of (S ? S.jobs : [])) rows.push(h('div', { class: 'job' + (j.error ? ' err' : ''), 'data-job': j.id },
    h('div', { style: 'display:flex;gap:8px;align-items:start' }, h('div', { style: 'flex:1' }, j.label, j.error ? h('div', { style: 'color:#ffb0b3;font-size:12.5px;margin-top:4px' }, j.error) : null),
      j.error ? h('button', { class: 'dlg-close', onclick: () => guard(api)('job/dismiss', { id: j.id }) }, svg(I.close)) : null),
    j.error ? null : h('div', { class: 'bar' }, h('i', { style: `width:${j.percent}%` }))));
  box.replaceChildren(...rows);
}

// ---------- WebSocket ----------
function connect() {
  const es = new EventSource('/events?role=operator');
  es.onmessage = e => {
    const m = JSON.parse(e.data);
    if (m.type === 'live') { const first = !L; L = m; if (S) { renderLive(); if (first) renderService(); } }
    else if (m.type === 'data') refreshSoon();
    else if (m.type === 'job' && S) { const j = S.jobs.find(j => j.id === m.id); if (j) { j.percent = m.percent; renderJobs(); } }
    else if (m.type === 'convert' && S) { const md = S.data.media.find(x => x.id === m.id); if (md) { md.status = { state: 'converting', percent: m.percent }; renderService(); if (dialogRefresh) dialogRefresh(); } }
  };
  es.onopen = () => refreshSoon();
}

// ---------- keyboard ----------
const editable = el => el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
addEventListener('keydown', e => {
  const fk = { F1: 'clear_all', F2: 'clear_slide', F3: 'clear_media', F4: 'clear_audio', F5: 'logo' }[e.key];
  if (fk) { e.preventDefault(); cmd(fk); return; }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'b') { e.preventDefault(); $('#dialog').open || $('#bibleInput').focus(); $('#bibleInput').select(); return; }
  if ($('#dialog').open) return;
  if (e.key === 'Escape' && editable(document.activeElement)) { document.activeElement.blur(); return; }
  if (editable(document.activeElement)) return;
  if (['ArrowRight', 'ArrowDown', 'PageDown', ' '].includes(e.key)) { e.preventDefault(); bibleArmed = null; next(); }
  else if (['ArrowLeft', 'ArrowUp', 'PageUp'].includes(e.key)) { e.preventDefault(); bibleArmed = null; prev(); }
  else if (e.key === 'Enter') {
    e.preventDefault();
    const it = bibleArmed ? S.items.find(i => i.id === bibleArmed) : selected();
    bibleArmed = null;
    if (it && it.slides.length) show(it.id, 0);
  }
});

// ---------- Bible box ----------
$('#bibleForm').addEventListener('submit', guard(async e => {
  e.preventDefault();
  const input = $('#bibleInput');
  const reference = input.value.trim();
  if (!reference) return;
  const found = await api('bible/lookup', { reference, translation });
  const position = insertPosition();
  const { id } = await api('service/add', { position, item: { kind: 'scripture', reference: found.label, translation: found.translation, verses: found.verses, per_slide: 0 } });
  input.value = '';
  input.blur();
  selectedId = id; bibleArmed = id;
  await refresh();
  toast(`${found.label} added — press Enter to show it`);
}));
function insertPosition() {
  if (L && L.cursor) { const i = S.items.findIndex(x => x.id === L.cursor.item_id); if (i >= 0) return i + 1; }
  return S.items.length;
}

// ---------- buttons ----------
$('#nextBtn').onclick = () => next();
$('#prevBtn').onclick = () => prev();
document.querySelectorAll('.clear').forEach(b => b.onclick = () => cmd(b.dataset.cmd));
$('#addBtn').onclick = () => openAdd('song');
$('#settingsBtn').onclick = () => openSettings();
$('#screensBtn').onclick = () => openScreens();
$('#phoneBtn').onclick = () => openPhone();
$('#servicePick').onclick = () => openServices();

// ---------- dialogs ----------
let dialogRefresh = null;
function dialog(title, body, foot, { wide = false, onClose } = {}) {
  const d = $('#dialog');
  d.style.width = wide ? 'min(1180px, 96vw)' : '';
  d.replaceChildren(
    h('div', { class: 'dlg-head' }, h('h3', {}, title), h('button', { class: 'dlg-close', onclick: () => d.close() }, svg(I.close))),
    h('div', { class: 'dlg-body' }, body),
    foot ? h('div', { class: 'dlg-foot' }, foot) : '');
  d.onclose = () => { dialogRefresh = null; onClose && onClose(); };
  if (!d.open) d.showModal();
  return d;
}
const closeDialog = () => $('#dialog').close();

async function addItem(item, { select = true, quiet = false } = {}) {
  const { id } = await api('service/add', { item, position: null });
  if (select) selectedId = id;
  await refresh();
  if (!quiet) toast('Added to the service');
  return id;
}

// ----- Add to service -----
function openAdd(tab = 'song') {
  let current = tab;
  const body = h('div');
  const tiles = h('div', { class: 'tiles' });
  const content = h('div');
  const T = (id, icon, label, sub) => h('button', { class: 'tile' + (current === id ? ' on' : ''), onclick: () => { current = id; draw(); } }, svg(I[icon]), label, h('small', {}, sub));
  function draw() {
    tiles.replaceChildren(
      T('song', 'song', 'Song', 'Lyrics'), T('scripture', 'scripture', 'Bible', 'Verses'),
      T('media', 'video', 'Video & media', 'Videos, pictures, music'), T('deck', 'deck', 'Slides', 'PowerPoint, pictures'),
      T('text', 'text', 'Announcement', 'Your own text'));
    content.replaceChildren(({ song: addSong, scripture: addScripture, media: addMedia, deck: addDeck, text: addText })[current]());
    dialogRefresh = () => content.replaceChildren(({ song: addSong, scripture: addScripture, media: addMedia, deck: addDeck, text: addText })[current]());
  }
  body.append(tiles, content);
  draw();
  dialog('Add to service', body, null, { wide: false });
}

function searchList(items, render, placeholder) {
  const q = h('input', { class: 'search', placeholder, type: 'text' });
  const list = h('div', { class: 'list' });
  const draw = () => {
    const term = q.value.trim().toLowerCase();
    const rows = items.filter(x => !term || JSON.stringify(x).toLowerCase().includes(term)).map(render);
    list.replaceChildren(...(rows.length ? rows : [h('div', { class: 'empty' }, term ? 'Nothing matches.' : 'Nothing here yet.')]));
  };
  q.oninput = draw;
  draw();
  setTimeout(() => q.focus(), 30);
  return h('div', {}, q, list);
}

function addSong() {
  const songs = [...S.data.songs].sort((a, b) => a.title.localeCompare(b.title));
  const fileBtn = h('button', { class: 'btn', onclick: () => pickFiles('.txt', files => uploadFiles(files, { add: true })) }, svg(I.upload), 'Import SongSelect lyrics (.txt)');
  return h('div', {},
    h('div', { style: 'display:flex;gap:8px;margin-bottom:12px' },
      h('button', { class: 'btn primary', onclick: () => openSongEditor(null, { addAfter: true }) }, svg(I.plus), 'New song'), fileBtn),
    searchList(songs, s => h('div', { class: 'row' },
      h('div', { class: 'ico-box' }, svg(I.song)),
      h('div', { class: 'main' }, h('div', { class: 't' }, s.title), h('div', { class: 's' }, [s.writers, s.ccli && `CCLI #${s.ccli}`].filter(Boolean).join(' · ') || firstLyric(s))),
      h('button', { class: 'btn small', onclick: () => openSongEditor(s) }, svg(I.edit), 'Edit'),
      h('button', { class: 'btn small primary', onclick: guard(async () => { await addItem({ kind: 'song', song_id: s.id }); closeDialog(); }) }, svg(I.plus), 'Add')),
      `Search ${songs.length} song${songs.length === 1 ? '' : 's'}…`));
}
function firstLyric(s) { return (s.lyrics.split('\n').find(l => l.trim() && !sectionLabel(l)) || '').slice(0, 80); }

function addScripture() {
  const input = h('input', { type: 'text', placeholder: 'e.g. John 3:16-18, Psalm 23, Romans 8:28,31-39', style: 'font-size:16px' });
  const result = h('div', { style: 'margin-top:12px' });
  const go = guard(async () => {
    const found = await api('bible/lookup', { reference: input.value, translation });
    await addItem({ kind: 'scripture', reference: found.label, translation: found.translation, verses: found.verses, per_slide: 0 }, { quiet: true });
    toast(`${found.label} added`);
    closeDialog();
  });
  input.onkeydown = e => { if (e.key === 'Enter') { e.preventDefault(); go(); } };
  setTimeout(() => input.focus(), 30);
  return h('div', {},
    h('div', { class: 'field' }, h('label', {}, `Reference (${translation})`), input),
    h('div', { style: 'display:flex;gap:8px' }, h('button', { class: 'btn primary', onclick: go }, svg(I.plus), 'Add verses')),
    result,
    h('div', { class: 'note', style: 'margin-top:14px' }, h('b', {}, 'Tip: '), 'you don’t need this window. Type a verse in the Bible box at the top (or press Ctrl+B), then Enter adds it, and Enter again shows it.'));
}

function mediaThumb(m) {
  const box = h('div', { class: 'thumbmini' });
  if (m.kind === 'image') box.append(h('img', { src: assetUrl(m.asset), loading: 'lazy' }));
  else if (m.kind === 'video') { const v = h('video', { muted: true, preload: 'metadata' }); v.muted = true; v.src = assetUrl(m.asset) + '#t=1'; box.append(v); }
  else box.append(h('div', { style: 'display:grid;place-items:center;height:100%;color:var(--muted)' }, svg(I.audio)));
  return box;
}
function statusChip(m) {
  const s = m.status;
  if (s.state === 'ready') return h('span', { class: 'status-chip ok' }, 'Ready');
  if (s.state === 'converting') return h('span', { class: 'status-chip warn' }, `Converting ${s.percent}%`);
  if (s.state === 'unconverted') return h('span', { class: 'status-chip warn', title: s.reason }, 'Needs converter');
  return h('span', { class: 'status-chip bad', title: s.reason }, 'Failed');
}

function addMedia() {
  const media = [...S.data.media].reverse();
  return h('div', {},
    h('div', { style: 'display:flex;gap:8px;margin-bottom:12px;align-items:center' },
      h('button', { class: 'btn primary', onclick: () => pickFiles('video/*,audio/*,image/*,.mkv,.mov,.wmv,.avi,.mts,.m2ts,.mpg,.mpeg,.flv,.vob,.wma,.flac,.aiff', files => uploadFiles(files, { add: true })) }, svg(I.upload), 'Upload files'),
      h('span', { style: 'color:var(--muted);font-size:13px' }, 'Any video format works. Unusual ones are converted once, automatically.')),
    searchList(media, m => h('div', { class: 'row' }, mediaThumb(m),
      h('div', { class: 'main' }, h('div', { class: 't' }, m.name), h('div', { class: 's' }, KIND_NAME[m.kind] || m.kind)),
      statusChip(m),
      m.status.state === 'unconverted' || m.status.state === 'failed' ? h('button', { class: 'btn small', onclick: guard(async () => { await api('media/convert', { id: m.id }); toast('Converting…'); }) }, 'Convert') : null,
      h('button', { class: 'btn small', title: 'Delete from library', onclick: guard(async () => { if (confirm(`Delete “${m.name}” from the library?`)) { await api('media/delete', { id: m.id }); } }) }, svg(I.trash)),
      h('button', { class: 'btn small primary', onclick: guard(async () => { await addItem({ kind: 'media', media_id: m.id, autoplay: true, end: 'stop', looped: false }); closeDialog(); }) }, svg(I.plus), 'Add')),
      `Search ${media.length} file${media.length === 1 ? '' : 's'}…`));
}

function addDeck() {
  const decks = [...S.data.decks].reverse();
  return h('div', {},
    h('div', { style: 'display:flex;gap:8px;margin-bottom:12px;flex-wrap:wrap' },
      h('button', { class: 'btn primary', onclick: () => pickFiles('.pptx,.ppt,.ppsx,.pps,.odp', files => uploadFiles(files, { add: true })) }, svg(I.upload), 'Import PowerPoint'),
      h('button', { class: 'btn', onclick: () => pickFiles('image/*', files => imagesToDeck(files)) }, svg(I.image), 'Make slides from pictures')),
    h('div', { class: 'note' }, 'Google Slides: in Google Slides choose File → Download → Microsoft PowerPoint (.pptx), then import that file here.'),
    searchList(decks, d => h('div', { class: 'row' },
      h('div', { class: 'thumbmini' }, d.slides[0] ? h('img', { src: assetUrl(d.slides[0]), loading: 'lazy' }) : ''),
      h('div', { class: 'main' }, h('div', { class: 't' }, d.title), h('div', { class: 's' }, `${d.slides.length} slide${d.slides.length === 1 ? '' : 's'}`)),
      h('button', { class: 'btn small', onclick: guard(async () => { if (confirm(`Delete “${d.title}”?`)) await api('deck/delete', { id: d.id }); }) }, svg(I.trash)),
      h('button', { class: 'btn small primary', onclick: guard(async () => { await addItem({ kind: 'deck', deck_id: d.id }); closeDialog(); }) }, svg(I.plus), 'Add')),
      'Search slides…'));
}

function addText() {
  const title = h('input', { type: 'text', placeholder: 'e.g. Welcome, Announcements, Offering' });
  const body = h('textarea', { placeholder: 'Type your text. Leave an empty line to start a new slide.' });
  setTimeout(() => title.focus(), 30);
  return h('div', {},
    h('div', { class: 'field' }, h('label', {}, 'Title'), title),
    h('div', { class: 'field' }, h('label', {}, 'Text'), body),
    h('button', { class: 'btn primary', onclick: guard(async () => {
      if (!title.value.trim()) throw new Error('Give it a title.');
      await addItem({ kind: 'text', title: title.value.trim(), body: body.value }); closeDialog();
    }) }, svg(I.plus), 'Add announcement'));
}

// ----- files -----
function pickFiles(accept, cb) {
  const p = $('#filePick');
  p.accept = accept; p.value = '';
  p.onchange = () => { const files = [...p.files]; if (files.length) cb(files); };
  p.click();
}

function xhrUpload(file, purpose, onProgress) {
  return new Promise((res, rej) => {
    const x = new XMLHttpRequest();
    x.open('POST', `/api/upload?name=${encodeURIComponent(file.name)}&purpose=${purpose}`);
    x.setRequestHeader('x-rp', '1');
    x.upload.onprogress = e => e.lengthComputable && onProgress(Math.round(e.loaded / e.total * 100));
    x.onload = () => { let j = {}; try { j = JSON.parse(x.responseText); } catch {} x.status < 300 ? res(j) : rej(new Error(j.error || 'Upload failed.')); };
    x.onerror = () => rej(new Error('Upload failed.'));
    x.send(file);
  });
}

const PPT = ['ppt', 'pptx', 'pps', 'ppsx', 'odp'];
async function uploadFiles(files, { add = false } = {}) {
  for (const file of files) {
    const e = ext(file.name);
    const jobId = uuid();
    try {
      if (e === 'txt') {
        const r = await apiText('song/import-txt', await file.text());
        if (r.duplicate_of) toast(`“${r.title}” was already in the library — added a second copy.`);
        await refresh();
        if (add) await addItem({ kind: 'song', song_id: r.id }, { quiet: true });
        toast(`Imported “${r.title}”`);
      } else if (PPT.includes(e)) {
        const before = new Set(S.data.decks.map(d => d.id));
        clientJobs.set(jobId, { label: `Uploading ${file.name}`, percent: 0 }); renderJobs();
        const r = await xhrUpload(file, 'powerpoint', p => { clientJobs.get(jobId).percent = p; renderJobs(); });
        clientJobs.delete(jobId); renderJobs();
        if (add) watchDeckJob(r.job, before);
      } else {
        clientJobs.set(jobId, { label: `Uploading ${file.name}`, percent: 0 }); renderJobs();
        const r = await xhrUpload(file, 'media', p => { clientJobs.get(jobId).percent = p; renderJobs(); });
        clientJobs.delete(jobId); renderJobs();
        await refresh();
        if (add) await addItem({ kind: 'media', media_id: r.media.id, autoplay: true, end: 'stop', looped: false }, { quiet: true });
        toast(r.media.status.state === 'converting' ? `${file.name} added — converting so it plays smoothly…` : `${file.name} added`);
      }
    } catch (err) {
      clientJobs.delete(jobId); renderJobs();
      toast(`${file.name}: ${err.message}`, true);
    }
  }
  if (dialogRefresh) dialogRefresh();
}

function watchDeckJob(job, before) {
  const tick = guard(async () => {
    if (S.jobs.some(j => j.id === job && !j.error)) return setTimeout(tick, 700);
    if (S.jobs.some(j => j.id === job && j.error)) return;
    const fresh = S.data.decks.filter(d => !before.has(d.id));
    for (const d of fresh) await addItem({ kind: 'deck', deck_id: d.id }, { quiet: true });
    if (fresh.length) toast(`“${fresh[0].title}” is ready`);
  });
  setTimeout(tick, 700);
}

async function imagesToDeck(files) {
  const assets = [];
  const jobId = uuid();
  clientJobs.set(jobId, { label: `Uploading ${files.length} picture${files.length > 1 ? 's' : ''}`, percent: 0 }); renderJobs();
  try {
    files.sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }));
    for (const [i, f] of files.entries()) {
      const r = await xhrUpload(f, 'asset', () => {});
      assets.push(r.asset);
      clientJobs.get(jobId).percent = Math.round((i + 1) / files.length * 100); renderJobs();
    }
    const title = prompt('Name these slides', files[0].name.replace(/\.[^.]+$/, '')) || 'Slides';
    const { id } = await api('deck/from-images', { title, assets });
    await refresh();
    await addItem({ kind: 'deck', deck_id: id });
    closeDialog();
  } catch (e) { toast(e.message, true); }
  clientJobs.delete(jobId); renderJobs();
}

// drag & drop anywhere
let dragDepth = 0;
addEventListener('dragenter', e => { if (e.dataTransfer.types.includes('Files')) { dragDepth++; $('#dropHint').classList.add('on'); } });
addEventListener('dragleave', e => { if (e.dataTransfer.types.includes('Files') && --dragDepth <= 0) { dragDepth = 0; $('#dropHint').classList.remove('on'); } });
addEventListener('dragover', e => { if (e.dataTransfer.types.includes('Files')) e.preventDefault(); });
addEventListener('drop', e => {
  if (!e.dataTransfer.files.length) return;
  e.preventDefault(); dragDepth = 0; $('#dropHint').classList.remove('on');
  const files = [...e.dataTransfer.files];
  const images = files.filter(f => /^image\//.test(f.type));
  if (images.length > 1 && images.length === files.length && confirm(`Make ${images.length} pictures into one set of slides?\n\nOK = one slide set, Cancel = separate pictures`)) imagesToDeck(images);
  else uploadFiles(files, { add: true });
});

// ----- song editor (mirror of the server's section rules for the preview) -----
const SECTION_WORDS = ['verse', 'chorus', 'bridge', 'intro', 'outro', 'ending', 'end', 'tag', 'interlude', 'refrain', 'pre-chorus', 'prechorus', 'instrumental', 'vamp', 'turnaround', 'coda', 'misc', 'post-chorus'];
function sectionLabel(line) {
  const t = line.trim();
  if (!t || t.length > 40) return null;
  const br = t.startsWith('[') && t.endsWith(']');
  const inner = t.replace(/^\[|\]$/g, '').trim().replace(/:$/, '').trim();
  if (br && inner) return inner;
  const lower = inner.toLowerCase();
  const words = inner.split(/\s+/);
  const two = /^(pre|post) chorus/.test(lower);
  const known = SECTION_WORDS.includes(lower.split(/\s+/)[0]) || two;
  const rest = words.slice(two ? 2 : 1).every(w => w.length <= 3 && /^[a-z0-9]+$/i.test(w));
  return known && rest ? inner : null;
}
function parseSections(lyrics) {
  const out = []; let cur = null; let group = [];
  const close = () => { if (group.length) { (cur ||= { label: '', groups: [] }).groups.push(group); group = []; } };
  for (const raw of lyrics.replace(/\r\n/g, '\n').split('\n')) {
    const line = raw.trim();
    const lbl = sectionLabel(line);
    if (lbl) { close(); if (cur && cur.groups.length) out.push(cur); cur = { label: lbl, groups: [] }; }
    else if (!line) close();
    else group.push(line);
  }
  close(); if (cur && cur.groups.length) out.push(cur);
  out.forEach((s, i) => { if (!s.label) s.label = `Part ${i + 1}`; });
  return out;
}
function songPreviewSlides(song) {
  const secs = parseSections(song.lyrics);
  const n = S.data.settings.lines_per_slide || 2;
  let order = (song.arrangement || []).map(l => secs.findIndex(s => s.label.toLowerCase() === l.trim().toLowerCase())).filter(i => i >= 0);
  if (!order.length) order = secs.map((_, i) => i);
  const slides = [];
  for (const i of order) for (const g of secs[i].groups) {
    const pages = []; for (let k = 0; k < g.length; k += n) pages.push(g.slice(k, k + n).join('\n'));
    pages.forEach((t, p) => slides.push({ label: pages.length > 1 ? `${secs[i].label} (${p + 1}/${pages.length})` : secs[i].label, content: { type: 'text', text: t } }));
  }
  return { slides, sections: secs.map(s => s.label) };
}

function openSongEditor(song, { addAfter = false } = {}) {
  const s = song ? structuredClone(song) : { id: '00000000-0000-0000-0000-000000000000', title: '', writers: '', ccli: '', copyright: '', lyrics: '', arrangement: [] };
  const title = h('input', { type: 'text', value: s.title, placeholder: 'Amazing Grace (My Chains Are Gone)' });
  const writers = h('input', { type: 'text', value: s.writers, placeholder: 'Writers' });
  const ccli = h('input', { type: 'text', value: s.ccli, placeholder: '4768151' });
  const copyright = h('input', { type: 'text', value: s.copyright, placeholder: '© 2006 …' });
  const arrangement = h('input', { type: 'text', value: (s.arrangement || []).join(', '), placeholder: 'Leave empty to use the written order' });
  const lyrics = h('textarea', { spellcheck: 'false', placeholder: 'Verse 1\nAmazing grace how sweet the sound\nThat saved a wretch like me\n\nChorus\nMy chains are gone…' });
  lyrics.value = s.lyrics;
  const preview = h('div', { class: 'preview-list' });
  const chips = h('div', { style: 'display:flex;gap:6px;flex-wrap:wrap;margin-top:6px' });
  const draw = () => {
    s.title = title.value; s.lyrics = lyrics.value;
    s.arrangement = arrangement.value.split(',').map(x => x.trim()).filter(Boolean);
    const { slides, sections } = songPreviewSlides(s);
    chips.replaceChildren(...sections.filter((v, i, a) => a.indexOf(v) === i).map(l => h('button', { class: 'btn small', type: 'button', onclick: () => {
      arrangement.value = [...s.arrangement.length ? s.arrangement : [], l].join(', '); draw();
    } }, '+ ' + l)));
    preview.replaceChildren(...slides.map(sl => {
      const pv = h('div', { class: 'pv' });
      const wrap = h('div', {}, pv, h('div', { class: 'pl' }, sl.label));
      requestAnimationFrame(() => { const c = makeCanvas(pv); drawStatic(c, { background: S.data.settings.background, slide: { type: 'text', text: sl.content.text } }, S.data.settings.theme, null); });
      return wrap;
    }));
    if (!slides.length) preview.replaceChildren(h('div', { class: 'empty', style: 'grid-column:1/-1' }, 'Slides appear here as you type. Put section names like “Verse 1” or “Chorus” on their own line.'));
  };
  let t; const later = () => { clearTimeout(t); t = setTimeout(draw, 250); };
  [title, lyrics, arrangement].forEach(x => x.addEventListener('input', later));
  const body = h('div', { class: 'song-editor' },
    h('div', {},
      h('div', { class: 'cols' }, h('div', { class: 'field' }, h('label', {}, 'Title'), title), h('div', { class: 'field' }, h('label', {}, 'Writers'), writers)),
      h('div', { class: 'cols' }, h('div', { class: 'field' }, h('label', {}, 'CCLI song #'), ccli), h('div', { class: 'field' }, h('label', {}, 'Copyright'), copyright)),
      h('div', { class: 'field' }, h('label', {}, 'Lyrics ', h('span', { class: 'help' }, '— section names on their own line; an empty line inside a section starts a new slide')), lyrics)),
    h('div', {},
      h('div', { class: 'field' }, h('label', {}, 'Order ', h('span', { class: 'help' }, '(optional)')), arrangement, chips),
      h('div', { class: 'lbl', style: 'margin-bottom:8px' }, 'Slides'), preview));
  const save = guard(async () => {
    Object.assign(s, { title: title.value.trim(), writers: writers.value.trim(), ccli: ccli.value.trim(), copyright: copyright.value.trim(), lyrics: lyrics.value });
    s.arrangement = arrangement.value.split(',').map(x => x.trim()).filter(Boolean);
    const { id } = await api('song/save', s);
    await refresh();
    if (addAfter) await addItem({ kind: 'song', song_id: id });
    else toast('Song saved');
    closeDialog();
  });
  const foot = [
    song ? h('button', { class: 'btn danger', style: 'margin-right:auto', onclick: guard(async () => { if (confirm(`Delete “${song.title}” from the library?`)) { await api('song/delete', { id: song.id }); closeDialog(); } }) }, svg(I.trash), 'Delete song') : null,
    h('button', { class: 'btn', onclick: closeDialog }, 'Cancel'),
    h('button', { class: 'btn primary', onclick: save }, addAfter ? 'Save and add to service' : 'Save song')].filter(Boolean);
  dialog(song ? 'Edit song' : 'New song', body, foot, { wide: true });
  draw();
  setTimeout(() => (song ? lyrics : title).focus(), 30);
}

// ----- background picker -----
function backgroundPicker(value, onChange, { allowDefault = true } = {}) {
  const box = h('div', { class: 'bg-choices' });
  const colors = ['#000000', '#0b1a33', '#1d2b1f', '#2b1530', '#3a2412', '#ffffff'];
  const same = (a, b) => JSON.stringify(a || null) === JSON.stringify(b || null);
  const draw = () => {
    const items = [];
    if (allowDefault) items.push(h('button', { class: 'bg-choice' + (!value ? ' on' : ''), onclick: () => { value = null; onChange(null); draw(); } }, h('span', {}, 'Theme default')));
    for (const c of colors) items.push(h('button', { class: 'bg-choice' + (same(value, { type: 'color', color: c }) ? ' on' : ''), style: `background:${c}`, onclick: () => { value = { type: 'color', color: c }; onChange(value); draw(); } }));
    const custom = h('input', { type: 'color', class: 'swatch', value: value && value.type === 'color' ? value.color : '#123456', title: 'Pick any color', oninput: e => { value = { type: 'color', color: e.target.value }; onChange(value); } });
    items.push(custom);
    for (const m of S.data.media.filter(m => (m.kind === 'image' || m.kind === 'video') && m.status.state === 'ready')) {
      const v = { type: m.kind, asset: m.asset };
      const pv = m.kind === 'image' ? h('img', { src: assetUrl(m.asset), loading: 'lazy' }) : (() => { const x = h('video', { preload: 'metadata' }); x.muted = true; x.src = assetUrl(m.asset) + '#t=1'; return x; })();
      items.push(h('button', { class: 'bg-choice' + (same(value, v) ? ' on' : ''), title: m.name, onclick: () => { value = v; onChange(v); draw(); } }, pv, h('span', {}, (m.kind === 'video' ? '▶ ' : '') + m.name)));
    }
    items.push(h('button', { class: 'bg-choice', style: 'background:var(--panel-2);color:var(--muted)', onclick: () => pickFiles('image/*,video/*', guard(async files => {
      for (const f of files) await xhrUpload(f, 'media', () => {});
      await refresh(); draw(); toast('Added to the media library — pick it here');
    })) }, h('span', { style: 'position:static;background:none;font-size:12px' }, '+ Upload…')));
    box.replaceChildren(...items);
  };
  draw();
  return box;
}

// ----- item editor -----
function openItemEdit(it) {
  const item = structuredClone(it.item);
  const parts = [];
  if (it.kind === 'song') {
    const song = S.data.songs.find(s => s.id === item.song_id);
    parts.push(h('div', { class: 'note' }, 'Lyrics, order and credits belong to the song in your library.', ' ',
      song ? h('button', { class: 'btn small', onclick: () => openSongEditor(song) }, svg(I.edit), 'Edit song') : 'This song was deleted from the library.'));
  }
  if (it.kind === 'scripture') {
    const seg = h('div', { class: 'seg' });
    const draw = () => seg.replaceChildren(...[[0, 'Automatic'], [1, '1 verse'], [2, '2 verses'], [3, '3 verses'], [4, '4 verses']].map(([v, l]) =>
      h('button', { class: (item.per_slide || 0) === v ? 'on' : '', onclick: () => { item.per_slide = v; draw(); } }, l)));
    draw();
    parts.push(h('div', { class: 'field' }, h('label', {}, 'Verses per slide'), seg));
  }
  if (it.kind === 'text') {
    const title = h('input', { type: 'text', value: item.title, oninput: e => item.title = e.target.value });
    const body = h('textarea', { oninput: e => item.body = e.target.value }); body.value = item.body;
    parts.push(h('div', { class: 'field' }, h('label', {}, 'Title'), title), h('div', { class: 'field' }, h('label', {}, 'Text ', h('span', { class: 'help' }, '— an empty line starts a new slide')), body));
  }
  if (it.kind === 'video') {
    const end = h('div', { class: 'seg' });
    const draw = () => end.replaceChildren(...[['stop', 'Clear the screen'], ['hold', 'Stay on last frame'], ['loop', 'Play again (loop)']].map(([v, l]) =>
      h('button', { class: item.end === v ? 'on' : '', onclick: () => { item.end = v; draw(); } }, l)));
    draw();
    parts.push(h('label', { class: 'switch' }, h('input', { type: 'checkbox', checked: item.autoplay !== false, onchange: e => item.autoplay = e.target.checked }), 'Start playing as soon as it is shown'),
      h('div', { class: 'field' }, h('label', {}, 'When the video ends'), end));
  }
  if (it.kind === 'audio') {
    parts.push(h('label', { class: 'switch' }, h('input', { type: 'checkbox', checked: !!item.looped, onchange: e => item.looped = e.target.checked }), 'Repeat until stopped'),
      h('div', { class: 'note' }, 'Audio keeps playing while you change slides. Stop it with Clear Audio (F4) or Clear All.'));
  }
  if (['video', 'image', 'audio'].includes(it.kind)) {
    const m = S.data.media.find(x => x.id === item.media_id);
    if (m) {
      const name = h('input', { type: 'text', value: m.name });
      parts.push(h('div', { class: 'field' }, h('label', {}, 'Name in library'), name));
      parts.saveName = () => name.value.trim() && name.value !== m.name && api('media/rename', { id: m.id, name: name.value.trim() });
      if (m.status.state !== 'ready') parts.push(h('div', { class: 'note' }, statusChip(m), ' ', m.status.reason || ''));
    }
  }
  if (it.kind === 'deck') {
    const d = S.data.decks.find(x => x.id === item.deck_id);
    if (d) {
      const name = h('input', { type: 'text', value: d.title });
      parts.push(h('div', { class: 'field' }, h('label', {}, 'Name'), name));
      parts.saveName = () => name.value.trim() && name.value !== d.title && api('deck/rename', { id: d.id, title: name.value.trim() });
    }
  }
  if (['song', 'scripture', 'text'].includes(it.kind)) {
    parts.push(h('div', { class: 'field' }, h('label', {}, 'Background'), backgroundPicker(item.background, v => item.background = v)));
  }
  const notes = h('textarea', { placeholder: 'Shown on the stage display only', style: 'min-height:64px', oninput: e => item.notes = e.target.value });
  notes.value = item.notes || '';
  parts.push(h('div', { class: 'field' }, h('label', {}, 'Stage notes'), notes));
  const foot = [
    h('button', { class: 'btn danger', style: 'margin-right:auto', onclick: guard(async () => { await api('service/remove', { id: it.id }); closeDialog(); }) }, svg(I.trash), 'Remove from service'),
    h('button', { class: 'btn', onclick: closeDialog }, 'Cancel'),
    h('button', { class: 'btn primary', onclick: guard(async () => { if (parts.saveName) await parts.saveName(); await api('service/update', item); closeDialog(); toast('Saved'); }) }, 'Save')];
  dialog(it.title, h('div', {}, ...parts), foot);
}

// ----- services -----
function openServices() {
  const body = h('div', {},
    h('div', { style: 'display:flex;gap:8px;margin-bottom:12px' },
      h('button', { class: 'btn primary', onclick: guard(async () => { const name = prompt('Name for the new service', `Service ${new Date().toLocaleDateString()}`); if (name != null) { await api('service/new', { name }); closeDialog(); } }) }, svg(I.plus), 'New empty service'),
      h('button', { class: 'btn', onclick: guard(async () => { const name = prompt('Name for the copy', `Service ${new Date().toLocaleDateString()}`); if (name != null) { await api('service/new', { name, copy_from: S.service_id }); closeDialog(); } }) }, svg(I.copy), 'Copy this service')),
    h('div', { class: 'list' }, ...S.data.services.map(s => h('div', { class: 'row' },
      h('div', { class: 'main' }, h('div', { class: 't' }, s.name, s.id === S.service_id ? h('span', { class: 'status-chip ok', style: 'margin-left:8px' }, 'Open') : ''), h('div', { class: 's' }, `${s.items.length} item${s.items.length === 1 ? '' : 's'}`)),
      h('button', { class: 'btn small', onclick: guard(async () => { const name = prompt('Rename service', s.name); if (name) await api('service/rename', { id: s.id, name }); dialogRefresh = null; openServices(); }) }, 'Rename'),
      S.data.services.length > 1 ? h('button', { class: 'btn small', onclick: guard(async () => { if (confirm(`Delete the service “${s.name}”? (Songs and media stay in the library.)`)) { await api('service/delete', { id: s.id }); openServices(); } }) }, svg(I.trash)) : null,
      s.id !== S.service_id ? h('button', { class: 'btn small primary', onclick: guard(async () => { await api('service/select', { id: s.id }); closeDialog(); }) }, 'Open') : null))));
  dialog('Services', body);
}

// ----- screens -----
function openScreens() {
  const draw = () => {
    const card = (role, title, desc) => {
      const assigned = S.data.settings.displays[role];
      const open = S.windows.includes(role);
      const connected = role === 'room' ? S.room_connected : S.stage_connected;
      let pick = assigned && S.monitors.some(m => m.id === assigned) ? assigned : (S.monitors.find(m => !m.primary) || {}).id;
      const list = h('div', { class: 'mon-pick' }, ...S.monitors.map(m => h('button', { class: m.id === pick ? 'on' : '', onclick: e => { pick = m.id; list.querySelectorAll('button').forEach(b => b.classList.remove('on')); e.currentTarget.classList.add('on'); } }, m.label)));
      return h('div', { class: 'screen-card' },
        h('div', { style: 'display:flex;align-items:center;gap:8px;margin-bottom:4px' }, h('h4', { style: 'flex:1;margin:0' }, title),
          h('span', { class: 'status-chip ' + (connected ? 'ok' : open ? 'warn' : '') }, connected ? 'Showing' : open ? 'Starting…' : 'Off')),
        h('p', {}, desc),
        S.monitors.length > 1 ? list : h('div', { class: 'note' }, 'Only one screen is connected. Plug in the projector/TV (and set Windows to “Extend these displays” with Win+P), then click Refresh.'),
        h('div', { style: 'display:flex;gap:8px;flex-wrap:wrap' },
          h('button', { class: 'btn primary', disabled: !pick, onclick: guard(async () => { await api('display/open', { role, monitor: pick }); toast(`${title} opened`); setTimeout(() => { if (dialogRefresh) dialogRefresh(); }, 1500); }) }, open ? 'Move / reopen here' : 'Show on this screen'),
          open ? h('button', { class: 'btn', onclick: guard(async () => { await api('display/close', { role }); }) }, 'Turn off') : null,
          h('button', { class: 'btn', onclick: guard(async () => { await api('display/open', { role, monitor: null }); }) }, 'Test in a window')));
    };
    return h('div', {},
      h('div', { class: 'screens' },
        card('room', 'Room screen', 'What the congregation sees: projector or TV.'),
        card('stage', 'Stage display (optional)', 'For singers and the pastor: current and next lyrics, notes and a clock.')),
      h('div', { style: 'margin-top:12px;display:flex;justify-content:space-between;align-items:center;color:var(--muted);font-size:13px' },
        'RustyPresenter remembers your choice and reopens the screens next time.',
        h('button', { class: 'btn small', onclick: guard(async () => { await refresh(); dialogRefresh(); }) }, svg(I.restart), 'Refresh')));
  };
  const body = h('div', {}, draw());
  dialog('Screens', body);
  dialogRefresh = () => body.replaceChildren(draw());
}

// ----- phone -----
let pairTimer = null;
function openPhone() {
  const box = h('div');
  const draw = () => box.replaceChildren(
    h('div', { class: 'note' }, 'Control the slides from any phone on the church Wi-Fi. The phone must be on the same network as this computer.',
      S.lan_url ? h('div', { style: 'margin-top:6px' }, 'Address: ', h('span', { class: 'copy' }, S.lan_url)) : ' (No network found — connect this computer to Wi-Fi.)'),
    h('button', { class: 'btn primary', onclick: guard(startPair) }, svg(I.phone), 'Pair a phone'),
    h('div', { id: 'pairArea', style: 'margin-top:16px' }),
    h('div', { class: 'lbl', style: 'margin:18px 0 8px' }, 'Paired phones'),
    S.phones.length ? h('div', { class: 'list' }, ...S.phones.map(p => h('div', { class: 'row' }, h('div', { class: 'main' }, h('div', { class: 't' }, p.name)),
      h('button', { class: 'btn small danger', onclick: guard(async () => { await api('phone/revoke', { id: p.id }); await refresh(); draw(); }) }, 'Remove')))) : h('div', { style: 'color:var(--muted)' }, 'None yet.'));
  async function startPair() {
    const r = await api('pair/start');
    let left = r.seconds;
    const area = box.querySelector('#pairArea');
    const count = h('div', { style: 'color:var(--muted)' });
    area.replaceChildren(h('div', { class: 'pair' },
      h('div', { class: 'qr', html: r.qr || '' }),
      h('div', {},
        h('div', { style: 'font-size:15px' }, '1. On the phone, open the camera and scan the code (or type the address).'),
        h('div', { style: 'font-size:15px;margin-top:10px' }, '2. Enter this code on the phone:'),
        h('div', { class: 'pin' }, r.pin), count)));
    clearInterval(pairTimer);
    const tick = () => { count.textContent = left > 0 ? `Code works for ${left} more seconds` : 'Code expired — click Pair a phone again.'; left--; if (left < -1) clearInterval(pairTimer); };
    tick(); pairTimer = setInterval(tick, 1000);
  }
  draw();
  dialog('Phone remote', box, null, { onClose: () => { clearInterval(pairTimer); api('pair/stop').catch(() => {}); } });
  dialogRefresh = () => { const area = box.querySelector('#pairArea'); if (!area || !area.children.length) draw(); };
}

// ----- settings -----
function openSettings(tab = 'look') {
  const st = structuredClone(S.data.settings);
  let current = tab;
  const tabs = h('div', { class: 'tabs' });
  const content = h('div');
  const TABS = [['look', 'Look'], ['songs', 'Songs'], ['bible', 'Bible'], ['video', 'Video'], ['backups', 'Backups'], ['help', 'Help']];
  const seg = (options, get, set) => { const s = h('div', { class: 'seg' }); const d = () => s.replaceChildren(...options.map(([v, l]) => h('button', { class: get() === v ? 'on' : '', onclick: () => { set(v); d(); preview(); } }, l))); d(); return s; };
  let pvCanvas = null;
  const pvHost = h('div', { style: 'position:relative;aspect-ratio:16/9;border-radius:10px;overflow:hidden;background:#000;outline:1px solid var(--line)' });
  const preview = () => {
    if (!pvCanvas) pvCanvas = makeCanvas(pvHost);
    drawStatic(pvCanvas, { background: st.background, slide: { type: 'text', text: 'Amazing grace how sweet the sound\nThat saved a wretch like me', footer: 'Amazing Grace · John Newton · Public Domain' } }, st.theme, st.logo);
  };
  const fonts = ['Segoe UI', 'Segoe UI Semibold', 'Segoe UI Black', 'Bahnschrift', 'Arial', 'Arial Black', 'Calibri', 'Candara', 'Corbel', 'Franklin Gothic Medium', 'Georgia', 'Tahoma', 'Trebuchet MS', 'Verdana', 'Gill Sans MT', 'Century Gothic', 'Montserrat', 'Open Sans', 'Lato', 'Roboto'];
  const pages = {
    look: () => {
      const font = h('select', { onchange: e => { st.theme.font = e.target.value; preview(); } }, ...fonts.map(f => h('option', { value: f, selected: f === st.theme.font, style: `font-family:'${f}'` }, f)));
      const size = h('input', { type: 'range', min: 40, max: 150, value: st.theme.size, oninput: e => { st.theme.size = +e.target.value; preview(); } });
      const ssize = h('input', { type: 'range', min: 36, max: 120, value: st.theme.scripture_size, oninput: e => { st.theme.scripture_size = +e.target.value; preview(); } });
      const color = h('input', { type: 'color', class: 'swatch', value: st.theme.color, oninput: e => { st.theme.color = e.target.value; preview(); } });
      const logoBox = h('div', { style: 'display:flex;gap:8px;align-items:center' },
        st.logo ? h('img', { src: assetUrl(st.logo), style: 'height:44px;background:#000;border-radius:6px;padding:4px' }) : h('span', { style: 'color:var(--muted)' }, 'No logo (Logo button shows black)'),
        h('button', { class: 'btn small', onclick: () => pickFiles('image/*', guard(async files => { const r = await xhrUpload(files[0], 'asset', () => {}); st.logo = r.asset; content.replaceChildren(pages.look()); preview(); })) }, svg(I.upload), 'Choose logo'),
        st.logo ? h('button', { class: 'btn small', onclick: () => { st.logo = null; content.replaceChildren(pages.look()); preview(); } }, 'Remove') : null);
      setTimeout(preview, 0);
      return h('div', { class: 'cols', style: 'grid-template-columns:1fr 1.1fr;gap:20px' },
        h('div', {},
          h('div', { class: 'field' }, h('label', {}, 'Font'), font),
          h('div', { class: 'field' }, h('label', {}, 'Lyrics size ', h('span', { class: 'help' }, '(biggest; long lines shrink to fit)')), size),
          h('div', { class: 'field' }, h('label', {}, 'Scripture size'), ssize),
          h('div', { class: 'cols' },
            h('div', { class: 'field' }, h('label', {}, 'Text color'), color),
            h('div', { class: 'field' }, h('label', {}, 'Text position'), seg([['center', 'Middle'], ['lower', 'Lower third']], () => st.theme.position, v => st.theme.position = v))),
          h('label', { class: 'switch' }, h('input', { type: 'checkbox', checked: st.theme.shadow, onchange: e => { st.theme.shadow = e.target.checked; preview(); } }), 'Shadow behind text (easier to read on pictures)'),
          h('label', { class: 'switch' }, h('input', { type: 'checkbox', checked: st.theme.uppercase, onchange: e => { st.theme.uppercase = e.target.checked; preview(); } }), 'ALL CAPS lyrics'),
          h('div', { class: 'field' }, h('label', {}, 'Logo'), logoBox)),
        h('div', {},
          h('div', { class: 'lbl', style: 'margin-bottom:8px' }, 'Preview'), pvHost,
          h('div', { class: 'field', style: 'margin-top:14px' }, h('label', {}, 'Default background'), backgroundPicker(st.background, v => { st.background = v; preview(); }, { allowDefault: false }))));
    },
    songs: () => h('div', {},
      h('div', { class: 'field' }, h('label', {}, 'Lines per slide'), seg([[1, '1'], [2, '2'], [3, '3'], [4, '4'], [5, '5'], [6, '6']], () => st.lines_per_slide, v => st.lines_per_slide = v)),
      h('div', { class: 'field' }, h('label', {}, 'Song credits'), seg([['first', 'On first slide'], ['last', 'On last slide'], ['none', 'Off']], () => st.credits, v => st.credits = v)),
      h('div', { class: 'field' }, h('label', {}, 'Church CCLI license #'), h('input', { type: 'text', value: st.church_ccli, oninput: e => st.church_ccli = e.target.value }))),
    bible: () => {
      const CHIP = { offline: ['ok', 'Ready — offline'], online: ['ok', 'Ready — online'], needs_key: ['warn', 'Needs a free key'], unavailable: ['warn', 'No source yet'] };
      const redraw = () => content.replaceChildren(pages.bible());
      const keyRow = b => {
        const input = h('input', { type: 'password', placeholder: b.status === 'online' ? 'Key saved — paste a new one to replace it' : 'Paste the key here', style: 'flex:1;min-width:0' });
        const save = guard(async () => { await api('bible/key', { translation: b.translation, key: input.value }); await refresh(); redraw(); toast(input.value.trim() ? `${b.translation} is ready` : `${b.translation} key removed`); });
        return h('div', { style: 'padding:4px 12px 12px' },
          h('div', { class: 's', style: 'margin-bottom:6px;color:var(--muted)' }, '1. Make a free account and an “application” at ',
            h('span', { class: 'copy' }, b.signup_url), ' ',
            h('button', { class: 'btn small', onclick: () => { navigator.clipboard.writeText(b.signup_url); toast('Link copied — paste it into your browser'); } }, svg(I.copy), 'Copy link'),
            h('br'), '2. Paste the key it gives you here. RustyPresenter checks it with one lookup, then keeps it on this computer only.'),
          h('div', { style: 'display:flex;gap:8px' }, input,
            h('button', { class: 'btn small primary', onclick: save }, 'Save key'),
            b.status === 'online' ? h('button', { class: 'btn small', onclick: () => { input.value = ''; save(); } }, 'Remove') : null));
      };
      const credits = S.bible_sources.filter(b => b.status === 'online' && b.copyright);
      return h('div', {},
        h('div', { class: 'note' }, h('b', {}, 'KJV, WEB and BSB are built in and work offline. '), 'The other translations are copyrighted. Each one comes from its publisher’s free service with a key your church makes once. Passages you add to a service are saved with the service, so they still show if the internet drops.'),
        h('div', { class: 'list' }, ...S.bible_sources.map(b => h('div', {},
          h('div', { class: 'row' }, h('div', { class: 'main' }, h('div', { class: 't' }, b.translation), h('div', { class: 's' }, b.provider || 'No licensed source available yet')),
            h('span', { class: 'status-chip ' + CHIP[b.status][0] }, CHIP[b.status][1])),
          b.signup_url ? keyRow(b) : null))),
        credits.length ? h('div', { class: 'note', style: 'margin-top:12px;font-size:12.5px' }, ...credits.map(b => h('div', { style: 'margin-bottom:6px' }, b.copyright))) : null);
    },
    video: () => h('div', {},
      h('div', { class: 'note' }, S.ffmpeg
        ? [h('b', {}, '✓ Video converter installed. '), 'Any video you add (MOV, MKV, AVI, WMV, HEVC, ProRes, MPEG…) is converted once so it plays smoothly.']
        : [h('b', {}, 'Install the free video converter (FFmpeg) so every video format works. '), 'MP4 and WebM videos already play without it.']),
      S.ffmpeg ? null : h('div', {},
        h('div', { style: 'margin-bottom:8px' }, '1. Click Start, type ', h('b', {}, 'PowerShell'), ', open it, and paste this line:'),
        h('div', { style: 'display:flex;gap:8px;align-items:center;margin-bottom:10px' }, h('span', { class: 'copy', style: 'padding:8px 10px;font-size:14px' }, 'winget install --id Gyan.FFmpeg -e'),
          h('button', { class: 'btn small', onclick: () => { navigator.clipboard.writeText('winget install --id Gyan.FFmpeg -e'); toast('Copied'); } }, svg(I.copy), 'Copy')),
        h('div', { style: 'margin-bottom:8px' }, '2. When it finishes, close RustyPresenter and open it again.'),
        h('button', { class: 'btn', onclick: guard(async () => { await refresh(); content.replaceChildren(pages.video()); toast(S.ffmpeg ? 'Converter found!' : 'Not found yet — restart RustyPresenter after installing.'); }) }, svg(I.restart), 'Check again'))),
    backups: () => h('div', {},
      h('div', { class: 'note' }, 'A full backup (songs, services, settings and every video, picture and slide) is made each time RustyPresenter starts and once a day. The last 10 are kept in ', h('span', { class: 'copy' }, S.data_folder + '\\backups'), '.'),
      h('button', { class: 'btn primary', style: 'margin-bottom:12px', onclick: guard(async () => { await api('backup/now'); await refresh(); content.replaceChildren(pages.backups()); toast('Backup made'); }) }, 'Back up now'),
      h('div', { class: 'list' }, ...(S.backups.length ? S.backups.map(b => h('div', { class: 'row' },
        h('div', { class: 'main' }, h('div', { class: 't' }, b.created.replace('_', ' at ').replace(/(\d\d)(\d\d)(\d\d)$/, '$1:$2') + ' UTC'), h('div', { class: 's' }, `${b.songs} songs · ${b.services} services · ${b.assets} files`)),
        h('button', { class: 'btn small', onclick: guard(async () => { if (confirm('Restore this backup? Your current songs and services are replaced (a copy of them is kept).')) { await api('backup/restore', { name: b.name }); closeDialog(); toast('Backup restored'); } }) }, 'Restore'))) : [h('div', { style: 'color:var(--muted)' }, 'No backups yet.')]))),
    help: () => h('div', {},
      h('div', { class: 'cols' },
        h('div', {}, h('div', { class: 'lbl', style: 'margin-bottom:8px' }, 'Keyboard'),
          ...[['→  Space  Page Down', 'Next'], ['←  Page Up', 'Back'], ['Enter', 'Show the selected item'], ['Ctrl + B', 'Bible box'], ['F1', 'Clear all'], ['F2', 'Clear text'], ['F3', 'Clear media & background'], ['F4', 'Stop audio'], ['F5', 'Logo'], ['Esc', 'Leave a text box']]
            .map(([k, v]) => h('div', { style: 'display:flex;justify-content:space-between;padding:5px 0;border-bottom:1px solid var(--line)' }, h('kbd', {}, k), v))),
        h('div', {}, h('div', { class: 'lbl', style: 'margin-bottom:8px' }, 'About'),
          h('div', { class: 'note' }, `RustyPresenter ${S.app_version}`, h('br'), 'Your data: ', h('span', { class: 'copy' }, S.data_folder)),
          h('div', { class: 'note' }, 'Tip: clicking any slide puts it on the screen right away. The red outline shows what the room sees.')))),
  };
  const draw = () => {
    tabs.replaceChildren(...TABS.map(([id, l]) => h('button', { class: current === id ? 'on' : '', onclick: () => { current = id; pvCanvas = null; pvHost.replaceChildren(); draw(); } }, l)));
    content.replaceChildren(pages[current]());
  };
  draw();
  dialog('Settings', h('div', {}, tabs, content), [
    h('button', { class: 'btn', onclick: closeDialog }, 'Cancel'),
    h('button', { class: 'btn primary', onclick: guard(async () => { await api('settings', st); closeDialog(); toast('Settings saved'); }) }, 'Save settings')], { wide: true });
}

// ---------- start ----------
guard(refresh)().then(connect);
