/* Shared seamless-loop BGM helper for the minigame pages.
 *
 * Renders a global-pool BGM id (2000 + sound-test slot) through the from-scratch
 * SPU + sequencer on the WASM side (`api.music01_bgm_render`), then plays it
 * as a WebAudio `AudioBufferSourceNode` whose `loopStart` / `loopEnd` are set
 * to exactly one SEQ loop period. That makes the repeat seamless, instead of
 * the old "render 45 s, hard-loop the whole buffer" cut that seamed audibly
 * because 45 s is never a whole number of the track's own loop periods.
 *
 * Every music_01 track is one `[VAB][SEQ]` pair in the bank the engine BGM
 * director loads, decoded live from the visitor's own disc - nothing ships
 * with the page. Playback is gated on the page mute (`js/audio-toggle.js`). */
(function () {
  'use strict';

  var cache = {}; /* bgm id -> { buffer, loopStart, loopEnd, hasLoop } | null */

  /* Render + cache one bgm id into a WebAudio buffer with its loop region.
   * Returns null when the id doesn't decode on this disc (so callers can fall
   * back to silence without throwing). Rendering blocks the thread briefly, so
   * callers schedule it off a click/timeout, not the first frame. */
  function render(api, ctx, bgm, seconds) {
    if (cache[bgm] !== undefined) return cache[bgm];
    if (!api || typeof api.music01_bgm_render !== 'function' || !ctx) return null;
    var r;
    try { r = api.music01_bgm_render(bgm, seconds || 45); } catch (e) { r = null; }
    if (!r || !r.ok) { cache[bgm] = null; return null; }
    var pcm = r.pcm, rate = r.rate;
    var frames = pcm.length / 2;
    if (!frames || !rate) { cache[bgm] = null; return null; }
    var buf = ctx.createBuffer(2, frames, rate);
    var L = buf.getChannelData(0), R = buf.getChannelData(1);
    for (var i = 0; i < frames; i++) { L[i] = pcm[i * 2] / 32768; R[i] = pcm[i * 2 + 1] / 32768; }
    var ls = r.loop_start, le = r.loop_end || frames;
    cache[bgm] = {
      buffer: buf,
      loopStart: ls / rate,
      loopEnd: le / rate,
      hasLoop: le > ls,
    };
    return cache[bgm];
  }

  /* Start a rendered entry as a looping source through a fresh gain node.
   * Unity is the mix's own level: the live SPU's cues come out at unity x the
   * trim too, so the BGM to cue balance is the SPU's, as on the play hosts.
   * Returns the source node (call .stop() to end) or null. When the render
   * found a true loop region the source repeats [loopStart, loopEnd) - one
   * SEQ period - after playing the lead-in once; otherwise it hard-loops the
   * whole buffer (the pre-existing fallback). */
  function start(ctx, entry, gain) {
    if (!ctx || !entry) return null;
    var src = ctx.createBufferSource();
    src.buffer = entry.buffer;
    src.loop = true;
    if (entry.hasLoop) { src.loopStart = entry.loopStart; src.loopEnd = entry.loopEnd; }
    var gn = ctx.createGain();
    // Caller's level x the site master trim (js/layout.js). The caller's
    // own value is untouched - this is the output stage, not the mix.
    var trim = window.LEGAIA_MASTER_TRIM == null ? 0.25 : window.LEGAIA_MASTER_TRIM;
    gn.gain.value = (gain == null ? 1 : gain) * trim;
    src.connect(gn).connect(ctx.destination);
    src.start();
    return src;
  }

  /* ---- the page's LIVE SPU -------------------------------------------
   *
   * Every minigame cue keys a voice on the engine's SPU
   * (`LegaiaMinigames::minigame_audio_*`, the audio director the play hosts
   * run), so it carries the tone's ADSR, the retail volume and pan and the
   * tone's reverb send. Only the BGM above is rendered offline. This is the
   * page-side gate: open it inside a user gesture, mirror the site sound
   * toggle onto it, and tell it which game is open.
   *
   * `api` is the `LegaiaMinigames` instance. Idempotent. */
  var spuOpen = false;
  var spuApi = null;

  function spuReady(api) {
    if (!api || typeof api.minigame_audio_open !== 'function') return false;
    spuApi = api;
    /* Page-level sound gate (js/audio-toggle.js) - same gate the offline cue
     * path above checks, so one toggle silences both. */
    var on = !window.LegaiaSound || LegaiaSound.isSoundOn();
    if (!spuOpen) {
      if (!on) return false;           /* never open a context while muted */
      if (!api.minigame_audio_open()) return false;
      spuOpen = true;
    }
    /* Resume on EVERY call, not only the opening one. The first open can
     * run outside a gesture (the duel's cue load at disc load, a disc that
     * auto-loaded from the cache), and the browser then parks the context
     * `suspended` and refuses a resume until a gesture. Resuming only once
     * left the page's whole live SPU - every minigame cue, the announcer
     * lines, the dome tally - silent for good, while the music (a separate
     * context `MgBgm` resumes on each start) played. A resume of a running
     * context is a no-op. */
    if (on && typeof api.minigame_audio_resume === 'function') {
      try { api.minigame_audio_resume(); } catch (e) {}
    }
    if (typeof api.minigame_audio_set_muted === 'function') {
      try { api.minigame_audio_set_muted(!on); } catch (e) {}
    }
    return on;
  }

  /* The toggle reaches a voice already sounding, such as the reel motor. */
  if (window.LegaiaSound) LegaiaSound.onChange(function () { if (spuApi) spuReady(spuApi); });

  window.MgSpu = {
    ready: spuReady,
    /* The open game's banks: a tab change is that game's mode entry. */
    enter: function (api, game) {
      if (api && typeof api.minigame_audio_enter === 'function') api.minigame_audio_enter(game);
    },
    /* One cue by id. Returns whether a voice keyed - the engine answers false
     * for an id it cannot resolve rather than keying a truncated one, so a
     * false here is information, not a failure. */
    cue: function (api, id) {
      if (!spuReady(api)) return false;
      try { return !!api.minigame_sfx_cue(id); } catch (e) { return false; }
    },
    /* One cue on whichever page instance last opened the SPU, for a caller
     * holding no `api` (the save bar). */
    cueOpen: function (id) {
      return !!spuApi && spuReady(spuApi) && !!spuApi.minigame_sfx_cue(id);
    },
    /* A dance good-step hit sting, keyed direct. */
    sting: function (api, r) {
      if (!spuReady(api)) return 0;
      try { return api.dance_sting_key(r) | 0; } catch (e) { return 0; }
    },
    /* The dome tally's voice-attr cues for one INTERVAL-screen tick. The
     * engine replays the ramp to `t` and keys only the steps it has not keyed
     * yet, which is what lets the page drive it off the same screen tick it
     * draws the rows from. */
    tallyVoice: function (api, t) {
      if (!spuReady(api)) return 0;
      if (typeof api.muscle_tally_voice !== 'function') return 0;
      try { return api.muscle_tally_voice(t) | 0; } catch (e) { return 0; }
    },
    tallyVoiceReset: function (api) {
      if (!api || typeof api.muscle_tally_voice_reset !== 'function') return;
      try { api.muscle_tally_voice_reset(); } catch (e) {}
    },
  };

  window.MgBgm2 = {
    render: render,
    start: start,
    /* Forget cached buffers (e.g. after a new disc is loaded). */
    clearCache: function () { for (var k in cache) { if (cache.hasOwnProperty(k)) delete cache[k]; } },
  };
})();
