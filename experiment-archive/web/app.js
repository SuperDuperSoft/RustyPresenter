'use strict';
const mode = document.body.dataset.mode;
const $ = id => document.getElementById(id);
let token = sessionStorage.getItem(`rustypresenter-${mode}`) || '';
if (location.hash.length > 1 && mode !== 'remote') {
  token = location.hash.slice(1);
  sessionStorage.setItem(`rustypresenter-${mode}`, token);
  history.replaceState(null, '', location.pathname);
}
let state = null, connected = false, draft = null, draftRevision = 0, dirty = false;
let selectedItem = null, selectedSlide = null, busy = false, toastTimer, lastBibleReference = '';
let renderedServiceRevision = -1;

function toast(message, error = false) {
  const el = $('toast'); if (!el) return;
  el.textContent = message; el.classList.toggle('error', error); el.hidden = false;
  clearTimeout(toastTimer); toastTimer = setTimeout(() => { el.hidden = true; }, error ? 9000 : 4000);
}
function connection(ok) {
  connected = ok;
  const el = $('connection');
  if (el) { el.textContent = ok ? 'Connected' : (token ? 'Reconnecting…' : 'Not paired'); el.className = `connection ${ok ? 'connected' : 'offline'}`; }
  document.querySelectorAll('#next,#previous,#blackout,#clear,#take').forEach(button => { button.disabled = !ok || busy; });
  document.querySelectorAll('.remote-cue').forEach(button => { button.disabled = !ok || busy; });
}
async function api(path, options = {}) {
  const headers = { ...(token ? { Authorization: `Bearer ${token}` } : {}), ...(options.body ? { 'Content-Type': 'application/json' } : {}) };
  const response = await fetch(path, { ...options, headers, cache: 'no-store', signal: AbortSignal.timeout(5000) });
  if (!response.ok) {
    if (response.status === 401) {
      token = ''; sessionStorage.removeItem(`rustypresenter-${mode}`); connection(false);
      if (mode === 'remote') { $('pair-form').hidden = false; $('remote-controls').hidden = true; }
      if (mode === 'operator') { $('auth-message').hidden = false; $('operator-app').hidden = true; }
    }
    throw new Error(await response.text() || `Request failed (${response.status})`);
  }
  return response.status === 204 ? null : response.json();
}
function element(tag, className, text) {
  const el = document.createElement(tag); if (className) el.className = className; if (text !== undefined) el.textContent = text; return el;
}
const resizeObserver = new ResizeObserver(entries => entries.forEach(entry => fitSlide(entry.target)));
function fitSlide(container) {
  const text = container.querySelector('.slide-text'); if (!text) return;
  const height = container.clientHeight;
  container.querySelector('.slide-reference').style.fontSize = `${Math.max(5, height * .038)}px`;
  container.querySelector('.slide-credit').style.fontSize = `${Math.max(4, height * .025)}px`;
  let size = Math.max(8, height * .112); text.style.fontSize = `${size}px`;
  while (size > 6 && (text.scrollHeight > text.clientHeight + 1 || text.scrollWidth > text.clientWidth + 1)) { size -= 1; text.style.fontSize = `${size}px`; }
}
function renderSlide(container, slide, theme = 'midnight', credits = '', scripture = false, hidden = false) {
  if (!container) return;
  container.dataset.theme = theme || 'plain';
  container.classList.toggle('blank', !slide || hidden === 'blackout');
  const content = hidden || !slide ? '' : slide.text;
  container.replaceChildren(element('div', 'slide-text', content), element('div', 'slide-reference', !hidden && slide && scripture ? slide.label : ''), element('div', 'slide-credit', !hidden && slide ? credits : ''));
  resizeObserver.observe(container); fitSlide(container);
}
function liveStatus() {
  if (!state?.live.slide) return 'Congregation display is clear';
  if (state.live.blackout) return 'BLACKOUT · Output held on black';
  if (state.live.clear_text) return 'TEXT CLEARED · Background remains on air';
  return `${state.live.item_title} · ${state.live.slide.label}`;
}
function renderLive() {
  const live = state.live;
  const scripture = state.service.items.find(i => i.id === live.item_id)?.kind === 'scripture' || live.credits === 'King James Version (KJV)';
  renderSlide($(mode === 'output' ? 'audience-slide' : 'live-preview'), live.slide, live.theme, live.credits, scripture, live.blackout ? 'blackout' : live.clear_text ? 'clear' : false);
  if ($('live-label')) $('live-label').textContent = live.slide?.label || 'Nothing on air';
  if ($('live-status')) $('live-status').textContent = liveStatus();
  if ($('blackout')) { $('blackout').classList.toggle('active', live.blackout); $('blackout').textContent = live.blackout ? 'Restore output' : 'Blackout'; }
  if ($('clear')) { $('clear').classList.toggle('active', live.clear_text); $('clear').textContent = live.clear_text ? 'Restore text' : 'Clear text'; }
}
function item() { return draft?.items.find(i => i.id === selectedItem); }
function slide() { return item()?.slides.find(s => s.id === selectedSlide); }
function ensureSelection() {
  if (!item()) selectedItem = draft?.items[0]?.id || null;
  if (!slide()) selectedSlide = item()?.slides[0]?.id || null;
}
function markDirty() { dirty = true; $('save-draft').disabled = false; $('save-draft').textContent = 'Save changes'; $('save-state').textContent = 'Unsaved edits · Live output is unchanged'; }
function renderEditor() {
  const currentItem = item(), currentSlide = slide();
  $('edit-title').value = currentItem?.title || '';
  $('edit-label').value = currentSlide?.label || '';
  $('edit-text').value = currentSlide?.text || '';
  $('edit-credits').value = currentItem?.credits || '';
  document.querySelectorAll('.editor input,.editor textarea').forEach(el => { el.disabled = !currentItem; });
  $('save-draft').disabled = !dirty;
  $('save-draft').textContent = dirty ? 'Save changes' : 'Saved';
  $('save-state').textContent = dirty ? 'Unsaved edits · Live output is unchanged' : 'Changes are saved on this computer.';
}
function renderPreview() {
  renderSlide($('preview'), slide(), draft.theme, item()?.credits || '', item()?.kind === 'scripture');
  $('preview-label').textContent = slide()?.label || 'Select a slide';
  $('item-title').textContent = item()?.title || 'Add your first service item';
  $('item-kind').textContent = item()?.kind?.toUpperCase() || 'SERVICE PLAN';
}
function renderServiceList() {
  $('service-title').textContent = draft.title;
  const list = $('service-list'); list.replaceChildren();
  draft.items.forEach((entry, index) => {
    const button = element('button', `service-item${entry.id === selectedItem ? ' selected' : ''}${entry.id === state.live.item_id ? ' on-air' : ''}`);
    button.append(element('span', 'item-number', String(index + 1).padStart(2, '0')));
    const content = element('span'); content.append(element('strong', '', entry.title), element('small', '', `${entry.kind} · ${entry.slides.length} slides`)); button.append(content);
    button.addEventListener('click', () => { selectedItem = entry.id; selectedSlide = entry.slides[0]?.id; renderDeck(); }); list.append(button);
  });
}
function renderGrid() {
  const grid = $('slide-grid'); resizeObserver.disconnect();
  // Existing monitor elements are observed again by renderPreview / renderLive.
  grid.replaceChildren();
  if (!item()) { grid.append(element('div', 'empty', 'Add a song, Bible passage, or announcement to begin.')); return; }
  item().slides.forEach((entry, index) => {
    const card = element('button', `slide-card${entry.id === selectedSlide ? ' selected' : ''}`);
    const image = element('div', 'slide');
    renderSlide(image, entry, draft.theme, item().credits, item().kind === 'scripture');
    const caption = element('div', 'slide-card-title'); caption.append(element('strong', '', `${index + 1}. ${entry.label}`));
    if (entry.id === state.live.slide?.id && selectedItem === state.live.item_id) caption.append(element('span', 'on-air-dot', '● LIVE'));
    card.append(image, caption);
    card.addEventListener('click', () => { selectedSlide = entry.id; renderDeck(); });
    card.addEventListener('dblclick', () => safe(take)); grid.append(card);
  });
  requestAnimationFrame(() => grid.querySelectorAll('.slide').forEach(fitSlide));
}
function renderDeck() { ensureSelection(); renderServiceList(); renderGrid(); renderPreview(); renderLive(); renderEditor(); }
function renderRemotePlan() {
  const open = new Set([...document.querySelectorAll('.remote-item[open]')].map(el => el.dataset.item));
  $('service-title').textContent = state.service.title;
  $('remote-plan').replaceChildren();
  state.service.items.forEach(entry => {
    const group = element('details', 'remote-item'); group.dataset.item = entry.id;
    group.open = open.has(entry.id) || entry.id === state.live.item_id;
    group.append(element('summary', '', entry.title));
    entry.slides.forEach(cue => {
      const button = element('button', `remote-cue${cue.id === state.live.slide?.id && entry.id === state.live.item_id ? ' on-air' : ''}`);
      button.append(element('strong', '', cue.label), element('small', '', cue.text));
      button.disabled = !connected || busy;
      button.addEventListener('click', () => safe(() => sendCommand({ type: 'take', item_id: entry.id, slide_id: cue.id })));
      group.append(button);
    });
    $('remote-plan').append(group);
  });
}
function applyState(snapshot) {
  if (!snapshot || (state && snapshot.revision < state.revision)) return;
  const serviceChanged = renderedServiceRevision !== snapshot.service_revision;
  state = snapshot; connection(true);
  if (mode === 'operator') {
    if (!draft || (serviceChanged && !dirty)) {
      draft = structuredClone(state.service); draftRevision = state.service_revision;
      renderDeck();
    } else { renderLive(); renderServiceList(); }
    if (!serviceChanged) {
      document.querySelectorAll('.slide-card').forEach((card, i) => {
        const live = item()?.slides[i]?.id === state.live.slide?.id && selectedItem === state.live.item_id;
        const dot = card.querySelector('.on-air-dot');
        if (dot && !live) dot.remove();
        else if (!dot && live) card.querySelector('.slide-card-title').append(element('span', 'on-air-dot', '● LIVE'));
      });
    }
  } else if (mode === 'remote') {
    $('pair-form').hidden = true; $('remote-controls').hidden = false; renderLive(); renderRemotePlan();
  } else renderLive();
  renderedServiceRevision = state.service_revision;
}
async function poll() {
  if (token) {
    try { const snapshot = await api(`/api/state${state ? `?since=${state.revision}` : ''}`); if (snapshot) applyState(snapshot); else connection(true); }
    catch { connection(false); }
  }
  setTimeout(poll, connected ? 180 : 1200);
}
async function safe(action) { try { await action(); } catch (error) { toast(error.message, true); } }
async function saveDraft() {
  if (!dirty) return;
  const snapshot = await api('/api/service', { method: 'PUT', body: JSON.stringify({ revision: draftRevision, service: draft }) });
  dirty = false; draft = structuredClone(snapshot.service); draftRevision = snapshot.service_revision;
  applyState(snapshot); renderEditor();
}
async function sendCommand(command) {
  if (busy || !connected) return;
  busy = true; connection(connected);
  try { applyState(await api('/api/command', { method: 'POST', body: JSON.stringify(command) })); }
  finally { busy = false; connection(connected); }
}
async function take() {
  if (!slide()) return;
  await saveDraft(); await sendCommand({ type: 'take', item_id: selectedItem, slide_id: selectedSlide });
}
function bind(id, fn, event = 'click') { if ($(id)) $(id).addEventListener(event, e => { if (event === 'submit') e.preventDefault(); safe(() => fn(e)); }); }
function newSlide() { return { id: crypto.randomUUID(), label: 'New slide', text: 'Your slide text' }; }
function newItem() { return { id: crypto.randomUUID(), title: 'New announcement', kind: 'announcement', credits: '', slides: [newSlide()] }; }

