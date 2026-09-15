/* play-fmv.js - STR / MDEC movie playback on the play page.
 *
 * Classic script (no `import` / `export` / `import.meta` - see
 * docs/tooling/site-shell.md). Defines `window.LegaiaPlayFmv`.
 *
 * The engine (`crates/web-viewer/src/play_fmv.rs`) owns every decision: when a
 * movie is armed, which raw-sector window of which `MV*.STR` it is, when the
 * world is held, and whether a pad edge may abort it (retail: the attract
 * movie only, on a face button / Select - routed engine-side off the pad word
 * the page already feeds, so this file reads no keyboard at all). The page's
 * job is the part the runtime cannot do: it still holds the raw disc bytes,
 * it owns the screen, and it owns the audio clock.
 *
 *   1. `play_fmv_wanted_json()` non-null -> slice `window.__playDiscBytes` at
 *      `first_sector * 2352` for `sector_count` raw sectors and hand them to
 *      `play_fmv_install(sectors)`.
 *   2. `play_fmv_active()` -> take the PCM once (`play_fmv_audio_pcm_i16`),
 *      play it through an AudioContext, and draw `play_fmv_frame_rgba(idx)`
 *      onto an overlay canvas over the GL view. The frame index is clocked
 *      off `audioCtx.currentTime` - the native window's `due_video_frame`
 *      rule (the picture follows the soundtrack, never a free-running timer);
 *      a silent movie / suspended context falls back to the wall clock.
 *   3. When the clock passes the last frame, `play_fmv_finish()`. The engine
 *      releases the world on its next tick / title step and runs the
 *      post-movie hand-off; the overlay tears down when `play_fmv_active()`
 *      reads false again.
 *
 * Retail decodes each movie into a 320x240 screen at `fb_y = 8`
 * (docs/formats/str-fmv-table.md), so a 320x224 frame sits eight lines down
 * over black; the overlay is that screen, stretched over the 4:3 view.
 *
 * Call `LegaiaPlayFmv.service(rt, hostEl)` once per animation frame from every
 * loop that ticks the runtime (the field loop and the boot-title loop);
 * `hostEl` is the positioned wrap the overlay canvas is appended to. Returns
 * true while a movie owns the screen. */
