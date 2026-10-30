// Room output. Preloads each new frame off-screen, swaps with a short fade,
// then tells the core it is on air (Committed). Media reports carry the
// playback instance id so the core can tell live media from retired media.
import { makeCanvas, backgroundEl, imgEl, textEl, fitText, logoEl, assetUrl } from '/static/render.js';

const host = document.getElementById('host');
const aspect = innerHeight / innerWidth || 9 / 16;
const canvas = makeCanvas(host, aspect);
const Z = { bg: 1, media: 2, slide: 3, logo: 4 };
const cur = { bg: null, media: null, slide: null, logo: null, audio: null };
let shownVersion = -1, shownTheme = '', token = 0;
let theme = {}, logo = null, lastMsg = null;

addEventListener('resize', () => {
  clearTimeout(window.__rz);
  window.__rz = setTimeout(() => { if (Math.abs(innerHeight / innerWidth - aspect) > 0.02) location.reload(); }, 600);
});

function send(ev) {
  fetch('/api/room-event', { method: 'POST', headers: { 'content-type': 'application/json', 'x-rp': '1' }, body: JSON.stringify(ev) }).catch(() => {});
}

function connect() {
  const es = new EventSource('/events?role=room');
  es.onmessage = e => {
    const m = JSON.parse(e.data);
    if (m.type === 'live') onLive(m);
  };
}

function onLive(m) {
  lastMsg = m;
  theme = m.theme || {};
  logo = m.logo || null;
  const tk = JSON.stringify([theme, logo]);
  if (m.version === shownVersion && tk === shownTheme) { syncPause(m.frame); return; }
  apply(m.version, m.frame, tk);
}

function keys(f, tk) {
  return {
    bg: f.background ? `bg:${f.background.inst}` : null,
    media: f.media ? `m:${f.media.inst}` : null,
    slide: f.slide ? `s:${JSON.stringify(f.slide)}:${tk}` : null,
    logo: f.logo ? `logo:${logo}` : null,
    audio: f.audio ? `a:${f.audio.inst}` : null,
  };
}

const timeout = (ms, what) => new Promise((_, rej) => setTimeout(() => rej(new Error(`${what} took too long to load`)), ms));

function waitMedia(el, what) {
  return new Promise((res, rej) => {
    if (el.readyState >= 3) return res();
    el.addEventListener('canplay', () => res(), { once: true });
    el.addEventListener('error', () => rej(new Error(`${what} could not be played (${mediaError(el)})`)), { once: true });
  });
}
function waitImg(img) { return img.decode ? img.decode().catch(() => { throw new Error('picture could not be loaded'); }) : Promise.resolve(); }
function mediaError(el) {
  const c = el.error && el.error.code;
  return ({ 1: 'stopped', 2: 'network/file error', 3: 'the file is damaged or uses an unsupported codec', 4: 'this format is not supported — convert it in the Media library' })[c] || 'unknown error';
}

/** Build a layer entry (hidden) and a promise that resolves when it is ready to show. */
function prepare(kind, f) {
  let el, media = null, ready, ms = 2500;
  if (kind === 'bg') {
    el = backgroundEl(f.background, { live: false });
    const v = el.querySelector('video'), i = el.querySelector('img');
    if (v) { v.src = assetUrl(f.background.asset); v.autoplay = false; media = v; ready = waitMedia(v, 'background video'); ms = 6000; }
    else if (i) ready = waitImg(i);
  } else if (kind === 'media') {
    el = document.createElement('div');
    el.style.cssText = 'position:absolute;inset:0;background:#000;';
    if (f.media.type === 'still') { const i = imgEl(f.media.asset, 'contain'); el.append(i); ready = waitImg(i); }
    else {
      const v = document.createElement('video');
      v.preload = 'auto'; v.playsInline = true;
      v.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;object-fit:contain;';
      v.loop = f.media.end === 'loop';
      v.src = assetUrl(f.media.asset);
      el.append(v); media = v; ready = waitMedia(v, 'video'); ms = 8000;
    }
  } else if (kind === 'slide') {
    el = document.createElement('div');
    el.style.cssText = 'position:absolute;inset:0;';
    if (f.slide.type === 'image') { const i = imgEl(f.slide.asset, 'contain'); i.style.background = '#000'; el.append(i); ready = waitImg(i); }
    else { el.append(textEl(f.slide, theme, canvas.H)); ready = document.fonts.ready; }
  } else if (kind === 'logo') {
    el = logoEl(logo);
    const i = el.querySelector('img'); ready = i ? waitImg(i) : Promise.resolve();
  } else if (kind === 'audio') {
    media = new Audio(); media.preload = 'auto'; media.loop = !!f.audio.looped; media.src = assetUrl(f.audio.asset);
    ready = waitMedia(media, 'audio'); ms = 8000;
  }
  if (el) { el.className = 'layer entering'; el.style.zIndex = Z[kind]; canvas.append(el); }
  const inst = kind === 'bg' ? f.background.inst : kind === 'media' ? f.media.inst : kind === 'audio' ? f.audio.inst : null;
  return { entry: { el, media, inst, kind, abort: new AbortController() }, ready: Promise.race([Promise.resolve(ready), timeout(ms, kind === 'audio' ? 'audio' : kind === 'media' ? 'video' : 'slide')]) };
}

