/* Baka Fighter duel view - the retail cabinet's own art, drawn from the
 * visitor's disc.
 *
 * Two layers over one <div>:
 *   - a WebGL canvas (the shared TmdRenderer R16UI paletted-VRAM pipeline)
 *     carrying the DUEL SCENE: the two fighters' real meshes - the player side
 *     from the battle-form party pack (PROT 1204), the opponent from its own
 *     per-rung pack (PROT 1206..1219) - posed per frame from their real
 *     animation banks (PROT 1203 for the party, the pack's own anim chunk for
 *     the opponent), plus the arena: the stage TMD's backdrop wall (PROT 1203
 *     descriptor 1) and a floor tiled from the wall's own dominant texture cell;
 *   - a 2D canvas carrying the HUD, drawn from the overlay's own 51-record
 *     widget table (DAT_801d7160) + the PROT 1203 art pages, at the screen
 *     positions traced from the retail HUD renderer FUN_801d2afc.
 *
 * Also hosts the retail PLAYER SELECT screen (the three party fighters idling
 * under the sheet's own banner, widget 12), the winner's post-match punch
 * flourish, and the between-match tally menu - the sheet's NEXT GAME /
 * PAY OUT / GET COIN cells (widgets 44/45/46 + the coin digit strip).
 *
 * Traced vs fitted, stated plainly (mirrors the slot-machine section's
 * contract): every sprite CELL, PALETTE and HUD screen position is read off
 * the disc or the disassembly; the 3D CAMERA and the fighters' floor spacing
 * are fitted by eye, because the duel's GTE matrices live in COP2 and no
 * mid-duel capture exists yet. Nothing is invented art.
 *
 * Requires webgl-math.js + webgl-shaders.js + webgl-tmd.js first.
 */
