// Shared slide renderer: the room output, the operator thumbnails and the live
// monitor all draw slides with this code, so what you see is what they see.

export const W = 1920;

export function assetUrl(a) { return a ? `/assets/${encodeURIComponent(a)}` : ''; }

/** A 1920-wide logical canvas, scaled to fill `host`. Height follows `aspect` (h/w). */
export function makeCanvas(host, aspect = 9 / 16) {
  const canvas = document.createElement('div');
  canvas.className = 'rp-canvas';
  const H = Math.round(W * aspect);
  canvas.style.cssText = `position:absolute;left:0;top:0;width:${W}px;height:${H}px;transform-origin:0 0;overflow:hidden;background:#000;`;
  if (getComputedStyle(host).position === 'static') host.style.position = 'relative';
  host.appendChild(canvas);
  const fitNow = () => {
    const s = Math.min(host.clientWidth / W, host.clientHeight / H) || 0;
    canvas.style.transform = `scale(${s})`;
    canvas.style.left = `${(host.clientWidth - W * s) / 2}px`;
    canvas.style.top = `${(host.clientHeight - H * s) / 2}px`;
  };
  new ResizeObserver(fitNow).observe(host);
  fitNow();
  canvas.H = H;
  return canvas;
}

const layerCss = 'position:absolute;inset:0;';

export function backgroundEl(bg, { live = false } = {}) {
  const el = document.createElement('div');
  el.style.cssText = layerCss + 'background:#000;';
  if (!bg) return el;
  if (bg.type === 'color') el.style.background = bg.color;
  else if (bg.type === 'image') el.append(imgEl(bg.asset));
  else if (bg.type === 'video') {
    const v = document.createElement('video');
    v.muted = true; v.loop = true; v.playsInline = true; v.preload = 'auto';
    v.style.cssText = layerCss + 'width:100%;height:100%;object-fit:cover;';
    v.src = assetUrl(bg.asset) + (live ? '' : '#t=0.5');
    if (live) v.autoplay = true;
    el.append(v);
  }
  return el;
}

export function imgEl(asset, fit = 'cover') {
  const i = document.createElement('img');
  i.src = assetUrl(asset);
  i.decoding = 'async';
  i.draggable = false;
  i.style.cssText = layerCss + `width:100%;height:100%;object-fit:${fit};`;
  return i;
}

/** Text slide (lyrics/scripture/announcement). */
export function textEl(layer, theme, H) {
  const t = theme || {};
  const scripture = !!layer.scripture;
  const wrap = document.createElement('div');
  wrap.style.cssText = layerCss + 'display:flex;flex-direction:column;isolation:isolate;';
  const lower = t.position === 'lower';
  const body = document.createElement('div');
  const pad = 110;
  body.style.cssText = `flex:1;display:flex;align-items:${lower ? 'flex-end' : 'center'};justify-content:center;padding:${pad * 0.6}px ${pad}px ${lower ? 70 : pad * 0.6}px;min-height:0;`;
  const text = document.createElement('div');
  text.className = 'rp-text';
  const size = scripture ? (t.scripture_size || 64) : (t.size || 84);
  text.style.cssText = `font-family:"${t.font || 'Segoe UI'}", "Segoe UI", system-ui, sans-serif;font-weight:${scripture ? 500 : 600};color:${t.color || '#fff'};` +
    `font-size:${size}px;line-height:${scripture ? 1.32 : 1.22};text-align:${scripture ? 'left' : 'center'};white-space:pre-wrap;overflow-wrap:break-word;max-width:100%;max-height:100%;` +
    (t.shadow !== false ? 'text-shadow:0 3px 14px rgba(0,0,0,.85),0 0 3px rgba(0,0,0,.9);' : '') +
    (t.uppercase && !scripture ? 'text-transform:uppercase;' : '');
  text.textContent = layer.text || '';
  text.dataset.max = size;
  body.append(text);
  wrap.append(body);
  if (layer.reference || layer.footer) {
    const foot = document.createElement('div');
    foot.style.cssText = `flex:none;padding:0 ${pad}px 46px;display:flex;justify-content:${layer.reference ? 'flex-end' : 'center'};` +
      `font-family:"${t.font || 'Segoe UI'}", "Segoe UI", system-ui, sans-serif;color:${t.color || '#fff'};opacity:.88;` +
      (t.shadow !== false ? 'text-shadow:0 2px 8px rgba(0,0,0,.9);' : '');
    foot.style.fontSize = layer.reference ? '44px' : '30px';
    foot.style.fontWeight = layer.reference ? '600' : '400';
    foot.textContent = layer.reference || layer.footer;
    wrap.append(foot);
  }
  if (lower) {
    const shade = document.createElement('div');
    shade.style.cssText = layerCss + 'top:auto;height:55%;background:linear-gradient(transparent, rgba(0,0,0,.55));z-index:-1;';
    wrap.prepend(shade);
  }
  return wrap;
}

