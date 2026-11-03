/* minigame-muscle-sound.js - the Muscle Dome panel's sound cues.
 *
 * The dome's UI blips and its melee impact, keyed on the page's live SPU
 * (js/minigame-bgm.js `MgSpu`). MgMuscleSound.create(api) returns
 * { playCue, playHit }.
 *
 * Classic script; loads before minigame-muscle.js. */
window.MgMuscleSound = (function () {
  'use strict';

  function create(api) {
    /* ---------------- sound ----------------
     *
     * The dome's own cue set, resolved by the WASM API
     * (crates/web-minigames/src/muscle_web.rs):
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
    let sfxMeta;         /* parsed muscle_sfx_json (undefined until asked) */

    function meta() {
      if (sfxMeta === undefined) {
        try { sfxMeta = JSON.parse(api.muscle_sfx_json()); }
        catch (e) { sfxMeta = null; }
      }
      return sfxMeta;
    }

    function playCue(name) {
      const m = meta();
      if (!m || !window.MgSpu) return;
      const ui = m.ui || [0x20, 0x21, 0x22];
      const row = { confirm: ui[0], cursor: ui[1], blip: ui[2] }[name];
      if (row != null) MgSpu.cue(api, row);
    }

    /* The impact cue keys every voice layer its descriptor declares that
     * resolves to a real sample (row 0x09 declares two; a layer whose
     * consecutive tone region names no VAG stays silent). */
    function playHit() {
      const m = meta();
      if (m && window.MgSpu) MgSpu.cue(api, m.hit != null ? m.hit : 9);
    }

    return { playCue, playHit };
  }

  return { create };
})();
