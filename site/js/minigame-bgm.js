/* Shared seamless-loop BGM helper for the minigame pages.
 *
 * Renders a global-pool BGM id (2000 + sound-test slot) through the clean-room
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
    gn.gain.value = gain == null ? 0.5 : gain;
    src.connect(gn).connect(ctx.destination);
    src.start();
    return src;
  }

  window.MgBgm2 = {
    render: render,
    start: start,
    /* Forget cached buffers (e.g. after a new disc is loaded). */
    clearCache: function () { for (var k in cache) { if (cache.hasOwnProperty(k)) delete cache[k]; } },
  };
})();
