/* minigame-muscle-sound.js - the Muscle Dome panel's sound cues.
 *
 * The dome's UI blips and its melee impact, decoded once from the visitor's
 * disc through the WASM API and played through one Web Audio context.
 * MgMuscleSound.create(api) returns { playCue, playHit }.
 *
 * Classic script; loads before minigame-muscle.js. */
window.MgMuscleSound = (function () {
  'use strict';

  function create(api) {
    /* ---------------- sound ----------------
     *
     * The dome's own cue set, decoded once from the visitor's disc through
     * the WASM API (crates/web-minigames/src/muscle_web.rs):
     *   - the match SM's UI blips: FUN_801d0748 fires 34 immediate
     *     FUN_8004fcc8(0x21/0x22/0x23) calls, whose < 0x40 leg enqueues
     *     id-1 - static descriptor rows 0x20/0x21/0x22, category 0 ->
     *     the PROT 0868 system bank;
     *   - the melee impact: the shared battle/duel bank's row 0x09
     *     (category 2 -> PROT 0869), the hit cue of the shared battle
     *     path the dome resolves its command plays through.
     * The id set is traced; WHICH blip fires on which page event is a
     * fitted assignment (the 34 sites spread across phase arms this page
     * does not reproduce one-to-one), and the note says so - that covers
     * the menu-cursor blip, the commit/confirm blip and the disabled-chip
     * buzz alike. */
    let sfx = null;      /* { ctx, confirm, cursor, blip, hit[] } */
    let sfxMeta;         /* parsed muscle_sfx_json (undefined until asked) */

    function audioReady() {
      /* Page-level sound gate (js/audio-toggle.js). */
      if (window.LegaiaSound && !LegaiaSound.isSoundOn()) return null;
      if (!api.muscle_sfx_pcm) return null;
      if (sfxMeta === undefined) {
        try { sfxMeta = JSON.parse(api.muscle_sfx_json()); }
        catch (e) { sfxMeta = null; }
      }
      if (!sfxMeta) return null;
      if (!sfx) {
        const Ctx = window.AudioContext || window.webkitAudioContext;
        if (!Ctx) return null;
        const ctx = new Ctx();
        const mk = (row, voice) => {
          const pcm = api.muscle_sfx_pcm(row, voice);
          const rate = api.muscle_sfx_rate(row, voice);
          if (!pcm.length || !rate) return null;
          const buf = ctx.createBuffer(1, pcm.length, rate);
          const ch = buf.getChannelData(0);
          for (let i = 0; i < pcm.length; i++) ch[i] = pcm[i] / 32768;
          return buf;
        };
        const ui = sfxMeta.ui || [0x20, 0x21, 0x22];
        const hit = [];
        for (let v = 0; v < (sfxMeta.hit_voices || 1); v++) {
          const b = mk(sfxMeta.hit != null ? sfxMeta.hit : 9, v);
          if (b) hit.push(b);
        }
        sfx = { ctx, confirm: mk(ui[0], 0), cursor: mk(ui[1], 0),
                blip: mk(ui[2], 0), hit };
      }
      if (sfx.ctx.state === 'suspended') sfx.ctx.resume();
      return sfx;
    }

    function playBuf(a, buf, gain) {
      if (!a || !buf) return;
      const src = a.ctx.createBufferSource();
      src.buffer = buf;
      const gn = a.ctx.createGain();
      // Every cue on this page funnels through here, so the site master trim
      // (js/layout.js) lands once and the authored per-cue levels stand.
      const trim = window.LEGAIA_MASTER_TRIM == null ? 0.25 : window.LEGAIA_MASTER_TRIM;
      gn.gain.value = gain * trim;
      src.connect(gn).connect(a.ctx.destination);
      src.start();
    }

    function playCue(name, gain) {
      const a = audioReady();
      if (a) playBuf(a, a[name], gain == null ? 0.5 : gain);
    }

    /* The impact cue keys every voice layer its descriptor declares that
     * resolves to a real sample (row 0x09 declares two; a layer whose
     * consecutive tone region names no VAG stays silent). */
    function playHit() {
      const a = audioReady();
      if (!a) return;
      for (const buf of a.hit) playBuf(a, buf, 0.5);
    }

    return { playCue, playHit };
  }

  return { create };
})();
