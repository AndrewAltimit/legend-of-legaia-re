/* Shared one-shot sound-cue player for the site pages that fire SFX
 * (the Baka Fighter minigame, the Tactical Arts viewer).
 *
 * The cues themselves are decoded from the visitor's disc by the WASM
 * `LegaiaSfx` surface (crates/web-viewer/src/sfx_view.rs), which walks the
 * retail chain: SCUS -> the static SFX descriptor table (DAT_8006F198), the
 * class-2 sound bank at PROT 869 -> a VAB, then each cue's descriptor through
 * the clean-room SPU into PCM. Nothing ships with the site; nothing is
 * uploaded.
 *
 * This module is the browser half: it owns one AudioContext, caches an
 * AudioBuffer per cue, and plays a cue by *event name* so pages never
 * hard-code a cue id (the id -> event map lives in Rust, next to its
 * provenance).
 *
 *   await LegaiaSfx.init(wasmModule, discBytes)   // renders every cue
 *   LegaiaSfx.play('baka', 'hit')                 // fire by event name
 *   LegaiaSfx.playCue(0x1A)                       // fire by raw cue id
 *   LegaiaSfx.registerPcm(key, i16, rate, st)     // stash a raw PCM clip
 *   LegaiaSfx.playPcm(key)                        // fire a registered clip
 *   LegaiaSfx.ready()                             // cues rendered?
 *   LegaiaSfx.info()                              // { bank, rate, cues, maps }
 *
 * Autoplay policy: the AudioContext is constructed on the first `play`, which
 * pages only reach from a user gesture (a click / keypress). Muting is the
 * site-wide `LegaiaSound` gate - every play checks it, and a mute stops
 * whatever is already sounding. */