/** Shrink text until it fits its box. Never drops words. Call once the element is in the DOM. */
export function fitText(root) {
  root.querySelectorAll('.rp-text').forEach(el => {
    let size = +el.dataset.max || 80;
    el.style.fontSize = size + 'px';
    const box = el.parentElement;
    let guard = 60;
    while (guard-- > 0 && size > 26 && (el.scrollHeight > box.clientHeight - 4 || el.scrollWidth > box.clientWidth + 1)) {
      size -= size > 60 ? 4 : 2;
      el.style.fontSize = size + 'px';
    }
  });
}

export function logoEl(logo) {
  const el = document.createElement('div');
  el.style.cssText = layerCss + 'background:#000;display:flex;align-items:center;justify-content:center;';
  if (logo) { const i = imgEl(logo, 'contain'); i.style.cssText = 'max-width:70%;max-height:70%;object-fit:contain;'; el.append(i); }
  return el;
}

/** Slide content -> the layer the engine would build (mirror of live.rs `go`). */
export function slideToLayers(slide, kind, background) {
  const c = slide.content;
  switch (c.type) {
    case 'text': return { background, slide: { type: 'text', text: c.text, footer: c.footer, reference: c.reference, scripture: kind === 'scripture' } };
    case 'image': return { slide: { type: 'image', asset: c.asset } };
    case 'video': return { background, media: { type: 'video', asset: c.asset } };
    case 'still': return { background, media: { type: 'still', asset: c.asset } };
    case 'audio': return { audio: { asset: c.asset } };
  }
  return {};
}

/** Static drawing of a frame (thumbnails, live monitor). */
export function drawStatic(canvas, frame, theme, logo, { mirrorVideo = null } = {}) {
  canvas.replaceChildren();
  if (frame.background) canvas.append(backgroundEl(frame.background, {}));
  if (frame.media) {
    if (frame.media.type === 'still') canvas.append(imgEl(frame.media.asset, 'contain'));
    else if (frame.media.type === 'video') {
      const v = mirrorVideo || document.createElement('video');
      if (!mirrorVideo) { v.muted = true; v.preload = 'metadata'; v.src = assetUrl(frame.media.asset) + '#t=1'; }
      v.style.cssText = layerCss + 'width:100%;height:100%;object-fit:contain;background:#000;';
      canvas.append(v);
    }
  }
  if (frame.slide) {
    if (frame.slide.type === 'image') canvas.append(imgEl(frame.slide.asset, 'contain'));
    else canvas.append(textEl(frame.slide, theme, canvas.H));
  }
  if (frame.logo) canvas.append(logoEl(logo));
  if (frame.audio && !frame.slide && !frame.media && !frame.background && !frame.logo) {
    const n = document.createElement('div');
    n.style.cssText = layerCss + 'display:flex;align-items:center;justify-content:center;color:#9aa3b5;font:600 120px "Segoe UI",system-ui;';
    n.textContent = '♪';
    canvas.append(n);
  }
  requestAnimationFrame(() => fitText(canvas));
}
