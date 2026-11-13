/* play-fmv.js - STR / MDEC movie playback on the play page.
 *
 * Classic script (no `import` / `export` / `import.meta` - see
 * docs/tooling/site-shell.md). Defines `window.LegaiaPlayFmv`.
 *
 * The engine (`crates/web-play/src/play_fmv.rs`) owns every decision: when a
 * movie is armed, which raw-sector window of which `MV*.STR` it is, when the
 * world is held, and whether a pad edge may abort it (retail: the attract
 * movie only, on a face button / Select - routed engine-side off the pad word
 * the page already feeds, so this file reads no keyboard at all). The page's
 * job is the part the runtime cannot do: it reads the raw disc sectors,
 * it owns the screen, and it owns the audio clock.
 *
 *   1. `play_fmv_wanted_json()` non-null -> take `sector_count` raw sectors at
 *      `first_sector` off `window.__playDisc` (js/play-disc.js) and hand them
 *      to `play_fmv_install(sectors)`. The sectors are read when the beat asks;
 *      the engine holds the world until the install, and they are let go right
 *      after it, since the install copies them and demuxes once.
 *   2. `play_fmv_active()` -> `play_fmv_audio_start()` puts the XA track on
 *      the ENGINE mixer's XA lane (the native window's `play_xa`), so the
 *      movie sits behind the same master trim and volume slider as BGM and
 *      SFX - a second, untrimmed AudioContext is what made movies shout
 *      over the rest of the game. Draw `play_fmv_frame_rgba(idx)` onto an
 *      overlay canvas over the GL view, asking `play_fmv_due_frame` for the frame index - the
 *      engine's `MovieClock`, the pacing rule the native window runs
 *      (the picture follows the soundtrack, never a free-running timer). A
 *      page with no engine audio up falls back to `play_fmv_audio_pcm_i16`
 *      through its own context, trimmed by `LEGAIA_MASTER_TRIM`, the play
 *      page's own half (`PLAY_PAGE_GAIN_SCALE`, as on the engine path), the
 *      volume slider and the site sound switch; a silent
 *      movie or a cursor that never moves falls back to the wall clock.
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

  /* The retail decode screen the frame is centred on. */
  const SCREEN_W = 320, SCREEN_H = 240;

  const st = {
    rt: null,          /* the runtime instance support was declared on */
    host: null,
    canvas: null,      /* the overlay canvas over the GL view */
    ctx: null,
    frameCanvas: null, /* w x h scratch the RGBA frame lands on */
    frameCtx: null,
    audioCtx: null,    /* the FALLBACK context (no engine audio) */
    source: null,
    audioStart: null,  /* audioCtx.currentTime when fallback PCM began, or null */
    engineAudio: false,/* the track is on the engine mixer; clock off its cursor */
    wallStart: 0,
    fps: 15,
    frameCount: 0,
    width: 0,
    height: 0,
    lastIdx: -1,
    active: false,
    finished: false,
    asked: null,       /* the window this page has the disc holding for a beat */
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

  /* Let the disc drop the window a beat asked for: installed, or no longer wanted. */
  function letGo() {
    if (st.asked) st.asked.disc.release(st.asked.first, st.asked.count, true);
    st.asked = null;
  }

  /* Install the wanted sector window off the page's disc. A page without the
   * disc, or that cannot read those sectors, installs an empty slice on
   * purpose: the engine then finishes the beat unplayed on its next tick
   * instead of waiting out its install timeout. */
  function install(rt, wanted) {
    const disc = window.__playDisc;
    let slice = new Uint8Array(0);
    if (disc) {
      const resident = disc.span(wanted.first_sector, wanted.sector_count);
      if (resident) {
        slice = resident;
      } else if (disc.failedSpan(wanted.first_sector, wanted.sector_count)) {
        console.warn('fmv: the disc would not read; skipping', wanted.path);
      } else {
        /* This service polls every frame: the next one after the read installs. */
        st.asked = { disc, first: wanted.first_sector, count: wanted.sector_count };
        disc.prefetch(wanted.first_sector, wanted.sector_count);
        return;
      }
    } else {
      console.warn('fmv: no disc on the page (window.__playDisc); skipping', wanted.path);
    }
    let ok = false;
    try { ok = rt.play_fmv_install(slice); } catch (e) { console.warn('fmv: install failed', e); }
    if (!ok) console.warn('fmv: engine rejected the movie', wanted.path);
    letGo();
  }

  /* The play page's output slider (`#play-volume`, the engine path's
   * `audio_set_gain` units) behind the site-wide sound switch: the two
   * controls the engine path's movie sits behind, so the fallback output
   * honours a mute and a lowered level too instead of playing at the
   * slider's default. */
  function pageLevel() {
    if (window.LegaiaSound && !window.LegaiaSound.isSoundOn()) return 0;
    const el = document.getElementById('play-volume');
    const v = el ? parseFloat(el.value) : 1;
    return (v >= 0 && isFinite(v)) ? v : 1;
  }

  function startAudio(rt) {
    st.audioStart = null;
    st.engineAudio = false;
    if (typeof rt.play_fmv_audio_start === 'function') {
      try { st.engineAudio = !!rt.play_fmv_audio_start(); } catch (e) { st.engineAudio = false; }
      if (st.engineAudio) return;
    }
    /* Fallback: no engine audio to stage on (or a cached bundle without the
     * export). Play the PCM here, behind the site's trim. */
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
      const trim = ctx.createGain();
      /* The site trim, then the play page's own half - the level the engine
       * path gives a movie at the slider's default 1x (web-play's
       * `PLAY_PAGE_GAIN_SCALE`). */
      const siteTrim = (typeof window.LEGAIA_MASTER_TRIM === 'number') ? window.LEGAIA_MASTER_TRIM : 0.25;
      trim.gain.value = siteTrim * 0.5 * pageLevel();
      src.connect(trim);
      trim.connect(ctx.destination);
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

  /* The frame due now. The pacing rule is the engine's
   * (`MovieClock`, which the native window's movie runs on too): the
   * mixer's XA cursor while the track is on it, else the page's own
   * fallback output's position, else the wall clock - and a cursor stuck at
   * zero past the stall window hands over to the wall clock for good. The
   * page only supplies the two clocks the engine cannot read. */
  function dueFrame(rt) {
    const wall = performance.now() / 1000 - st.wallStart;
    let pageAudio = -1;
    if (st.audioStart !== null && st.audioCtx && st.audioCtx.state === 'running') {
      pageAudio = st.audioCtx.currentTime - st.audioStart;
    }
    return rt.play_fmv_due_frame(wall, pageAudio);
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
    st.engineAudio = false;   /* the engine takes its own track off on finish */
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
      /* Past the install timeout, or a rebuilt engine: a read still in flight
       * lands with nobody waiting and is dropped. */
      if (st.asked && (!wanted || wanted.first_sector !== st.asked.first
        || wanted.sector_count !== st.asked.count || window.__playDisc !== st.asked.disc)) letGo();
      if (wanted) install(rt, wanted);
      return false;
    }
    if (!st.active) start(rt, host || st.host || document.body);
    let idx = 0;
    try { idx = dueFrame(rt); } catch (e) { idx = 0; }
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
    /* Which audio path the open movie took ('engine' / 'page' / 'none'). */
    audioPath: function () { return !st.active ? 'none' : st.engineAudio ? 'engine' : (st.audioStart !== null ? 'page' : 'none'); },
    teardown: teardown,
  };
}());