function dispose(entry, fade = false) {
  if (!entry) return;
  entry.abort.abort();
  if (entry.media) { try { entry.media.pause(); entry.media.removeAttribute('src'); entry.media.load(); } catch {} }
  if (entry.el) {
    if (fade) { entry.el.classList.add('entering'); setTimeout(() => entry.el.remove(), 400); } else entry.el.remove();
  }
}

async function apply(version, f, tk) {
  const my = ++token;
  const want = keys(f, tk);
  const prepared = {};
  const jobs = [];
  for (const k of Object.keys(want)) {
    const have = cur[k] ? cur[k].key : null;
    if (want[k] && want[k] !== have) {
      const p = prepare(k, f);
      p.entry.key = want[k];
      prepared[k] = p.entry;
      jobs.push(p.ready);
    }
  }
  try {
    await Promise.all(jobs);
    if (prepared.slide) fitText(prepared.slide.el);
  } catch (err) {
    Object.values(prepared).forEach(e => dispose(e));
    if (my === token) send({ ev: 'failed', version, reason: err.message || String(err) });
    return;
  }
  if (my !== token) { Object.values(prepared).forEach(e => dispose(e)); return; }

  // Swap.
  for (const k of Object.keys(want)) {
    const have = cur[k] ? cur[k].key : null;
    if (want[k] === have) continue;
    const old = cur[k];
    cur[k] = prepared[k] || null;
    if (cur[k] && cur[k].el) { void cur[k].el.offsetWidth; cur[k].el.classList.remove('entering'); }
    dispose(old, true);
    if (cur[k] && cur[k].media) startMedia(cur[k], k === 'bg');
  }
  shownVersion = version;
  shownTheme = tk;
  syncPause(f);
  send({ ev: 'committed', version });
}

function startMedia(entry, isBackground) {
  const m = entry.media;
  if (isBackground) { m.muted = true; m.loop = true; m.play().catch(() => {}); return; }
  const sig = { signal: entry.abort.signal };
  let last = 0;
  const report = (state, extra = {}) => send({ ev: 'media', inst: entry.inst, state, position: m.currentTime || 0, duration: isFinite(m.duration) ? m.duration : 0, ...extra });
  m.addEventListener('playing', () => report('playing'), sig);
  m.addEventListener('pause', () => { if (!m.ended) report('paused'); }, sig);
  m.addEventListener('ended', () => report('ended'), sig);
  m.addEventListener('error', () => report('error', { error: mediaError(m) }), sig);
  m.addEventListener('timeupdate', () => { const n = performance.now(); if (n - last > 500) { last = n; report(m.paused ? 'paused' : 'playing'); } }, sig);
  entry.started = true;
}

function syncPause(f) {
  const pairs = [[cur.media, f.media], [cur.audio, f.audio]];
  for (const [entry, layer] of pairs) {
    if (!entry || !entry.media || !layer || entry.inst !== layer.inst) continue;
    const m = entry.media;
    const wantPaused = !!layer.paused;
    if (wantPaused && !m.paused) m.pause();
    if (!wantPaused && m.paused && !m.ended) {
      m.play().catch(err => {
        if (err && err.name === 'NotAllowedError') showUnmute();
        else send({ ev: 'media', inst: entry.inst, state: 'error', position: 0, duration: 0, error: err.message });
      });
    }
  }
}

function showUnmute() {
  const u = document.getElementById('unmute');
  u.style.display = 'flex';
  u.onclick = () => { u.style.display = 'none'; if (lastMsg) syncPause(lastMsg.frame); };
}

connect();
