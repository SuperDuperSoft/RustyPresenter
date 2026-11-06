const $ = id => document.getElementById(id);
const rid = () => Math.random().toString(36).slice(2) + Date.now().toString(36) + Math.random().toString(36).slice(2);
let es = null, V = null;

function toast(msg, good = false) { const t = $('toast'); t.textContent = msg; t.classList.toggle('good', good); t.style.display = 'block'; clearTimeout(t._h); t._h = setTimeout(() => t.style.display = 'none', 2500); }
async function post(env) {
  try {
    const r = await fetch('/remote/command', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(env) });
    if (r.status === 401) return start();
    const out = await r.json();
    if (!out.ok && out.error) {
      if (!out.error.startsWith('stale')) toast(out.error);
      else if (env.cmd === 'show') toast('The screen changed just now — tap the slide again.');
    }
  } catch { toast('Not connected'); }
}
function send(cmd, extra = {}, withBase = false) {
  const env = { id: rid(), cmd, ...extra };
  if (withBase && V) env.base = V.version;
  if (navigator.vibrate) navigator.vibrate(12);
  post(env);
}

const phoneName = () => /iPhone/.test(navigator.userAgent) ? 'iPhone' : /Android/.test(navigator.userAgent) ? 'Android phone' : 'Phone';

// The QR code opens /remote#pair=<one-time secret>. Take it out of the address bar straight away (so it is not
// bookmarked, shared or reused), then pair with it: no typing.
async function pairFromLink() {
  const m = /^#pair=([A-Za-z0-9_-]+)$/.exec(location.hash);
  if (!location.hash) return false;
  history.replaceState(null, '', location.pathname + location.search);
  if (!m) return false;
  $('pairErr').textContent = '';
  const st = await fetch('/remote/status').then(r => r.json()).catch(() => ({ paired: false }));
  if (st.paired) return false;
  const pair = () => fetch('/remote/pair', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ pair: m[1], name: phoneName() }) }).catch(() => null);
  let r = await pair();
  // Scanning twice within a second hits the one-try-a-second limit (429): the code is still good, so wait and retry.
  if (r && r.status === 429) { await new Promise(ok => setTimeout(ok, 1100)); r = await pair(); }
  if (r && r.ok) { toast('Connected', true); return false; }
  const j = r ? await r.json().catch(() => ({})) : {};
  await start();
  // "Expired" only when the server said the code is wrong or pairing is closed.
  $('pairErr').textContent = !r ? 'Could not connect.' : r.status === 429 ? j.error || 'Could not connect.'
    : 'That QR code has expired. On the computer, click Pair a phone again.';
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
  if (!r.ok) { $('pairErr').textContent = j.error || 'Could not connect.'; return; }
  start();
};
$('pin').addEventListener('keydown', e => { if (e.key === 'Enter') $('pairBtn').click(); });

function connect() {
  if (es) es.close();
  es = new EventSource('/remote/events');
  es.onopen = () => $('dot').classList.add('on');
  es.onmessage = e => { const m = JSON.parse(e.data); if (m.type === 'remote') { V = m; render(); } };
  es.onerror = async () => {
    $('dot').classList.remove('on');
    const st = await fetch('/remote/status').then(r => r.json()).catch(() => null);
    if (st && !st.paired) { es.close(); es = null; start(); }
  };
}

function render() {
  $('svc').textContent = V.service || 'Service';
  const air = V.on_air;
  $('now').textContent = air.logo ? '(Logo)' : V.current && (air.slide || air.media) ? V.current.text : air.audio ? '(Music playing)' : '(Clear)';
  $('nowLabel').textContent = V.current && (air.slide || air.media) ? V.current.label : '';
  $('nextText').textContent = V.next ? V.next.text : 'End of service';
  $('nextLabel').textContent = V.next ? `${V.next.title} · ${V.next.label}` : '';
  $('logoBtn').classList.toggle('on', air.logo);
  $('media').classList.toggle('on', !!V.media);
  if (V.media) $('mediaBtn').textContent = V.media.paused ? '▶ Play video' : '❚❚ Pause video';
  const liveId = V.cursor && V.cursor.item_id;
  $('items').replaceChildren(...V.items.map((it, i) => {
    const row = document.createElement('div');
    row.className = 'it' + (it.id === liveId ? ' live' : '');
    row.innerHTML = `<span class="n">${i + 1}</span><span class="t"></span><span class="k"></span>`;
    row.querySelector('.t').textContent = it.title;
    row.querySelector('.k').textContent = it.slides.length + (it.slides.length === 1 ? ' slide' : ' slides');
    row.onclick = () => openSheet(it);
    return row;
  }));
}

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
$('mediaBtn').onclick = () => V && V.media && send('media_pause', { inst: V.media.inst, paused: !V.media.paused });

const fromLink = () => pairFromLink().catch(() => false).then(shown => { if (!shown) start(); });
fromLink();
// A rescan can land in this same tab (Android reuses it): then only the fragment changes and the page doesn't reload.
window.addEventListener('hashchange', fromLink);