bind('next', () => sendCommand({ type: 'next' }));
bind('previous', () => sendCommand({ type: 'previous' }));
bind('blackout', () => sendCommand({ type: 'blackout', enabled: !state.live.blackout }));
bind('clear', () => sendCommand({ type: 'clear_text', enabled: !state.live.clear_text }));

if (mode === 'operator') {
  if (!token) { $('operator-app').hidden = true; $('auth-message').hidden = false; }
  bind('take', take);
  bind('save-draft', saveDraft);
  bind('output-button', () => {
    const output = window.open(`/output#${encodeURIComponent(token)}`, 'rustypresenter-output', 'popup,width=1280,height=720');
    if (!output) throw new Error('Allow this browser to open the congregation display window.');
  });
  bind('add-item', () => { const entry = newItem(); draft.items.push(entry); selectedItem = entry.id; selectedSlide = entry.slides[0].id; markDirty(); renderDeck(); });
  bind('add-slide', () => { if (!item()) return; const entry = newSlide(); item().slides.push(entry); selectedSlide = entry.id; markDirty(); renderDeck(); });
  bind('delete-slide', () => {
    if (!item() || item().slides.length < 2) throw new Error('Keep at least one slide in each service item.');
    item().slides = item().slides.filter(s => s.id !== selectedSlide); markDirty(); renderDeck();
  });
  bind('delete-item', () => {
    if (!item() || !confirm(`Remove “${item().title}” from the service? The current live output will remain on air.`)) return;
    draft.items = draft.items.filter(i => i.id !== selectedItem); markDirty(); renderDeck();
  });
  function moveItem(offset) { const index = draft.items.findIndex(i => i.id === selectedItem); const target = index + offset; if (index < 0 || target < 0 || target >= draft.items.length) return; [draft.items[index], draft.items[target]] = [draft.items[target], draft.items[index]]; markDirty(); renderDeck(); }
  bind('move-up', () => moveItem(-1)); bind('move-down', () => moveItem(1));
  ['edit-title', 'edit-label', 'edit-text', 'edit-credits'].forEach(id => bind(id, () => {
    if (!item() || !slide()) return;
    item().title = $('edit-title').value; item().credits = $('edit-credits').value;
    slide().label = $('edit-label').value; slide().text = $('edit-text').value;
    markDirty(); renderGrid(); renderPreview(); renderLive(); renderServiceList();
  }, 'input'));
  document.querySelectorAll('[data-close]').forEach(button => button.addEventListener('click', () => $(button.dataset.close).close()));
  bind('bible-button', async () => { await saveDraft(); $('bible-dialog').showModal(); });
  bind('song-button', async () => { await saveDraft(); $('song-dialog').showModal(); });
  bind('settings-button', () => { $('setting-title').value = draft.title; $('setting-theme').value = draft.theme; $('setting-license').value = draft.ccli_license; $('settings-dialog').showModal(); });
  bind('settings-save', async () => { draft.title = $('setting-title').value; draft.theme = $('setting-theme').value; draft.ccli_license = $('setting-license').value; markDirty(); await saveDraft(); $('settings-dialog').close(); renderDeck(); });
  bind('bible-form', async () => {
    const reference = $('bible-reference').value.trim(); $('bible-add').hidden = true;
    const verses = await api(`/api/bible?reference=${encodeURIComponent(reference)}`);
    $('bible-results').replaceChildren();
    verses.forEach(verse => { const p = element('p'); p.append(element('strong', '', `${verse.reference} · KJV`), document.createTextNode(verse.text)); $('bible-results').append(p); });
    lastBibleReference = reference; $('bible-add').hidden = false;
  }, 'submit');
  bind('bible-add', async () => {
    const snapshot = await api('/api/import/bible', { method: 'POST', body: JSON.stringify({ reference: lastBibleReference }) });
    selectedItem = snapshot.service.items.at(-1).id; selectedSlide = null; applyState(snapshot); $('bible-dialog').close(); toast('Bible slides added to the service.');
  });
  bind('song-file', async () => {
    const file = $('song-file').files[0]; if (!file) return;
    try {
      const text = await file.text();
      const snapshot = await api('/api/import/songselect', { method: 'POST', body: JSON.stringify({ text, lines_per_slide: Number($('song-lines').value) }) });
      selectedItem = snapshot.service.items.at(-1).id; selectedSlide = null; applyState(snapshot); $('song-dialog').close(); toast('Song imported and split into lyric slides.');
    } finally { $('song-file').value = ''; }
  }, 'change');
  bind('export-service', async () => {
    await saveDraft(); const url = URL.createObjectURL(new Blob([JSON.stringify(draft, null, 2)], { type: 'application/json' }));
    const link = element('a'); link.href = url; link.download = `${draft.title.replace(/[^a-z0-9 -]/gi, '').trim() || 'service'}.json`; link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
  });
  bind('load-service', () => $('service-file').click());
  bind('service-file', async () => {
    const file = $('service-file').files[0]; if (!file) return;
    try {
      const service = JSON.parse(await file.text());
      if (!confirm('Replace the service plan with this file? The current live output will remain on air.')) return;
      const snapshot = await api('/api/service', { method: 'PUT', body: JSON.stringify({ revision: state.service_revision, service }) });
      dirty = false; draft = null; applyState(snapshot); toast('Service loaded.');
    } finally { $('service-file').value = ''; }
  }, 'change');
  document.addEventListener('keydown', e => {
    if (e.repeat || e.ctrlKey || e.metaKey || e.altKey || e.target.closest('input,textarea,select,button,a,summary') || document.querySelector('dialog[open]')) return;
    if (e.key === 'ArrowRight' || e.key === ' ') { e.preventDefault(); safe(() => sendCommand({ type: 'next' })); }
    if (e.key === 'ArrowLeft') { e.preventDefault(); safe(() => sendCommand({ type: 'previous' })); }
    if (e.key === 'Enter') { e.preventDefault(); safe(take); }
    if (e.key.toLowerCase() === 'b') { e.preventDefault(); safe(() => sendCommand({ type: 'blackout', enabled: !state?.live.blackout })); }
  });
  window.addEventListener('beforeunload', e => { if (dirty) { e.preventDefault(); e.returnValue = ''; } });
  if (token) safe(async () => {
    const info = await api('/api/operator'); $('pairing-code').textContent = info.pairing_code;
    $('remote-url').textContent = info.remote_url || 'Open http://YOUR-COMPUTER-IP:4318/remote on your phone';
  });
} else if (mode === 'remote') {
  bind('pair-form', async () => {
    const result = await api('/api/pair', { method: 'POST', body: JSON.stringify({ code: $('pair-code').value }) });
    token = result.token; sessionStorage.setItem('rustypresenter-remote', token);
    state = null; applyState(await api('/api/state')); $('pair-code').value = ''; toast('Remote connected.');
  }, 'submit');
  bind('disconnect', () => { token = ''; state = null; sessionStorage.removeItem('rustypresenter-remote'); $('pair-form').hidden = false; $('remote-controls').hidden = true; connection(false); });
} else {
  bind('fullscreen', () => document.documentElement.requestFullscreen());
  document.addEventListener('fullscreenchange', () => { $('fullscreen').hidden = Boolean(document.fullscreenElement); });
}
poll();