(function () {
  'use strict';

  /* Normalisation target. Retail SFX come out of the SPU well below full
   * scale (the descriptors carry their own mixer-channel volumes, which the
   * page has no mixer to apply), so each cue is scaled to a common peak.
   * Presentation-level gain staging, not a retail value. */
  var PEAK_TARGET = 0.85;
  var MAX_GAIN = 24;

  var api = null;        /* wasm LegaiaSfx */
  var ctx = null;        /* AudioContext (lazily built inside a gesture) */
  var master = null;     /* master GainNode */
  var buffers = {};      /* cue id -> { buf: AudioBuffer, gain: number } */
  var pcmBank = {};      /* key -> { data, rate, stereo, buf, gain } - raw
                          * PCM clips registered by pages (e.g. the arts
                          * page's XA voice channels), independent of the
                          * cue renderer. */
  var maps = {};         /* table name -> { event: {cue, source, why} } */
  var meta = { bank: 0, rate: 0, cues: [] };
  var live = [];         /* sounding AudioBufferSourceNodes */
  var log = [];          /* [{ cue, event, t }] - the headless-check hook */

  function soundOn() {
    return !window.LegaiaSound || window.LegaiaSound.isSoundOn();
  }

  /* Build the AudioContext on demand. Must be reached from a user gesture;
   * browsers otherwise hand back a suspended context (we resume it anyway,
   * which succeeds once a gesture has happened). */
  function audio() {
    if (!ctx) {
      var Ctor = window.AudioContext || window.webkitAudioContext;
      if (!Ctor) return null;
      ctx = new Ctor();
      master = ctx.createGain();
      master.gain.value = 1;
      master.connect(ctx.destination);
    }
    if (ctx.state === 'suspended' && ctx.resume) ctx.resume();
    return ctx;
  }

  /* Copy one cue's interleaved-stereo i16 PCM into an AudioBuffer. */
  function bufferFor(id) {
    if (buffers[id] !== undefined) return buffers[id];
    var c = audio();
    if (!c || !api) return null;
    var pcm = api.cue_pcm_i16(id);           /* Int16Array, interleaved L/R */
    var peak = api.cue_peak(id) / 32768;
    if (!pcm || !pcm.length || peak <= 0) {
      buffers[id] = null;
      return null;
    }
    var frames = pcm.length >> 1;
    /* The SPU renders at 44.1 kHz; the AudioContext resamples if it runs at
     * another rate. */
    var buf = c.createBuffer(2, frames, meta.rate || 44100);
    var l = buf.getChannelData(0);
    var r = buf.getChannelData(1);
    for (var i = 0; i < frames; i++) {
      l[i] = pcm[i * 2] / 32768;
      r[i] = pcm[i * 2 + 1] / 32768;
    }
    buffers[id] = { buf: buf, gain: Math.min(MAX_GAIN, PEAK_TARGET / peak) };
    return buffers[id];
  }

  /* Build (once) the AudioBuffer for a registered raw-PCM clip. */
  function pcmBufferFor(key) {
    var p = pcmBank[key];
    if (!p || !p.data || !p.data.length) return null;
    if (p.buf) return p;
    var c = audio();
    if (!c) return null;
    var chs = p.stereo ? 2 : 1;
    var frames = Math.floor(p.data.length / chs);
    if (!frames) return null;
    var buf = c.createBuffer(chs, frames, p.rate || 37800);
    for (var ch = 0; ch < chs; ch++) {
      var d = buf.getChannelData(ch);
      for (var i = 0; i < frames; i++) d[i] = p.data[i * chs + ch] / 32768;
    }
    var d0 = buf.getChannelData(0);
    var peak = 0;
    for (var j = 0; j < d0.length; j++) {
      var v = Math.abs(d0[j]);
      if (v > peak) peak = v;
    }
    p.buf = buf;
    /* XA-decoded voice is near full scale already; cap the boost low. */
    p.gain = peak > 0 ? Math.min(4, PEAK_TARGET / peak) : 1;
    return p;
  }

  /* Start one prepared { buf, gain } through the master chain. */
  function startBuffer(b) {
    var c = audio();
    if (!c) return false;
    var src = c.createBufferSource();
    src.buffer = b.buf;
    var g = c.createGain();
    g.gain.value = b.gain;
    src.connect(g);
    g.connect(master);
    src.onended = function () {
      var i = live.indexOf(src);
      if (i >= 0) live.splice(i, 1);
    };
    live.push(src);
    src.start();
    return true;
  }

  function stopAll() {
    for (var i = 0; i < live.length; i++) {
      try { live[i].stop(); } catch (e) { /* already ended */ }
    }
    live = [];
  }

  var Sfx = {
    /* Render every site cue off `discBytes` (a Uint8Array of the full .bin).
     * `mod` is the already-initialised wasm module. Resolves to the info
     * object, or null when this build/disc has no cue support. */
    init: function (mod, discBytes) {
      if (!mod || typeof mod.LegaiaSfx !== 'function') return Promise.resolve(null);
      try {
        var s = new mod.LegaiaSfx();
        var info = JSON.parse(s.load_disc(discBytes));
        api = s;
        buffers = {};
        meta = { bank: info.bank, rate: info.rate, cues: info.cues };
        maps = {
          baka: JSON.parse(s.baka_cues_json()),
          arts: JSON.parse(s.art_cues_json()),
        };
        console.log('[sfx] ' + info.cues.length + ' cues from PROT ' + info.bank +
                    ' @ ' + info.rate + ' Hz');
        return Promise.resolve(Sfx.info());
      } catch (err) {
        console.warn('[sfx] cue decode failed:', err.message || err);
        api = null;
        return Promise.resolve(null);
      }
    },

    ready: function () { return !!api; },

    info: function () {
      return {
        bank: meta.bank, rate: meta.rate, cues: meta.cues, maps: maps,
        ctx: ctx ? ctx.state : 'none',
      };
    },

    /* Peak absolute sample of the AudioBuffer a cue actually plays through
     * (0 = silence / not built). Builds the buffer if it isn't cached, so it
     * needs the AudioContext - i.e. a user gesture. The verification hook:
     * "did a non-silent buffer reach WebAudio", answered on the browser side
     * rather than trusting the Rust-reported peak. */
    bufferPeak: function (id) {
      var b = bufferFor(id);
      if (!b) return 0;
      var ch = b.buf.getChannelData(0);
      var peak = 0;
      for (var i = 0; i < ch.length; i++) {
        var v = Math.abs(ch[i]);
        if (v > peak) peak = v;
      }
      return peak * b.gain;
    },

    /* The cue id an event resolves to (-1 when unknown / not loaded). */
    cueFor: function (table, event) {
      var m = maps[table];
      if (!m || !m[event]) return -1;
      return m[event].cue;
    },

    /* Fire one cue by raw id. No-op when muted, unrendered, or unsupported. */
    playCue: function (id, event) {
      if (!api || !soundOn()) return false;
      var b = bufferFor(id);
      if (!b) return false;
      if (!startBuffer(b)) return false;
      log.push({ cue: id, event: event || null, t: Date.now() });
      if (log.length > 400) log.shift();
      return true;
    },

    /* Register a raw i16 PCM clip under `key` (e.g. the arts page's
     * disc-demuxed XA voice channels). `data` is an Int16Array (interleaved
     * L/R when `stereo`); `rate` in Hz. Independent of the cue renderer -
     * works even when the descriptor-table decode was skipped. */
    registerPcm: function (key, data, rate, stereo) {
      if (!key || !data || !data.length) return false;
      pcmBank[key] = { data: data, rate: rate || 37800, stereo: !!stereo,
                       buf: null, gain: 1 };
      return true;
    },

    hasPcm: function (key) { return !!pcmBank[key]; },

    /* Fire one registered PCM clip. Logged like a cue (cue: key). */
    playPcm: function (key, event) {
      if (!soundOn()) return false;
      var b = pcmBufferFor(key);
      if (!b) return false;
      if (!startBuffer(b)) return false;
      log.push({ cue: key, event: event || null, t: Date.now() });
      if (log.length > 400) log.shift();
      return true;
    },

    /* Fire one cue by event name (see the Rust event tables). */
    play: function (table, event) {
      var id = Sfx.cueFor(table, event);
      if (id < 0) return false;
      return Sfx.playCue(id, table + '.' + event);
    },

    /* Fire `event` after `ms` milliseconds. Returns a cancel handle. */
    playIn: function (table, event, ms) {
      return window.setTimeout(function () { Sfx.play(table, event); }, ms);
    },

    stopAll: stopAll,

    /* Headless-verification hook: every cue this page has fired. */
    log: function () { return log.slice(); },
    clearLog: function () { log = []; },
  };

  /* Muting kills anything already sounding. */
  if (window.LegaiaSound) {
    window.LegaiaSound.onChange(function (on) { if (!on) stopAll(); });
  }

  window.LegaiaSfx = Sfx;
})();