(function () {
  'use strict';

  const ANIM_FPS = 14;                 /* retail clip rate */
  const A2R = (Math.PI * 2) / 4096;    /* PSX angle units -> radians */

  /* Retail 320x240 HUD frame. */
  const HUD_W = 320, HUD_H = 240;

  /* Widget ids, named from the decoded art itself (the labels describe the
   * cells; the pixels stay on the visitor's disc).
   * Traced draw sites: FUN_801d2afc (HUD), FUN_801d21fc (round banner),
   * FUN_801d69e4 (digit = widget 0x13, u patched digit*8),
   * FUN_801d67f0 case 2 (stage digit = widget 5, u patched digit*24).
   * The tally / cash-out sheet (page 5 of the art pack) carries the retail
   * between-match menu: NEXT GAME (44) / PAY OUT (45) beside GET COIN (46)
   * and its digit strip (47, u = 88 + digit*16); the PLAYER SELECT banner is
   * widget 12 and the select cursor arrows are 48 / 49. */
  const W = {
    PRESS_START: 0,
    NEXT_GAME: 1, RETIRE: 2, ROUND: 3, FIGHT: 4, STAGE_DIGIT: 5,
    FINAL: 6, YOU: 7, WIN: 8, LOSE: 9, DRAW: 10, GAME_OVER: 11,
    PLAYER_SELECT: 12,
    PERFECT: 17, STAGE_SM: 18, DIGIT_SM: 19, PRESS_SELECT: 20,
    VITAL: 24, VICTORY: 26, ITS_NOT_OVER: 28, CONGRATS: 29,
    LOGO_BAKA: 40, LOGO_FIGHTER: 50,
    NEXT_GAME_SM: 44, PAY_OUT: 45, GET_COIN: 46, COIN_DIGIT: 47,
    ARROW_L: 48, ARROW_R: 49,
  };

  /* Anim record slots within a fighter's bank. The bank mirrors the 9-record
   * action table: record 0 is the idle (docs/formats/character-mesh.md) and
   * the damage kernel indexes attacks at records 1..3, the special at 4
   * (legaia_asset::baka_opponents ACTION_ATTACK_BASE / ACTION_SPECIAL); the
   * knockdown slots after them are Inferred from the display-id fold in
   * FUN_801d3f44. */
  const ACT = { IDLE: 0, ATTACK1: 1, ATTACK2: 2, ATTACK3: 3, SPECIAL: 4, HIT: 5 };

  class MinigameBakaView {
    constructor(glCanvas, hudCanvas) {
      this.glCanvas = glCanvas;
      this.hudCanvas = hudCanvas;
      this.renderer = null;
      this.ok = false;
      this.api = null;
      this.widgets = null;
      this.pageCanvases = new Map();   /* "page:palette" -> canvas */
      this.cam = { yaw: 0.0, pitch: 0.1, distance: 1.7 };
      this._attachOrbit();
    }

    /* ---------------- scene assembly ---------------- */

    /* Build the duel scene for one (playerChar, opponentRoster) pairing.
     * Returns true when every asset decoded; false leaves the view inert
     * (the page keeps its text presentation and says why). */
    load(api, playerChar, opponentRoster) {
      this.api = api;
      this.ok = false;
      if (!api.baka_presentation_ready || !api.baka_presentation_ready()) return false;

      this.widgets = JSON.parse(api.baka_hud_json());
      if (!this.widgets.length) return false;
      this.pageCanvases.clear();

      /* Duel facing is DATA, not a hard-coded yaw: which side of the arena
       * each fighter stands on (`side`) and which way it faces (`facing`),
       * read straight off the WASM surface (baka_duel_facing_json, the single
       * source of truth also asserted in baka_presentation_wasm_api.rs). The
       * player stands LEFT and faces RIGHT; the opponent stands RIGHT and
       * faces LEFT - so each looks at the other. */
      this.facing = (api.baka_duel_facing_json
        ? JSON.parse(api.baka_duel_facing_json())
        : { player: { side: -1, facing: 1 }, opponent: { side: 1, facing: -1 } });

      const side = (s, id) => {
        const pos = api.baka_fighter_positions(s, id);
        if (!pos.length) return null;
        return {
          pos,
          uvs: api.baka_fighter_uvs(s, id),
          ct: api.baka_fighter_cba_tsb(s, id),
          idx: api.baka_fighter_indices(s, id),
          oid: api.baka_fighter_object_ids(s, id),
          flat: api.baka_fighter_flat_rgba(s, id),
          parts: api.baka_fighter_part_count(s, id),
        };
      };
      const P = side(0, playerChar);
      const O = side(1, opponentRoster);
      if (!P || !O) return false;

      /* Stage set: PROT 1203 descriptor 1. Mesh 0 is the arena backdrop -
       * a single object: the tall patterned wall with its lattice fences and the
       * two ceiling lamps, authored with its base ON the fighters' floor
       * plane (y = 0) and its face at z 44..225. Under this page's camera
       * convention negative z is behind the fighters, so the whole piece is
       * spun 180 degrees about Y (a rotation, not a mirror - the art is
       * symmetric about the duel line in retail too). Meshes 1..3 are prop
       * pieces whose objects sit in object-local space (they need placement
       * transforms this page hasn't traced), so drawing them raw would pile
       * them at the origin - they are left out, and the section note says
       * so. */
      const stage = this._stageBuffers(api);

      /* Idle clips (frame 0 = the rest pose that assembles the parts). */
      const clip = (s, id, action, parts) => {
        const dims = api.baka_anim_dims(s, id, action);
        if (!dims[0] || !dims[1]) return null;
        const frames = api.baka_anim_pose_frames(s, id, action, parts);
        if (!frames.length) return null;
        return { frames, frameCount: dims[1], parts };
      };
      this.playerChar = playerChar;
      this.opponentRoster = opponentRoster;
      this.clipCache = new Map();
      this.clipFor = (fi, action) => {
        const key = fi + ':' + action;
        if (!this.clipCache.has(key)) {
          this.clipCache.set(key, fi === 0
            ? clip(0, playerChar, action, P.parts)
            : clip(1, opponentRoster, action, O.parts));
        }
        return this.clipCache.get(key);
      };
      const idleP = this.clipFor(0, ACT.IDLE);
      const idleO = this.clipFor(1, ACT.IDLE);
      if (!idleP || !idleO) return false;

      /* Fighter spacing off the assembled rest poses: pose frame 0, measure,
       * then stand the two on the stage floor (model origin = floor). */
      const poseExtent = (f, clipMeta) => {
        const out = new Float32Array(f.pos);
        poseInto(out, f.pos, f.oid, clipMeta, 0, 0, 0);
        let lo = Infinity, hi = -Infinity;
        for (let i = 0; i < out.length; i += 3) {
          if (out[i] < lo) lo = out[i];
          if (out[i] > hi) hi = out[i];
        }
        return (hi - lo) / 2;
      };
      const halfP = poseExtent(P, idleP);
      const halfO = poseExtent(O, idleO);
      this.gap = Math.max(halfP, halfO) * 2.4;

      /* Combined buffers: player verts, opponent verts, stage verts. */
      const nP = P.pos.length / 3, nO = O.pos.length / 3, nS = stage.pos.length / 3;
      const n = nP + nO + nS;
      const pos = new Float32Array(n * 3);
      pos.set(P.pos, 0); pos.set(O.pos, nP * 3); pos.set(stage.pos, (nP + nO) * 3);
      const uvs = new Uint8Array(n * 2);
      uvs.set(P.uvs, 0); uvs.set(O.uvs, nP * 2); uvs.set(stage.uvs, (nP + nO) * 2);
      const ct = new Uint16Array(n * 2);
      ct.set(P.ct, 0); ct.set(O.ct, nP * 2); ct.set(stage.ct, (nP + nO) * 2);
      const flat = new Uint8Array(n * 4);
      flat.set(P.flat, 0); flat.set(O.flat, nP * 4); flat.set(stage.flat, (nP + nO) * 4);
      const idx = new Uint32Array(P.idx.length + O.idx.length + stage.idx.length);
      idx.set(P.idx, 0);
      for (let i = 0; i < O.idx.length; i++) idx[P.idx.length + i] = O.idx[i] + nP;
      for (let i = 0; i < stage.idx.length; i++)
        idx[P.idx.length + O.idx.length + i] = stage.idx[i] + nP + nO;

      this.scene = {
        P, O, nP, nO, nS,
        base: pos.slice(),      /* pristine object-local vertices */
        out: pos,               /* per-frame posed copy */
      };

      if (!this.renderer) this.renderer = new window.TmdRenderer(this.glCanvas);
      this.renderer.uploadVram(api.baka_duel_vram(opponentRoster));
      this.renderer.uploadMesh(pos, uvs, ct, idx, flat);

      /* Frame the pair: centre between the fighters, radius spanning both.
       * FITTED, not read - the retail camera's GTE matrices live in COP2. */
      this.center = [0, -halfP * 0.8, 0];
      this.radius = this.gap * 0.95 + Math.max(halfP, halfO) * 0.4;

      /* Per-fighter live action state. */
      this.action = [
        { id: ACT.IDLE, start: 0, loop: true },
        { id: ACT.IDLE, start: 0, loop: true },
      ];
      this.banner = null;
      this.victory = null;   /* winner's post-match punch flourish */
      this.choice = null;    /* the NEXT GAME / PAY OUT tally menu */
      this.lastKey = '';
      this.roundSeen = -1;
      this.tick = 0;
      this.mode = 'duel';
      this.ok = true;
      return true;
    }

    /* ---------------- fighter-select screen ----------------
     *
     * The retail cabinet opens on a PLAYER SELECT screen: the three party
     * fighters' battle-form models side by side under the sheet's own
     * "PLAYER SELECT" banner (widget 12), with the cursor arrows (widgets
     * 48/49) picking one. This rebuilds that screen from the same assets:
     * the three PROT 1204 meshes idling in front of the arena stage. */
    loadSelect(api) {
      this.api = api;
      this.ok = false;
      if (!api.baka_presentation_ready || !api.baka_presentation_ready()) return false;
      this.widgets = JSON.parse(api.baka_hud_json());
      if (!this.widgets.length) return false;
      this.pageCanvases.clear();

      const fighters = [];
      for (let ch = 0; ch < 3; ch++) {
        const pos = api.baka_fighter_positions(0, ch);
        if (!pos.length) return false;
        const parts = api.baka_fighter_part_count(0, ch);
        const dims = api.baka_anim_dims(0, ch, 0);
        const frames = api.baka_anim_pose_frames(0, ch, 0, parts);
        if (!dims[0] || !dims[1] || !frames.length) return false;
        fighters.push({
          pos,
          uvs: api.baka_fighter_uvs(0, ch),
          ct: api.baka_fighter_cba_tsb(0, ch),
          idx: api.baka_fighter_indices(0, ch),
          oid: api.baka_fighter_object_ids(0, ch),
          flat: api.baka_fighter_flat_rgba(0, ch),
          clip: { frames, frameCount: dims[1], parts },
        });
      }
      const stage = this._stageBuffers(api);

      /* Extent of the widest fighter's rest pose sets the line-up spacing. */
      let half = 0, height = 0;
      for (const f of fighters) {
        const out = new Float32Array(f.pos);
        poseInto(out, f.pos, f.oid, f.clip, 0, 0, 0);
        for (let i = 0; i < out.length; i += 3) {
          half = Math.max(half, Math.abs(out[i]));
          height = Math.max(height, -out[i + 1]);
        }
      }
      this.selGap = half * 2.6;

      /* Combined buffers: the three fighters, then the stage. */
      const counts = fighters.map(f => f.pos.length / 3);
      const nS = stage.pos.length / 3;
      const n = counts.reduce((a, b) => a + b, 0) + nS;
      const pos = new Float32Array(n * 3);
      const uvs = new Uint8Array(n * 2);
      const ct = new Uint16Array(n * 2);
      const flat = new Uint8Array(n * 4);
      const idx = [];
      let vb = 0;
      const bases = [];
      for (const f of fighters) {
        bases.push(vb);
        pos.set(f.pos, vb * 3); uvs.set(f.uvs, vb * 2);
        ct.set(f.ct, vb * 2); flat.set(f.flat, vb * 4);
        for (const ix of f.idx) idx.push(vb + ix);
        vb += f.pos.length / 3;
      }
      pos.set(stage.pos, vb * 3); uvs.set(stage.uvs, vb * 2);
      ct.set(stage.ct, vb * 2); flat.set(stage.flat, vb * 4);
      for (const ix of stage.idx) idx.push(vb + ix);

      this.selScene = {
        fighters, bases,
        base: pos.slice(),
        out: pos,
      };
      if (!this.renderer) this.renderer = new window.TmdRenderer(this.glCanvas);
      /* The party atlases ride in the duel VRAM build; any opponent works. */
      this.renderer.uploadVram(api.baka_duel_vram(5));
      this.renderer.uploadMesh(pos, uvs, ct, new Uint32Array(idx), flat);

      this.center = [0, -height * 0.45, 0];
      this.radius = this.selGap * 1.15 + half;
      this.cam = { yaw: 0.0, pitch: 0.08, distance: 1.55 };
      this.tick = 0;
      this.mode = 'select';
      this.ok = true;
      return true;
    }

    /* Per-frame drive of the select screen: `sel` is the highlighted
     * character (0..2), `names` the three roster names off the disc. */
    frameSelect(sel, names) {
      if (!this.ok || this.mode !== 'select') return;
      this.tick++;
      const S = this.selScene;
      for (let i = 0; i < 3; i++) {
        const f = S.fighters[i];
        const frame = Math.floor(this.tick * (ANIM_FPS / 60)) % f.clip.frameCount;
        /* The picked fighter steps toward the camera; all face it (their
         * intrinsic authored facing, yaw 0). */
        const dx = (i - 1) * this.selGap;
        const dz = i === sel ? 70 : 0;
        poseInto(S.out, S.base, f.oid, f.clip, frame, S.bases[i], dx, 0, dz);
      }
      this.renderer.updatePositions(S.out);
      this.renderer.render(this.cam.yaw, this.cam.pitch, this.cam.distance,
                           0, 0, this.center, this.radius);
      this._drawSelect(sel, names);
    }

    _drawSelect(sel, names) {
      const cv = this.hudCanvas, g = cv.getContext('2d');
      g.setTransform(cv.width / HUD_W, 0, 0, cv.height / HUD_H, 0, 0);
      g.clearRect(0, 0, HUD_W, HUD_H);
      g.imageSmoothingEnabled = false;
      /* The sheet's own PLAYER SELECT banner across the top. */
      this._widget(g, W.PLAYER_SELECT, 160, 24);
      /* Cursor arrows flanking the picked fighter's column (the fighters
       * stand at screen thirds under the fitted select camera). */
      const cx = 160 + (sel - 1) * 96;
      const bob = Math.sin(this.tick * 0.15) * 3;
      this._widget(g, W.ARROW_L, cx - 46 - bob, 120);
      this._widget(g, W.ARROW_R, cx + 46 + bob, 120);
      /* The picked fighter's disc name (UI text - the art pack carries no
       * name font). */
      const name = (names && names[sel]) || '';
      if (name) {
        g.font = 'bold 12px ui-monospace, monospace';
        g.textAlign = 'center';
        g.fillStyle = 'rgba(0,0,0,0.6)';
        g.fillText(name, cx + 1, 206 + 1);
        g.fillStyle = '#ffe9a8';
        g.fillText(name, cx, 206);
      }
      /* PRESS START, blinking like the attract loop. */
      if ((this.tick >> 5) & 1) this._widget(g, W.PRESS_START, 160, 228);
    }

    /* The between-match tally menu (the retail NEXT GAME / PAY OUT cells).
     * `pot` is the run's coins at risk, `sel` 0 = NEXT GAME, 1 = PAY OUT,
     * `flags` optionally { lapClear, allClear }. Clear with setChoice(null). */
    setChoice(choice) {
      this.choice = choice;
    }

    /* Swap the match banner for the full-clear tally (VICTORY! / ALL STAGE
     * CLEAR! / CONGRATULATIONS! + the banked pot). */
    showAllClear(pot) {
      this.banner = { kind: 'all_clear', until: Infinity };
      this.choice = { pot };
      this.victory = this.victory || { fi: 0, step: 0, nextAt: this.tick + 12 };
    }

    /* Build the arena stage buffers: the backdrop wall + the tiled floor. */
    _stageBuffers(api) {
      const stage = { pos: [], uvs: [], ct: [], idx: [], flat: [] };
      for (const si of [0]) {
        const sp = Array.from(api.baka_stage_positions(si));
        if (!sp.length) continue;
        for (let i = 0; i < sp.length; i += 3) { sp[i] = -sp[i]; sp[i + 2] = -sp[i + 2]; }
        const base = stage.pos.length / 3;
        stage.pos.push(...sp);
        stage.uvs.push(...api.baka_stage_uvs(si));
        stage.ct.push(...api.baka_stage_cba_tsb(si));
        stage.flat.push(...api.baka_stage_flat_rgba(si));
        for (const ix of api.baka_stage_indices(si)) stage.idx.push(base + ix);
      }
      /* Arena floor: the stage set carries no floor mesh (the retail camera
       * grazes the ground line), so a floor is TILED from the wall's own
       * wall texture - the dominant face's exact uv cell + CLUT,
       * repeated on the y = 0 plane the fighters and the wall base share.
       * Disc art throughout; the tiling itself is fitted, and the section
       * note says so. */
      this._appendFloor(stage);
      return stage;
    }

    /* Append a tiled floor to the stage buffers (which hold the wall mesh at
     * this point). The tile is the wall's own dominant textured face:
     * its exact uv cell, CLUT and texpage, repeated across the y = 0 plane.
     * No new art - the cell is sampled from the same VRAM the wall draws
     * from; only the tiling layout is fitted. */
    _appendFloor(stage) {
      /* The tile cell: the wall's dominant surface - the textured face with
       * the largest WORLD footprint (the woven wall panel itself), so the
       * floor reads as the same matting the room is built from. */
      let best = null, bestArea = 0;
      for (let t = 0; t + 2 < stage.idx.length; t += 3) {
        const a = stage.idx[t], b = stage.idx[t + 1], c = stage.idx[t + 2];
        if (stage.flat[a * 4 + 3] === 0) continue;      /* untextured prim */
        const us = [stage.uvs[a * 2], stage.uvs[b * 2], stage.uvs[c * 2]];
        const vs = [stage.uvs[a * 2 + 1], stage.uvs[b * 2 + 1], stage.uvs[c * 2 + 1]];
        const xs = [stage.pos[a * 3], stage.pos[b * 3], stage.pos[c * 3]];
        const ys = [stage.pos[a * 3 + 1], stage.pos[b * 3 + 1], stage.pos[c * 3 + 1]];
        const ww = Math.max(...xs) - Math.min(...xs);
        const wh = Math.max(...ys) - Math.min(...ys);
        if (ww * wh <= bestArea) continue;
        bestArea = ww * wh;
        best = {
          u0: Math.min(...us), u1: Math.max(...us),
          v0: Math.min(...vs), v1: Math.max(...vs),
          cba: stage.ct[a * 2], tsb: stage.ct[a * 2 + 1],
          tw: Math.max(64, ww),
          th: Math.max(64, wh),
        };
      }
      if (!best) return;
      /* Grid spanning the arena: wall-to-camera in z, wall width in x. */
      const X0 = -1750, X1 = 1750, Z0 = -260, Z1 = 520;
      const nx = Math.ceil((X1 - X0) / best.tw);
      const nz = Math.ceil((Z1 - Z0) / best.th);
      for (let iz = 0; iz < nz; iz++) {
        for (let ix = 0; ix < nx; ix++) {
          const x0 = X0 + ix * best.tw, x1 = Math.min(x0 + best.tw, X1);
          const z0 = Z0 + iz * best.th, z1 = Math.min(z0 + best.th, Z1);
          const base = stage.pos.length / 3;
          stage.pos.push(x0, 0, z0, x1, 0, z0, x1, 0, z1, x0, 0, z1);
          stage.uvs.push(best.u0, best.v0, best.u1, best.v0,
                         best.u1, best.v1, best.u0, best.v1);
          for (let k = 0; k < 4; k++) {
            stage.ct.push(best.cba, best.tsb);
            stage.flat.push(0, 0, 0, 255);
          }
          stage.idx.push(base, base + 1, base + 2, base, base + 2, base + 3);
        }
      }
    }

    /* Trigger a one-shot clip on fighter `fi` (falls back to idle if the
     * record is missing / empty). `hold` freezes the clip on its final frame
     * instead of dropping back to idle - the loser's stay-down knockdown. */
    play(fi, actionId, hold) {
      if (!this.ok) return;
      const c = this.clipFor(fi, actionId);
      this.action[fi] = c
        ? { id: actionId, start: this.tick, loop: actionId === ACT.IDLE, hold: !!hold }
        : { id: ACT.IDLE, start: this.tick, loop: true };
    }

    /* ---------------- per-frame drive ---------------- */

    /* `st` is the engine's baka_state_json object; `meta` carries
     * { stage, final } for the HUD's stage counter. */
    frame(st, meta) {
      if (!this.ok || this.mode !== 'duel') return;
      this.tick++;

      /* Exchange reactions: winner swings, loser takes the hit. */
      if (st && st.last) {
        const key = JSON.stringify(st.last) + ':' + st.round;
        if (key !== this.lastKey) {
          this.lastKey = key;
          const l = st.last;
          /* Fists flying = the round intro is over. */
          if (this.banner && this.banner.kind === 'round') this.banner = null;
          if (!l.draw) {
            const winner = l.winner, loser = 1 - l.winner;
            const t = st.chosen && st.chosen[winner];
            const atk = l.special ? ACT.SPECIAL
              : t === 2 ? ACT.ATTACK2 : t === 3 ? ACT.ATTACK3 : ACT.ATTACK1;
            this.play(winner, atk);
            this.play(loser, ACT.HIT);
          } else {
            this.play(0, ACT.ATTACK1);
            this.play(1, ACT.ATTACK1);
          }
        }
      }
      if (st && st.round !== this.roundSeen) {
        this.roundSeen = st.round;
        this.banner = { kind: 'round', n: st.round + 1, until: this.tick + 80 };
      }
      if (st && st.phase === 'round_over' && (!this.banner || this.banner.kind === 'round')) {
        const won = st.wins && st.hp ? (st.hp[1] <= 0) : false;
        this.banner = { kind: won ? 'win' : (st.hp[0] <= 0 ? 'lose' : 'draw'),
                        until: this.tick + 90 };
      }
      if (st && st.phase === 'match_over') {
        const kind = st.winner === 0 ? 'match_win' : 'match_lose';
        /* The all-clear tally (showAllClear) supersedes the plain win banner. */
        if ((!this.banner || this.banner.kind !== kind)
            && !(this.banner && this.banner.kind === 'all_clear')) {
          this.banner = { kind, until: Infinity };
          /* Victory flourish: the winner throws a short punch combo (the
           * same attack anim slots the exchanges play); the loser stays
           * down on the final knockdown frame. */
          this.victory = { fi: st.winner, step: 0, nextAt: this.tick + 12 };
          this.play(1 - st.winner, ACT.HIT, true);
        }
      }

      /* Drive the flourish: five swings, ~0.55 s apart, then back to idle. */
      if (this.victory) {
        const v = this.victory;
        if (this.tick >= v.nextAt) {
          if (v.step < 5) {
            this.play(v.fi, [ACT.ATTACK1, ACT.ATTACK3, ACT.ATTACK2][v.step % 3]);
            v.nextAt = this.tick + 34;
            v.step++;
          } else {
            this.victory = null;
          }
        }
      }

      this._pose();
      this.renderer.render(this.cam.yaw, this.cam.pitch, this.cam.distance,
                           0, 0, this.center, this.radius);
      this._drawHud(st, meta);
    }

    _pose() {
      const S = this.scene;
      /* One-shot clips drop back to idle when they run out (held clips
       * freeze on their final frame instead - the stay-down knockdown). */
      for (let fi = 0; fi < 2; fi++) {
        const a = this.action[fi];
        const c = this.clipFor(fi, a.id);
        if (!a.loop && !a.hold && c) {
          const f = Math.floor((this.tick - a.start) * (ANIM_FPS / 60));
          if (f >= c.frameCount) this.action[fi] = { id: ACT.IDLE, start: this.tick, loop: true };
        }
      }
      const poseFighter = (fi, f, vertBase, dx, yaw) => {
        const a = this.action[fi];
        const c = this.clipFor(fi, a.id) || this.clipFor(fi, ACT.IDLE);
        if (!c) return;
        const rawF = Math.floor((this.tick - a.start) * (ANIM_FPS / 60));
        const frame = a.loop ? rawF % c.frameCount
                             : Math.min(rawF, c.frameCount - 1);
        poseInto(S.out, S.base, f.oid, c, frame, vertBase, dx, yaw);
      };
      /* Face the fighters at each other from the layout data. Both mesh
       * families share the same intrinsic authored facing, so they take
       * OPPOSITE world yaws (facing * PI/2): the player (left, facing +1)
       * turns to look right at the opponent, the opponent (right, facing -1)
       * turns to look left at the player. An earlier build spun both the same
       * way on a wrong "opposite intrinsic facing" assumption, so both looked
       * left. */
      const fp = this.facing.player, fo = this.facing.opponent;
      poseFighter(0, S.P, 0, fp.side * this.gap / 2, fp.facing * Math.PI / 2);
      poseFighter(1, S.O, S.nP, fo.side * this.gap / 2, fo.facing * Math.PI / 2);
      /* Stage verts stay at their TMD coordinates (already world-framed). */
      this.renderer.updatePositions(S.out);
    }

    /* ---------------- HUD ---------------- */

    _page(page, palette) {
      const key = page + ':' + palette;
      if (!this.pageCanvases.has(key)) {
        const w = this.api.baka_page_width(page) || 256;
        const rgba = this.api.baka_page_rgba(page, palette);
        if (!rgba.length) { this.pageCanvases.set(key, null); return null; }
        const c = document.createElement('canvas');
        c.width = w; c.height = 256;
        c.getContext('2d').putImageData(
          new ImageData(new Uint8ClampedArray(rgba), w, 256), 0, 0);
        this.pageCanvases.set(key, c);
      }
      return this.pageCanvases.get(key);
    }

    /* Draw widget `id` centred at (cx, cy) - the same contract as the retail
     * emitter FUN_801d5ed0 (scale field applied; abr 1 = additive blend). */
    _widget(g, id, cx, cy, alpha) {
      const w = this.widgets[id];
      if (!w || w.page == null) return;
      const img = this._page(w.page, w.palette);
      if (!img) return;
      const hw = (w.w * w.scale) / 0x1000 / 2, hh = (w.h * w.scale) / 0x1000 / 2;
      g.save();
      if (w.semi && w.abr === 1) g.globalCompositeOperation = 'lighter';
      g.globalAlpha = alpha === undefined ? 1 : alpha;
      g.drawImage(img, w.u, w.v, w.w, w.h, cx - hw, cy - hh, hw * 2, hh * 2);
      g.restore();
    }

    /* A cell straight off a page (for the pip / digit / icon cells whose
     * UVs are computed arithmetically by the HUD renderer, not via the
     * widget table). */
    _cell(g, page, palette, u, v, w, h, x, y) {
      const img = this._page(page, palette);
      if (!img) return;
      g.drawImage(img, u, v, w, h, x, y, w, h);
    }

    _drawHud(st, meta) {
      const cv = this.hudCanvas, g = cv.getContext('2d');
      g.setTransform(cv.width / HUD_W, 0, 0, cv.height / HUD_H, 0, 0);
      g.clearRect(0, 0, HUD_W, HUD_H);
      g.imageSmoothingEnabled = false;
      if (!st || !st.live) {
        /* Attract: the cabinet's own PRESS START strip. */
        this._widget(g, 0, 160, 204);
        return;
      }

      /* The glyph page that carries the pips / combo digits / attack icons
       * (texpage 5 = art page of the widget-0 strip). */
      const glyphPage = this.widgets[W.STAGE_SM] ? this.widgets[W.STAGE_SM].page : 0;

      /* Top strip - FUN_801d2afc tail: "STAGE" + number at (0x30, 0x1e),
       * "PRESS SELECT TO MENU" at (0xea, 0x1e). */
      this._widget(g, W.STAGE_SM, 0x30, 0x1e);
      const stageNo = meta && meta.stage ? meta.stage : 1;
      const digits = String(stageNo).split('').map(Number);
      let dx = 0x40 - (digits.length - 1) * 8;
      for (const d of digits) {
        /* widget 0x13 with u patched to digit*8 (FUN_801d69e4). */
        const w19 = this.widgets[W.DIGIT_SM];
        if (w19) this._cell(g, w19.page, w19.palette, d * 8, w19.v, 8, 8, dx - 4, 0x1e - 4);
        dx += 8;
      }
      this._widget(g, W.PRESS_SELECT, 0xea, 0x1e);

      /* HP bars - FUN_801d2afc: player anchored right at x 0x89, opponent
       * anchored left at 0xb8; y 0x26..0x2b; width hp>>5; gouraud
       * (0xbc, hp>>5, 0) at the far end fading to (0xbc, 0, 0). */
      const bar = (hp, mirror) => {
        const wpx = Math.max(0, hp >> 5);
        const gpk = Math.min(255, Math.max(0, hp >> 5));
        const x0 = mirror ? 0xb8 : 0x89 - wpx;
        const x1 = mirror ? 0xb8 + wpx : 0x89;
        const grad = g.createLinearGradient(x0, 0, x1, 0);
        const far = `rgb(188,${gpk},0)`, near = 'rgb(188,0,0)';
        grad.addColorStop(0, mirror ? near : far);
        grad.addColorStop(1, mirror ? far : near);
        g.fillStyle = grad;
        g.fillRect(x0, 0x26, x1 - x0, 6);
      };
      /* Chrome: the retail frame cells come from a runtime-built table
       * (DAT_801dbc34) this page can't read statically; the VITAL label +
       * a plain outline stand in, and the section note says so. */
      g.strokeStyle = 'rgba(255,255,255,0.55)';
      g.lineWidth = 1;
      g.strokeRect(0x1c + 0.5, 0x25 + 0.5, 110, 8);
      g.strokeRect(0xb0 + 0.5, 0x25 + 0.5, 110, 8);
      this._widget(g, W.VITAL, 0x1c + 16, 0x24);
      this._widget(g, W.VITAL, 0xb0 + 16, 0x24);
      bar(st.hp[0], false);
      bar(st.hp[1], true);

      /* Round-win pips: 16x16 cells at u 0x30 (filled) / 0x40 (empty), v 0;
       * player x 0x70 + i*16, opponent x 0xc0 - i*16, y 0x30. */
      for (let i = 0; i < 2; i++) {
        this._cell(g, glyphPage, 0, st.wins[0] > i ? 0x30 : 0x40, 0, 16, 16,
                   0x70 + i * 16, 0x30);
        this._cell(g, glyphPage, 0, st.wins[1] > i ? 0x30 : 0x40, 0, 16, 16,
                   0xc0 - i * 16 - 16, 0x30);
      }

      /* Combo counters: digit cells (u digit*16, v 0x20) via palette 2,
       * descending from x 0x30 / 0x100; the "HIT!" label cell (0, 0x10)
       * 32x16 via palette 1 right of the digits; y 0x40. Retail draws the
       * OPPONENT's hits-taken counter on the player's side (your streak) and
       * vice versa - the `(uVar8 & 1)` fold in FUN_801d2afc. */
      const combo = (n, x0) => {
        if (!n) return;
        let x = x0, v = n;
        do {
          this._cell(g, glyphPage, 2, (v % 10) * 16, 0x20, 16, 16, x, 0x40);
          x -= 16; v = Math.floor(v / 10);
        } while (v > 0);
        this._cell(g, glyphPage, 1, 0, 0x10, 32, 16, x0 + 16, 0x40);
      };
      combo(st.combo[1], 0x30);
      combo(st.combo[0], 0x100);

      /* Attack-type icon columns: cells (i*16, 0x30) via palette 3 at
       * x 0x20 / 0x110, y 0x60 + i*16; the fighter's committed type lights. */
      for (let i = 0; i < 3; i++) {
        const y = 0x60 + i * 16;
        const litP = st.chosen[0] === i + 1, litO = st.chosen[1] != null;
        g.globalAlpha = litP ? 1 : 0.55;
        this._cell(g, glyphPage, 3, i * 16, 0x30, 16, 16, 0x20, y);
        g.globalAlpha = litO ? 1 : 0.55;
        this._cell(g, glyphPage, 3, i * 16, 0x30, 16, 16, 0x110, y);
        g.globalAlpha = 1;
      }

      /* Banners. */
      if (this.banner && this.tick < this.banner.until) {
        const b = this.banner;
        if (b.kind === 'round') {
          if (meta && meta.final) {
            this._widget(g, W.FINAL, 120, 100);
          } else {
            this._widget(g, W.ROUND, 120, 100);
            const w5 = this.widgets[W.STAGE_DIGIT];
            if (w5) this._cell(g, w5.page, w5.palette, (b.n % 10) * 24, w5.v, 24, 32,
                               120 + 80, 100 - 16);
          }
          if (this.tick > b.until - 40) this._widget(g, W.FIGHT, 160, 132);
        } else if (b.kind === 'win') {
          this._widget(g, W.YOU, 120, 104); this._widget(g, W.WIN, 200, 104);
        } else if (b.kind === 'lose') {
          this._widget(g, W.YOU, 110, 104); this._widget(g, W.LOSE, 205, 104);
        } else if (b.kind === 'draw') {
          this._widget(g, W.DRAW, 160, 104);
        } else if (b.kind === 'match_win') {
          this._widget(g, W.YOU, 120, 80); this._widget(g, W.WIN, 200, 80);
          if (this.choice) this._drawChoice(g, this.choice);
        } else if (b.kind === 'match_lose') {
          /* GAME OVER; a forfeited pot is reported by the page text below
           * the canvas (the sheet has no "lost coins" cell to draw). */
          this._widget(g, W.GAME_OVER, 160, 104);
        } else if (b.kind === 'all_clear') {
          /* The tally sheet's own VICTORY! / ALL STAGE CLEAR! block +
           * CONGRATULATIONS!, with the full pot on the GET COIN line. */
          this._widget(g, W.VICTORY, 160, 76);
          this._widget(g, W.CONGRATS, 160, 128);
          this._widget(g, W.GET_COIN, 120, 156);
          this._coinNumber(g, this.choice ? this.choice.pot : 0, 172, 156);
        }
      }
    }

    /* The retail between-match tally menu: GET COIN <pot>, then the
     * NEXT GAME / PAY OUT cells with the cursor arrow on the picked one.
     * Cell art + palettes are the sheet's own; the screen positions are
     * fitted (the tally screen has no parked capture). */
    _drawChoice(g, ch) {
      this._widget(g, W.GET_COIN, 120, 116);
      this._coinNumber(g, ch.pot, 172, 116);
      if (ch.lapClear) this._widget(g, W.ITS_NOT_OVER, 160, 96);
      const items = [
        { id: W.NEXT_GAME_SM, cx: 92, cy: 150 },
        { id: W.PAY_OUT, cx: 232, cy: 150 },
      ];
      for (let i = 0; i < 2; i++) {
        g.globalAlpha = ch.sel === i ? 1 : 0.45;
        this._widget(g, items[i].id, items[i].cx, items[i].cy);
        g.globalAlpha = 1;
      }
      /* Cursor: the sheet's arrow on the OUTER side of the picked item, so
       * it never rides over the other cell. */
      const it = items[ch.sel] || items[0];
      const bob = Math.sin(this.tick * 0.15) * 2;
      const w = this.widgets[it.id];
      const hw = w ? (w.w * w.scale) / 0x1000 / 2 : 40;
      if (ch.sel === 1) this._widget(g, W.ARROW_L, it.cx + hw + 14 + bob, it.cy);
      else this._widget(g, W.ARROW_R, it.cx - hw - 14 - bob, it.cy);
    }

    /* Draw a number in the tally sheet's own coin digit strip (widget 47's
     * cell row: 16x16 digits at u = 88 + d*16 on the GET COIN line). */
    _coinNumber(g, n, x, cy) {
      const w47 = this.widgets[W.COIN_DIGIT];
      if (!w47) return;
      let s = String(Math.abs(Math.trunc(n)));
      for (const chd of s) {
        const d = chd.charCodeAt(0) - 48;
        this._cell(g, w47.page, w47.palette, 88 + d * 16, w47.v, 16, 16, x, cy - 8);
        x += 14;
      }
    }

    /* ---------------- camera orbit (drag only; the default is the fitted
     * side-on framing) ---------------- */
    _attachOrbit() {
      const c = this.glCanvas;
      let drag = false, lx = 0, ly = 0;
      c.addEventListener('pointerdown', (e) => {
        drag = true; lx = e.clientX; ly = e.clientY;
        c.setPointerCapture(e.pointerId);
      });
      c.addEventListener('pointerup', (e) => {
        drag = false; c.releasePointerCapture(e.pointerId);
      });
      c.addEventListener('pointermove', (e) => {
        if (!drag) return;
        this.cam.yaw -= (e.clientX - lx) * 0.006;
        this.cam.pitch = Math.max(-1.2, Math.min(1.2,
          this.cam.pitch - (e.clientY - ly) * 0.006));
        lx = e.clientX; ly = e.clientY;
      });
      c.addEventListener('dblclick', () => {
        this.cam = { yaw: 0.0, pitch: 0.1, distance: 1.7 };
      });
      c.addEventListener('wheel', (e) => {
        e.preventDefault();
        this.cam.distance = Math.max(0.9, Math.min(6,
          this.cam.distance * (e.deltaY > 0 ? 1.1 : 0.9)));
      }, { passive: false });
    }
  }

  /* Pose `base` (object-local verts) through `clip` at `frame` into `out`,
   * then spin the whole figure `yaw` about Y and shift it `dx` / `dz` along
   * X / Z. Exactly the retail per-object composition Rz.Ry.Rx . v + T (see
   * mesh-view.js), with a world transform on top. `vertBase` selects the
   * fighter's slice of the combined buffers. */
  function poseInto(out, base, oids, clip, frame, vertBase, dx, yaw, dz) {
    const pc = clip.parts, f = clip.frames;
    const ff = ((frame % clip.frameCount) + clip.frameCount) % clip.frameCount;
    const sin = new Float32Array(pc * 3), cos = new Float32Array(pc * 3);
    const tr = new Float32Array(pc * 3);
    for (let p = 0; p < pc; p++) {
      const o = (ff * pc + p) * 6;
      for (let k = 0; k < 3; k++) {
        const a = f[o + 3 + k] * A2R;
        sin[p * 3 + k] = Math.sin(a);
        cos[p * 3 + k] = Math.cos(a);
        tr[p * 3 + k] = f[o + k];
      }
    }
    const wy = yaw || 0;
    const wsin = Math.sin(wy), wcos = Math.cos(wy);
    const n = oids.length;
    for (let v = 0; v < n; v++) {
      const vi = (vertBase + v) * 3;
      const o = oids[v];
      let x = base[vi], y = base[vi + 1], z = base[vi + 2];
      if (o < pc) {
        const sx = sin[o * 3], cxx = cos[o * 3];
        const sy = sin[o * 3 + 1], cyy = cos[o * 3 + 1];
        const sz = sin[o * 3 + 2], czz = cos[o * 3 + 2];
        let ny = y * cxx - z * sx, nz = y * sx + z * cxx; y = ny; z = nz;
        let nx = x * cyy + z * sy; nz = -x * sy + z * cyy; x = nx; z = nz;
        nx = x * czz - y * sz; ny = x * sz + y * czz; x = nx; y = ny;
        x += tr[o * 3]; y += tr[o * 3 + 1]; z += tr[o * 3 + 2];
      }
      /* world: yaw about Y, then the floor offset */
      const wx = x * wcos + z * wsin;
      const wz = -x * wsin + z * wcos;
      out[vi] = wx + (dx || 0);
      out[vi + 1] = y;
      out[vi + 2] = wz + (dz || 0);
    }
  }

  window.MinigameBakaView = MinigameBakaView;
})();