(function () {
  'use strict';

  /* Raw Mode-2 sector size. Paired with RAW_SECTOR_SIZE in
   * crates/web-viewer/src/play_fmv.rs. */
  const RAW_SECTOR = 2352;
  /* The retail decode screen the frame is centred on. */
  const SCREEN_W = 320, SCREEN_H = 240;

  const st = {
    rt: null,          /* the runtime instance support was declared on */
    host: null,
    canvas: null,      /* the overlay canvas over the GL view */
    ctx: null,
    frameCanvas: null, /* w x h scratch the RGBA frame lands on */
    frameCtx: null,
    audioCtx: null,
    source: null,
    audioStart: null,  /* audioCtx.currentTime when the PCM began, or null */
    wallStart: 0,
    fps: 15,
    frameCount: 0,
    width: 0,
    height: 0,
    lastIdx: -1,
    active: false,
    finished: false,
  };

  function hasApi(rt) {
    return !!rt && typeof rt.play_fmv_wanted_json === 'function'
      && typeof rt.play_fmv_install === 'function'
      && typeof rt.play_fmv_active === 'function';
  }

  function ensureCanvas(host) {
    if (st.canvas && st.canvas.parentNode === host) return;
    if (st.canvas && st.canvas.parentNode) st.canvas.parentNode.removeChild(st.canvas);
    const cv = document.createElement('canvas');
    cv.className = 'play-fmv-overlay';
    cv.width = SCREEN_W; cv.height = SCREEN_H;
    cv.style.position = 'absolute';
    cv.style.inset = '0';
    cv.style.width = '100%';
    cv.style.height = '100%';
    cv.style.pointerEvents = 'none';
    cv.style.background = '#000';
    cv.style.display = 'none';
    cv.setAttribute('aria-hidden', 'true');
    host.appendChild(cv);
    st.canvas = cv;
    st.ctx = cv.getContext('2d');
    st.host = host;
  }

  /* Slice the wanted sector window out of the page's disc bytes and install
   * it. A page without the bytes installs an empty slice on purpose: the
   * engine then finishes the beat unplayed on its next tick instead of
   * waiting out its install timeout. */
  function install(rt, wanted) {
    const disc = window.__playDiscBytes;
    let slice = new Uint8Array(0);
    if (disc && disc.length) {
      const start = wanted.first_sector * RAW_SECTOR;
      const end = Math.min(disc.length, start + wanted.sector_count * RAW_SECTOR);
      if (end > start) slice = disc.subarray(start, end);
    } else {
      console.warn('fmv: no disc bytes on the page (window.__playDiscBytes); skipping', wanted.path);
    }
    let ok = false;
    try { ok = rt.play_fmv_install(slice); } catch (e) { console.warn('fmv: install failed', e); }
    if (!ok) console.warn('fmv: engine rejected the movie', wanted.path);
  }

  function startAudio(rt) {
    st.audioStart = null;
    let pcm = null;
    try { pcm = rt.play_fmv_audio_pcm_i16(); } catch (e) { pcm = null; }
    const rate = rt.play_fmv_audio_rate();
    const channels = rt.play_fmv_audio_channels();
    if (!pcm || !pcm.length || !rate || !channels) return;
    try {
      if (!st.audioCtx) {
        const Ctor = window.AudioContext || window.webkitAudioContext;
        if (!Ctor) return;
        st.audioCtx = new Ctor();
      }
      const ctx = st.audioCtx;
      if (ctx.state === 'suspended') { try { ctx.resume(); } catch (e) {} }
      const frames = Math.floor(pcm.length / channels);
      const buf = ctx.createBuffer(channels, frames, rate);
      for (let c = 0; c < channels; c++) {
        const out = buf.getChannelData(c);
        for (let i = 0; i < frames; i++) out[i] = pcm[i * channels + c] / 32768;
      }
      const src = ctx.createBufferSource();
      src.buffer = buf;
      src.connect(ctx.destination);
      src.start();
      st.source = src;
      st.audioStart = ctx.currentTime;
    } catch (e) {
      console.warn('fmv: audio start failed; video runs on the wall clock', e);
      st.audioStart = null;
    }
  }

  function start(rt, host) {
    ensureCanvas(host);
    const size = rt.play_fmv_size();
    st.width = size[0] | 0; st.height = size[1] | 0;
    st.frameCount = rt.play_fmv_frame_count() | 0;
    const fps = rt.play_fmv_fps();
    st.fps = (fps > 0.5 && isFinite(fps)) ? fps : 15;
    st.lastIdx = -1;
    st.finished = false;
    /* The frame is centred on the retail decode screen when it fits (the
     * `fb_y = 8` letterbox of a 320x224 movie); a larger frame is its own
     * screen. */
    const sw = Math.max(SCREEN_W, st.width), sh = Math.max(SCREEN_H, st.height);
    st.canvas.width = sw; st.canvas.height = sh;
    st.frameCanvas = document.createElement('canvas');
    st.frameCanvas.width = Math.max(1, st.width);
    st.frameCanvas.height = Math.max(1, st.height);
    st.frameCtx = st.frameCanvas.getContext('2d');
    st.ctx.fillStyle = '#000';
    st.ctx.fillRect(0, 0, sw, sh);
    st.canvas.style.display = 'block';
    st.wallStart = performance.now() / 1000;
    startAudio(rt);
    st.active = true;
    draw(rt, 0);
  }

  /* Audio-cursor clock while the context is running, wall clock otherwise
   * (the native `due_video_frame(audio_secs, wall_elapsed, period)` rule). */
  function clock() {
    if (st.audioStart !== null && st.audioCtx && st.audioCtx.state === 'running') {
      return st.audioCtx.currentTime - st.audioStart;
    }
    return performance.now() / 1000 - st.wallStart;
  }

  function draw(rt, idx) {
    if (idx === st.lastIdx) return;
    let rgba = null;
    try { rgba = rt.play_fmv_frame_rgba(idx); } catch (e) { rgba = null; }
    st.lastIdx = idx;
    if (!rgba || rgba.length !== st.width * st.height * 4) return;   /* keep the last picture */
    const img = new ImageData(new Uint8ClampedArray(rgba.buffer, rgba.byteOffset, rgba.length), st.width, st.height);
    st.frameCtx.putImageData(img, 0, 0);
    const x = ((st.canvas.width - st.width) / 2) | 0;
    const y = ((st.canvas.height - st.height) / 2) | 0;
    st.ctx.drawImage(st.frameCanvas, x, y);
  }

  function teardown() {
    if (st.source) { try { st.source.stop(); } catch (e) {} st.source = null; }
    st.audioStart = null;
    if (st.canvas) st.canvas.style.display = 'none';
    st.active = false;
    st.finished = false;
    st.lastIdx = -1;
  }

  /* Per-frame service. Returns true while a movie owns the screen. */
  function service(rt, host) {
    if (!hasApi(rt)) return false;
    if (st.rt !== rt) {
      /* A fresh runtime (first boot, or a rebuild after a trap): declare
       * support on it, and drop whatever the old one was showing. */
      if (st.active) teardown();
      try { rt.play_fmv_set_supported(true); } catch (e) {}
      st.rt = rt;
    }
    let active = false;
    try { active = rt.play_fmv_active(); } catch (e) { active = false; }
    if (!active) {
      if (st.active) teardown();
      let wanted = null;
      try {
        const j = rt.play_fmv_wanted_json();
        wanted = (j && j !== 'null') ? JSON.parse(j) : null;
      } catch (e) { wanted = null; }
      if (wanted) install(rt, wanted);
      return false;
    }
    if (!st.active) start(rt, host || st.host || document.body);
    const idx = Math.floor(clock() * st.fps);
    if (idx >= st.frameCount) {
      draw(rt, st.frameCount - 1);
      if (!st.finished) {
        st.finished = true;
        try { rt.play_fmv_finish(); } catch (e) {}
      }
      return true;
    }
    draw(rt, Math.max(0, idx));
    return true;
  }

  window.LegaiaPlayFmv = {
    service: service,
    active: function () { return st.active; },
    teardown: teardown,
  };
}());
