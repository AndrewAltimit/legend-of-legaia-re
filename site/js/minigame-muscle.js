/* Muscle Dome - the 3D card-battle arena, drawn from the visitor's disc.
 *
 * Two layers over one <div> (the same template as the dance / Baka panels):
 *   - a WebGL canvas (the shared TmdRenderer R16UI paletted-VRAM pipeline)
 *     carrying the ARENA SCENE: the Sol arena backdrop (PROT 1225 - the
 *     scene_tmd_stream tail slot of the dome's own `other6.lzs` file, the
 *     fenced dirt ring the retail contest is fought in) plus the retail
 *     battle ground grid (the func_0x801d02c0 flat tiled plane, sampling
 *     the backdrop's own (832,0) page window through CLUT (0,479)); over
 *     it the player's battle-form party mesh (PROT 1204, posed from the
 *     PROT 1203 battle-form anim bank) versus a monster of the PROT 867
 *     archive - its own embedded TMD, texture pool relocated to battle
 *     texture slot 0 exactly as the retail battle loader does
 *     (FUN_80055468 via `battle_render_mesh`), posed from its own
 *     rigid-part keyframe animations (docs/formats/monster-animation.md);
 *   - a 2D canvas carrying the HUD text overlays: the round banner, HP +
 *     Spirit bars, damage numbers, the round time meter, the between-round
 *     interval panel and the verdict banners.
 *
 * SOUND: the dome's own cue set, decoded from the disc's SFX banks - the
 * match SM's UI blips (static rows 0x20..0x22, PROT 0868) and the shared
 * battle/duel melee-impact cue (row 0x09, PROT 0869); the BGM (the battle
 * theme the arena inherits) is the page-level MgBgm hook.
 *
 * The RULES are `legaia-engine-core::muscle_dome` + the ported battle
 * formulas, reached through `LegaiaMinigames` (crates/web-viewer/src/
 * minigames_muscle.rs): every committed card resolves through the real
 * arts/physical damage roll (FUN_801dd0ac), the element-affinity scale
 * (FUN_801dd864) and the damage finisher (FUN_801ddb30), against fighter
 * stats read off the disc's own records - the monster's PROT 867 stat block
 * and the player's SCUS new-game template leveled through the growth curves.
 * This file is presentation only; it never computes a damage number itself.
 *
 * Traced vs fitted, stated plainly: the deal, budget gate, action queue,
 * score readout, damage rolls, spirit accrual, the arena backdrop + ground
 * grid texture address, the time-meter ramp and the cue id set are the
 * disc's own tables + traced constants; the CAMERA, fighter spacing/facing,
 * the HUD text layout and the per-event cue assignment are fitted, and the
 * panel's note says so. The card->animation pairing is an approximation
 * over the battle-form bank's attack records.
 *
 * Requires webgl-math.js + webgl-shaders.js + webgl-tmd.js first.
 */
window.MgMuscle = (function () {
  'use strict';

  const A2R = (Math.PI * 2) / 4096;   /* PSX angle units -> radians */
  const HUD_W = 320, HUD_H = 240;     /* retail frame; canvas is 2x */

  /* The four swing-card command ids and their directions - the runtime
   * action-constant space (crates/art queue.rs: 0x0C Left, 0x0D Right,
   * 0x0E Down, 0x0F Up). */
  const CMD = {
    12: { name: 'Left',  glyph: '←' },
    13: { name: 'Right', glyph: '→' },
    14: { name: 'Down',  glyph: '↓' },
    15: { name: 'Up',    glyph: '↑' },
  };

  /* Player battle-form anim bank slots (PROT 1203, 9 records/char: 0 idle,
   * 1..3 attacks, 4 special, 5 hit - see minigame-baka.js). The card ->
   * record pairing is an APPROXIMATION: the true swing clips live in the
   * player battle files' ME archives, which this page does not decode. */
  const P_ANIM = { IDLE: 0, HIT: 5, BY_CMD: { 12: 1, 13: 2, 14: 3, 15: 1 } };

  /* Monster action tags (docs/formats/monster-animation.md): 0 idle, 2/3
   * light hit reactions, 4 knockdown, 0x20 pre-approach / 0x21 close-in
   * attacks. */
  function pickMonsterClips(anims) {
    const byTag = (t) => anims.findIndex(a => a.action_id === t);
    const idle = byTag(0);
    let attack = byTag(0x21);
    if (attack < 0) attack = byTag(0x20);
    if (attack < 0) attack = anims.findIndex(a => a.action_id >= 0x20);
    if (attack < 0) attack = anims.length > 1 ? 1 : idle;
    let hit = byTag(2);
    if (hit < 0) hit = byTag(3);
    let ko = byTag(4);
    if (ko < 0) ko = hit;
    return { idle: Math.max(idle, 0), attack, hit: hit < 0 ? idle : hit, ko };
  }

  function create(api, hudCanvas, glCanvas) {
    const g = hudCanvas.getContext('2d');
    g.imageSmoothingEnabled = false;

    let scene = null;          /* 3D scene (null = text fallback) */
    let sceneMonster = -1;     /* monster the scene was built for */
    let mode = 'idle';         /* idle|select|playback|interval|decided */
    let tick = 0;
    let banner = null;         /* {text, sub, t, life, cls} */
    let popups = [];           /* {text, x, y, t, life, color} */
    let playQueue = [];        /* remaining round-log events */
    let playT = 0;             /* ticks into the current event */
    let hpShow = [0, 0];       /* eased HP bar values */
    let lastOpts = null;       /* {char, level, monster} for restart */
    let roster = null;         /* muscle_roster_json rows */
    let meter = 0;             /* round time meter 0..0xC (FUN_801d3444) */

    /* ------------------------------------------------ roster + spell names */

    function loadRoster() {
      if (!roster) {
        try { roster = JSON.parse(api.muscle_roster_json()); }
        catch (e) { roster = []; }
      }
      return roster;
    }

    function st() {
      try { return JSON.parse(api.muscle_state_json()); }
      catch (e) { return { live: false }; }
    }

    /* --------------------------------------------------- 3D scene assembly */

    /* Pose `base` through `clip` at `frame` into `out` - the shared retail
     * per-object composition Rz.Ry.Rx . v + T with a world yaw + offset on
     * top (identical to minigame-baka.js / minigame-dance.js). */
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
      const wsin = Math.sin(yaw || 0), wcos = Math.cos(yaw || 0);
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
        const wx = x * wcos + z * wsin;
        const wz = -x * wsin + z * wcos;
        out[vi] = wx + (dx || 0);
        out[vi + 1] = y;
        out[vi + 2] = wz + (dz || 0);
      }
    }

    /* Half-extent + height of a rest pose, for spacing / camera framing. */
    function poseExtent(f, clip) {
      const out = new Float32Array(f.pos);
      poseInto(out, f.pos, f.oid, clip, 0, 0, 0, 0, 0);
      let lo = Infinity, hi = -Infinity, top = 0;
      for (let i = 0; i < out.length; i += 3) {
        if (out[i] < lo) lo = out[i];
        if (out[i] > hi) hi = out[i];
        if (-out[i + 1] > top) top = -out[i + 1];   /* Y-down: up = -y */
      }
      return { half: (hi - lo) / 2 || 200, height: top || 400 };
    }

    /* Fallback plain-quad floor: alternating dark tiles on y = 0. Used only
     * when the arena backdrop entry (PROT 1225) doesn't decode on this image
     * (flat-coloured geometry, no invented texture art). */
    function floorBuffers(extent) {
      const out = { pos: [], uvs: [], ct: [], flat: [], idx: [] };
      const T = Math.max(160, Math.round(extent / 4));
      const N = 12;
      for (let iz = -N; iz < N; iz++) {
        for (let ix = -N; ix < N; ix++) {
          const dark = ((ix + iz) & 1) === 0;
          const c = dark ? [34, 36, 44] : [48, 52, 62];
          const base = out.pos.length / 3;
          const x0 = ix * T, x1 = x0 + T, z0 = iz * T, z1 = z0 + T;
          out.pos.push(x0, 0, z0, x1, 0, z0, x1, 0, z1, x0, 0, z1);
          for (let k = 0; k < 4; k++) {
            out.uvs.push(0, 0);
            out.ct.push(0, 0);
            out.flat.push(c[0], c[1], c[2], 0);   /* flag 0 = flat colour */
          }
          out.idx.push(base, base + 1, base + 2, base, base + 2, base + 3);
        }
      }
      return out;
    }

    /* The Sol arena backdrop (PROT 1225): the fenced dirt ring's own TMD,
     * world-fixed at raw coordinates exactly as the retail battle renderer
     * draws a stage dome (one instance, no mirror - docs/subsystems/
     * battle.md). Its texture pages ride in muscle_vram. Null when the entry
     * doesn't decode. */
    function arenaBuffers() {
      if (!api.muscle_arena_positions) return null;
      const pos = api.muscle_arena_positions();
      if (!pos.length) return null;
      return {
        pos,
        uvs: api.muscle_arena_uvs(),
        ct: api.muscle_arena_cba_tsb(),
        flat: api.muscle_arena_flat_rgba(),
        idx: api.muscle_arena_indices(),
      };
    }

    /* The retail battle ground grid (func_0x801d02c0, battle overlay): a flat
     * tiled plane on y = 0 centred at the world origin. Traced constants
     * (docs/subsystems/battle.md "Backdrop ground"): cell pitch 0x200 with
     * each cell emitted as FOUR quads (2x2 sub-step 0x100), texture = the
     * 4bpp page at framebuffer (832, 0) (tpage attr 0x000D) through CLUT
     * (0, 479) (CBA 0x77C0), UV window (192..255)^2 stretched across one
     * cell - deterministic sub-tiling, no RNG. The live capture reads the
     * grid as 28x28 cells; the page emits the same. */
    function groundBuffers() {
      const out = { pos: [], uvs: [], ct: [], flat: [], idx: [] };
      const CELL = 0x200, SUB = 0x100, N = 14;   /* 28x28 cells */
      const CBA = 0x77C0, TSB = 0x000D;
      for (let cz = -N; cz < N; cz++) {
        for (let cx = -N; cx < N; cx++) {
          for (let sr = 0; sr < 2; sr++) {
            for (let sc = 0; sc < 2; sc++) {
              const x0 = cx * CELL + sc * SUB, x1 = x0 + SUB;
              const z0 = cz * CELL + sr * SUB, z1 = z0 + SUB;
              const u0 = 192 + sc * 32, u1 = u0 + 31;
              const v0 = 192 + sr * 32, v1 = v0 + 31;
              const base = out.pos.length / 3;
              out.pos.push(x0, 0, z0, x1, 0, z0, x1, 0, z1, x0, 0, z1);
              out.uvs.push(u0, v0, u1, v0, u1, v1, u0, v1);
              for (let k = 0; k < 4; k++) {
                out.ct.push(CBA, TSB);
                out.flat.push(255, 255, 255, 255);   /* textured */
              }
              out.idx.push(base, base + 1, base + 2, base, base + 2, base + 3);
            }
          }
        }
      }
      return out;
    }

    /* Build the arena for (charSlot, monsterId). Returns null when either
     * body doesn't decode - the panel then keeps its text presentation. */
    function buildScene(charSlot, monsterId) {
      if (!glCanvas || !window.TmdRenderer) return null;
      if (!api.muscle_scene_ready || !api.muscle_scene_ready(monsterId)) return null;

      const P = {
        pos: api.baka_fighter_positions(0, charSlot),
        uvs: api.baka_fighter_uvs(0, charSlot),
        ct: api.baka_fighter_cba_tsb(0, charSlot),
        idx: api.baka_fighter_indices(0, charSlot),
        oid: api.baka_fighter_object_ids(0, charSlot),
        flat: api.baka_fighter_flat_rgba(0, charSlot),
        parts: api.baka_fighter_part_count(0, charSlot),
      };
      const M = {
        pos: api.muscle_monster_positions(monsterId),
        uvs: api.muscle_monster_uvs(monsterId),
        ct: api.muscle_monster_cba_tsb(monsterId),
        idx: api.muscle_monster_indices(monsterId),
        oid: api.muscle_monster_object_ids(monsterId),
        flat: api.muscle_monster_flat_rgba(monsterId),
        parts: api.muscle_monster_part_count(monsterId),
      };
      if (!P.pos.length || !M.pos.length) return null;

      /* Player clips out of the battle-form bank. */
      const pClip = (action) => {
        const dims = api.baka_anim_dims(0, charSlot, action);
        if (!dims[0] || !dims[1]) return null;
        const frames = api.baka_anim_pose_frames(0, charSlot, action, P.parts);
        if (!frames.length) return null;
        return { frames, frameCount: dims[1], parts: P.parts, rate: 4 };
      };
      /* Monster clips out of its own action set (rate is the retail cursor
       * byte: rate/8 keyframes per tick with the normal x4 scale). */
      const mAnims = JSON.parse(api.muscle_monster_anims_json(monsterId));
      const mPick = pickMonsterClips(mAnims);
      const mClip = (index) => {
        if (index < 0 || index >= mAnims.length) return null;
        const a = mAnims[index];
        const frames = api.muscle_monster_pose_frames(monsterId, index, M.parts);
        if (!frames.length) return null;
        return {
          frames, frameCount: a.frame_count, parts: M.parts,
          rate: Math.max(1, (a.rate || 1) * 2),
        };
      };
      const clips = [
        { idle: pClip(P_ANIM.IDLE), hit: pClip(P_ANIM.HIT),
          byCmd: Object.fromEntries(Object.entries(P_ANIM.BY_CMD)
            .map(([c, r]) => [c, pClip(r)])) },
        { idle: mClip(mPick.idle), hit: mClip(mPick.hit),
          attack: mClip(mPick.attack), ko: mClip(mPick.ko) },
      ];
      if (!clips[0].idle || !clips[1].idle) return null;

      const extP = poseExtent(P, clips[0].idle);
      const extM = poseExtent(M, clips[1].idle);
      const gap = (extP.half + extM.half) * 1.5 + 120;

      /* Static geometry behind the fighters: the real arena backdrop + the
       * retail ground grid when PROT 1225 decodes, the flat fallback floor
       * otherwise. */
      const arena = arenaBuffers();
      const statics = arena ? [arena, groundBuffers()] : [floorBuffers(gap)];

      /* Combined buffers: player, monster, then the static set. */
      const nP = P.pos.length / 3, nM = M.pos.length / 3;
      let n = nP + nM;
      for (const st2 of statics) n += st2.pos.length / 3;
      const pos = new Float32Array(n * 3);
      const uvs = new Uint8Array(n * 2);
      const ct = new Uint16Array(n * 2);
      const flat = new Uint8Array(n * 4);
      const idx = [];
      pos.set(P.pos, 0); pos.set(M.pos, nP * 3);
      uvs.set(P.uvs, 0); uvs.set(M.uvs, nP * 2);
      ct.set(P.ct, 0); ct.set(M.ct, nP * 2);
      flat.set(P.flat, 0); flat.set(M.flat, nP * 4);
      for (const i of P.idx) idx.push(i);
      for (const i of M.idx) idx.push(i + nP);
      let at = nP + nM;
      for (const st2 of statics) {
        pos.set(st2.pos, at * 3);
        uvs.set(st2.uvs, at * 2);
        ct.set(st2.ct, at * 2);
        flat.set(st2.flat, at * 4);
        for (const i of st2.idx) idx.push(i + at);
        at += st2.pos.length / 3;
      }

      const renderer = new window.TmdRenderer(glCanvas);
      renderer.uploadVram(api.muscle_vram(monsterId));
      renderer.uploadMesh(pos, uvs, ct, new Uint32Array(idx), flat);

      /* With the real arena up, the shell is authored at X >= 0 with the
       * open side facing -X (the town01 half-stage rule) and the fighters
       * seat near the world origin; spread them across Z so the default
       * camera - parked on the open side, looking into the shell - sees
       * them side by side. Without the arena, keep the old X spread. The
       * exact seats + camera remain FITTED, as the note says. */
      const spreadZ = !!arena;
      const s = {
        renderer, P, M, nP, nM,
        clips,
        base: pos.slice(),
        out: pos,
        /* Fighter world placement: the families' intrinsic facing needs
         * opposite world yaws (the Baka finding). */
        dx: spreadZ ? [0, 0] : [-gap / 2, gap / 2],
        dz: spreadZ ? [-gap / 2, gap / 2] : [0, 0],
        yaw: spreadZ ? [0, Math.PI] : [Math.PI / 2, -Math.PI / 2],
        /* Per-fighter clip state: {clip, start, loop, hold} */
        act: [
          { clip: clips[0].idle, start: 0, loop: true },
          { clip: clips[1].idle, start: 0, loop: true },
        ],
        cam: {
          yaw: spreadZ ? Math.PI / 2 : 0.0,
          pitch: 0.14,
          distance: spreadZ ? 2.1 : 1.75,
        },
        defCam: {
          yaw: spreadZ ? Math.PI / 2 : 0.0,
          pitch: 0.14,
          distance: spreadZ ? 2.1 : 1.75,
        },
        center: [spreadZ ? 260 : 0,
          -Math.max(extP.height, extM.height) * 0.42, 0],
        radius: gap * 0.95 + Math.max(extP.half, extM.half) * 0.6,
      };
      attachOrbit(s);
      return s;
    }

    function attachOrbit(s) {
      const c = glCanvas;
      let drag = false, lx = 0, ly = 0;
      c.addEventListener('pointerdown', (e) => {
        drag = true; lx = e.clientX; ly = e.clientY;
        c.setPointerCapture(e.pointerId);
      });
      c.addEventListener('pointerup', (e) => {
        drag = false; try { c.releasePointerCapture(e.pointerId); } catch (_) { /* */ }
      });
      c.addEventListener('pointermove', (e) => {
        if (!drag) return;
        s.cam.yaw -= (e.clientX - lx) * 0.006;
        s.cam.pitch = Math.max(-1.1, Math.min(1.1,
          s.cam.pitch - (e.clientY - ly) * 0.006));
        lx = e.clientX; ly = e.clientY;
      });
      c.addEventListener('dblclick', () => { s.cam = Object.assign({}, s.defCam); });
      c.addEventListener('wheel', (e) => {
        e.preventDefault();
        s.cam.distance = Math.max(0.9, Math.min(6,
          s.cam.distance * (e.deltaY > 0 ? 1.1 : 0.9)));
      }, { passive: false });
    }

    /* ---------------- sound ----------------
     *
     * The dome's own cue set, decoded once from the visitor's disc through
     * the WASM API (crates/web-viewer/src/minigames_muscle.rs):
     *   - the match SM's UI blips: FUN_801d0748 fires 34 immediate
     *     FUN_8004fcc8(0x21/0x22/0x23) calls, whose < 0x40 leg enqueues
     *     id-1 - static descriptor rows 0x20/0x21/0x22, category 0 ->
     *     the PROT 0868 system bank;
     *   - the melee impact: the shared battle/duel bank's row 0x09
     *     (category 2 -> PROT 0869), the hit cue of the shared battle
     *     path the dome resolves its card plays through.
     * The id set is traced; WHICH blip fires on which page event is a
     * fitted assignment (the 34 sites spread across phase arms this page
     * does not reproduce one-to-one), and the note says so. */
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
      gn.gain.value = gain;
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

    /* Trigger a one-shot clip on fighter `fi` (idle resumes after; `hold`
     * freezes the final frame - the loser's knockdown). */
    function play(fi, clip, hold) {
      if (!scene || !clip) return;
      scene.act[fi] = { clip, start: tick, loop: false, hold: !!hold };
    }

    function renderScene() {
      const s = scene;
      if (!s) return;
      for (let fi = 0; fi < 2; fi++) {
        const a = s.act[fi];
        let clip = a.clip || s.clips[fi].idle;
        let frame;
        if (a.loop) {
          frame = Math.floor((tick - a.start) * clip.rate / 16) % clip.frameCount;
        } else {
          frame = Math.floor((tick - a.start) * clip.rate / 16);
          if (frame >= clip.frameCount) {
            if (a.hold) {
              frame = clip.frameCount - 1;
            } else {
              s.act[fi] = { clip: s.clips[fi].idle, start: tick, loop: true };
              clip = s.clips[fi].idle;
              frame = 0;
            }
          }
        }
        const f = fi === 0 ? s.P : s.M;
        poseInto(s.out, s.base, f.oid, clip, frame,
          fi === 0 ? 0 : s.nP, s.dx[fi], s.yaw[fi], s.dz[fi]);
      }
      s.renderer.updatePositions(s.out);
      s.renderer.render(s.cam.yaw, s.cam.pitch, s.cam.distance,
        0, 0, s.center, s.radius);
    }

    /* -------------------------------------------------------- contest flow */

    /* Start a contest. opts = {char, level, monster}; the RNG seed is drawn
     * fresh per contest so replays differ (pass opts.seed to pin one). */
    function start(opts) {
      lastOpts = Object.assign({ char: 0, level: 30, monster: 0 }, opts || {});
      let monster = lastOpts.monster | 0;
      if (!monster) {
        const r = loadRoster();
        monster = r.length ? r[0].id : 1;
        lastOpts.monster = monster;
      }
      const seed = (lastOpts.seed != null ? lastOpts.seed
        : (Date.now() & 0x7fffffff)) >>> 0;
      if (!api.muscle_start_vs(lastOpts.char, lastOpts.level, monster, seed)) {
        return false;
      }
      if (sceneMonster !== monster || !scene ||
          scene.charSlot !== lastOpts.char) {
        try { scene = buildScene(lastOpts.char, monster); }
        catch (e) { scene = null; }
        if (scene) { scene.charSlot = lastOpts.char; }
        sceneMonster = monster;
      } else {
        scene.act = [
          { clip: scene.clips[0].idle, start: tick, loop: true },
          { clip: scene.clips[1].idle, start: tick, loop: true },
        ];
      }
      const state = st();
      hpShow = [state.hp[0], state.hp[1]];
      popups = [];
      playQueue = [];
      mode = 'select';
      setBanner('ROUND 1', 'commit cards, SPACE to fight', 90);
      return true;
    }

    function commit(slot) {
      if (mode !== 'select') return false;
      const ok = api.muscle_commit(slot);
      /* Confirm blip on a committed card; cursor blip on a rejected one
       * (overspend / queue full) - fitted assignment over the traced ids. */
      playCue(ok ? 'confirm' : 'cursor', ok ? 0.5 : 0.35);
      return ok;
    }

    /* SPACE / Confirm: advances whatever the current presentation mode is. */
    function confirm() {
      const state = st();
      if (!state.live) { if (lastOpts) start(lastOpts); return; }
      if (mode === 'select') {
        api.muscle_end_selection();
        api.muscle_resolve();
        playQueue = JSON.parse(api.muscle_round_log_json());
        playT = 0;
        banner = null;
        mode = 'playback';
      } else if (mode === 'playback') {
        /* Skip: settle every pending event instantly. */
        while (playQueue.length) applyEvent(playQueue.shift(), true);
        finishPlayback();
      } else if (mode === 'interval') {
        api.muscle_next_round();
        const s2 = st();
        if (s2.phase === 'select') {
          mode = 'select';
          setBanner('ROUND ' + (s2.round + 1), 'commit cards, SPACE to fight', 90);
        }
      } else if (mode === 'decided') {
        if (lastOpts) start(lastOpts);
      }
    }

    function setBanner(text, sub, life, cls) {
      banner = { text, sub, t: 0, life: life || 75, cls: cls || '' };
      /* Phase-advance blip (fitted assignment over the traced id set). */
      playCue('blip', 0.35);
    }

    /* One play event lands: animations + popup + HP target. */
    function applyEvent(ev, instant) {
      const defender = ev.attacker ^ 1;
      if (!instant && scene) {
        if (ev.attacker === 0) {
          play(0, scene.clips[0].byCmd[ev.cmd] || scene.clips[0].byCmd[12]);
        } else {
          play(1, scene.clips[1].attack);
        }
      }
      hpShow[defender] = ev.hp[defender];
      if (!instant) {
        const x = defender === 0 ? 88 : 232;
        popups.push({
          text: '-' + ev.damage, x, y: 92, t: 0, life: 46,
          color: defender === 0 ? '#ff9d9d' : '#ffe9a8',
        });
      }
    }

    function finishPlayback() {
      const state = st();
      if (state.phase === 'round_over') {
        mode = 'interval';
      } else if (state.phase === 'won' || state.phase === 'lost') {
        mode = 'decided';
        if (scene) {
          const loser = state.phase === 'won' ? 1 : 0;
          play(loser, loser === 1 ? scene.clips[1].ko : scene.clips[0].hit, true);
        }
        if (state.phase === 'won') {
          /* Retail victory banner wording (FUN_801d8de8 case 0x59 composes
           * "...acquired the power of..." + the reward spell name out of the
           * shared spell-name table). */
          const spell = api.muscle_spell_name ? api.muscle_spell_name(state.reward_spell) : '';
          setBanner('YOU WIN!', spell
            ? state.names[0] + ' acquired the power of ' + spell + '! — SPACE for a rematch'
            : 'SPACE for a rematch', 100000, 'good');
        } else {
          setBanner('YOU LOSE', 'SPACE for a rematch', 100000, 'bad');
        }
      } else {
        mode = 'select';
      }
    }

    /* ------------------------------------------------------------ HUD draw */

    function bar(x, y, w, h, frac, col, back) {
      g.fillStyle = back || 'rgba(0,0,0,0.55)';
      g.fillRect(x * 2, y * 2, w * 2, h * 2);
      g.fillStyle = col;
      g.fillRect(x * 2, y * 2, Math.max(0, Math.min(1, frac)) * w * 2, h * 2);
      g.strokeStyle = 'rgba(255,255,255,0.35)';
      g.strokeRect(x * 2 + 0.5, y * 2 + 0.5, w * 2, h * 2);
    }

    function text(s, x, y, size, col, align, boldness) {
      g.font = (boldness || 'bold ') + (size * 2) + 'px ui-monospace, monospace';
      g.textAlign = align || 'left';
      g.textBaseline = 'middle';
      g.fillStyle = 'rgba(0,0,0,0.65)';
      g.fillText(s, x * 2 + 2, y * 2 + 2);
      g.fillStyle = col || '#e8ecf2';
      g.fillText(s, x * 2, y * 2);
    }

    function drawFighterPlates(state) {
      const plates = [
        { x: 8, name: state.names[0], hp: hpShow[0], max: state.hp_max[0],
          spirit: state.spirit[0], col: '#2dcca7' },
        { x: 168, name: state.names[1], hp: hpShow[1], max: state.hp_max[1],
          spirit: state.spirit[1], col: '#d84b4b' },
      ];
      for (const p of plates) {
        g.fillStyle = 'rgba(6,8,12,0.62)';
        g.fillRect(p.x * 2, 8 * 2, 144 * 2, 34 * 2);
        text(p.name, p.x + 4, 15, 8, '#e8ecf2');
        bar(p.x + 4, 21, 136, 5, p.max ? p.hp / p.max : 0, p.col);
        text('HP ' + Math.max(0, Math.round(p.hp)) + '/' + p.max,
          p.x + 4, 31, 6, '#aeb6c4', 'left', '');
        /* The per-fighter Spirit gauge (actor+0x170) - the value the dome
         * HUD's own bar elements display (FUN_801d8de8 elems 0x52/0x53),
         * headed by the retail "Spirit" string (elem 0x0B). */
        bar(p.x + 74, 29, 66, 4, p.spirit / 100, '#7798d4');
        text('Spirit', p.x + 71, 31, 6, '#7798d4', 'right', '');
      }
    }

    /* The round TIME METER (FUN_801d3444): a 0..0xC counter that ramps while
     * the commit/playback phase (`ctx+6 == 0x50`) runs and drains otherwise,
     * mapped to a 160-px bar (`counter * 160 / 12`). The ramp + mapping are
     * traced (port `engine-core::muscle_dome::time_meter_step`); the screen
     * placement is fitted. */
    function drawTimeMeter() {
      const hFull = 160;
      const hh = Math.round(meter * hFull / 12);
      const x = 306, yBot = 196;
      g.fillStyle = 'rgba(0,0,0,0.55)';
      g.fillRect(x * 2, (yBot - hFull) * 2, 6 * 2, hFull * 2);
      g.fillStyle = '#ffd166';
      g.fillRect(x * 2, (yBot - hh) * 2, 6 * 2, hh * 2);
      g.strokeStyle = 'rgba(255,255,255,0.35)';
      g.strokeRect(x * 2 + 0.5, (yBot - hFull) * 2 + 0.5, 6 * 2, hFull * 2);
      text('TIME', x + 3, yBot + 7, 6, '#aeb6c4', 'center', '');
    }

    function drawSelectHud(state) {
      const budget = state.budget[0];
      const pool = state.stats ? state.stats[0].budget_pool : budget;
      text('BUDGET', 8, 206, 7, '#aeb6c4');
      bar(48, 203, 120, 6, pool ? budget / pool : 0, '#ffd166');
      text(budget + ' / ' + pool, 172, 206, 7, '#e8ecf2');
      /* Committed queue pips (the actor +0x1df action queue). */
      const q = state.queue[0];
      let s = '';
      for (const cmd of q) s += (CMD[cmd] ? CMD[cmd].glyph : '?') + ' ';
      text('QUEUE ' + (s || '—'), 8, 220, 7, '#e8ecf2');
      text('1-4 commit · SPACE fight', 312, 220, 6, '#aeb6c4', 'right', '');
    }

    function drawInterval(state) {
      g.fillStyle = 'rgba(4,6,10,0.82)';
      g.fillRect(30 * 2, 52 * 2, 260 * 2, 136 * 2);
      g.strokeStyle = 'rgba(255,255,255,0.25)';
      g.strokeRect(30 * 2 + 0.5, 52 * 2 + 0.5, 260 * 2, 136 * 2);
      text('ROUND ' + (state.round + 1) + ' SETTLED', 160, 66, 10, '#ffd166', 'center');
      /* The retail score readout: hp * 0x6c / max (FUN_801d0748 phase 0x6e),
       * rendered per fighter out of 108. */
      text('score  ' + state.names[0] + ' ' + state.score[0] + '/108' +
        '   ·   ' + state.names[1] + ' ' + state.score[1] + '/108',
        160, 84, 7, '#e8ecf2', 'center', '');
      text('damage taken   you ' + state.last_damage[0] +
        '  ·  foe ' + state.last_damage[1], 160, 100, 7, '#e8ecf2', 'center', '');
      /* Spirit recovered this contest - the +0x170 gauge each hit fills
       * (spirit_gauge_fill); the interval framing itself is approximated. */
      text('spirit gauge   you ' + state.spirit[0] + '/100' +
        '  ·  foe ' + state.spirit[1] + '/100', 160, 116, 7, '#7798d4', 'center', '');
      const q0 = state.queue[0].length, q1 = state.queue[1].length;
      text('cards played   you ' + q0 + '  ·  foe ' + q1,
        160, 132, 7, '#aeb6c4', 'center', '');
      text('budget reseeds from your AGL pool next round',
        160, 152, 6, '#aeb6c4', 'center', '');
      text('SPACE: next round', 160, 172, 8, '#2dcca7', 'center');
    }

    function drawBanner() {
      if (!banner) return;
      if (banner.t > banner.life) { banner = null; return; }
      const a = banner.t < 8 ? banner.t / 8
        : banner.t > banner.life - 12 ? (banner.life - banner.t) / 12 : 1;
      g.save();
      g.globalAlpha = Math.max(0, Math.min(1, a));
      const col = banner.cls === 'good' ? '#2dcca7'
        : banner.cls === 'bad' ? '#d84b4b' : '#ffd166';
      text(banner.text, 160, 108, 16, col, 'center');
      if (banner.sub) text(banner.sub, 160, 126, 7, '#e8ecf2', 'center', '');
      g.restore();
      banner.t++;
    }

    function drawPopups() {
      popups = popups.filter(p => p.t < p.life);
      for (const p of popups) {
        const rise = Math.min(p.t, 20) * 0.8;
        g.save();
        g.globalAlpha = p.t > p.life - 12 ? (p.life - p.t) / 12 : 1;
        text(p.text, p.x, p.y - rise, 12, p.color, 'center');
        g.restore();
        p.t++;
      }
    }

    /* ------------------------------------------------------- per-frame tick */

    function frame() {
      tick++;
      const state = st();

      /* Playback: land one event every 34 ticks (attacker swing, then the
       * hit + number as it connects). */
      if (mode === 'playback') {
        if (playQueue.length) {
          if (playT === 0) {
            applyEvent(playQueue[0], false);
            /* Defender hit reaction + the impact cue fire as the swing
             * lands (the cue rides the same 12-tick connect delay). */
            const ev = playQueue[0];
            const defender = ev.attacker ^ 1;
            const hitClip = scene
              ? (defender === 0 ? scene.clips[0].hit : scene.clips[1].hit)
              : null;
            setTimeoutTick(12, () => {
              if (mode !== 'playback') return;
              if (hitClip) play(defender, hitClip);
              playHit();
            });
          }
          playT++;
          if (playT >= 34) { playQueue.shift(); playT = 0; }
        } else {
          finishPlayback();
        }
      }
      runTickTimers();

      /* Round time meter: ramp while the round is playing out, drain
       * otherwise (the FUN_801d3444 shape, one step per tick). */
      meter = mode === 'playback'
        ? Math.min(12, meter + 1)
        : Math.max(0, meter - 1);

      /* Ease the HP bars toward their targets. */
      /* (targets are set per landed event; outside playback follow state) */
      if (mode !== 'playback' && state.live) {
        hpShow[0] += (state.hp[0] - hpShow[0]) * 0.3;
        hpShow[1] += (state.hp[1] - hpShow[1]) * 0.3;
      }

      /* 3D under, HUD over. */
      if (scene) {
        renderScene();
        g.clearRect(0, 0, hudCanvas.width, hudCanvas.height);
      } else {
        g.fillStyle = '#0b0b10';
        g.fillRect(0, 0, hudCanvas.width, hudCanvas.height);
        g.fillStyle = '#11131c';
        g.fillRect(0, 150 * 2, hudCanvas.width, hudCanvas.height - 150 * 2);
        if (state.live) {
          text('3D bodies unavailable on this image — text HUD only',
            160, 160, 7, '#aeb6c4', 'center', '');
        }
      }
      if (!state.live) { drawBanner(); return; }

      drawFighterPlates(state);
      if (meter > 0) drawTimeMeter();
      if (mode === 'select') drawSelectHud(state);
      if (mode === 'interval') drawInterval(state);
      drawPopups();
      drawBanner();
    }

    /* Tiny tick-based timer queue (the page has no per-event rAF hooks). */
    let timers = [];
    function setTimeoutTick(dt, fn) { timers.push({ at: tick + dt, fn }); }
    function runTickTimers() {
      const due = timers.filter(t => t.at <= tick);
      timers = timers.filter(t => t.at > tick);
      for (const t of due) t.fn();
    }

    return {
      loadRoster, start, commit, confirm, frame,
      state: st,
      mode: () => mode,
      sceneOk: () => !!scene,
      arenaOk: () => {
        try { return !!JSON.parse(api.muscle_arena_json()).ok; }
        catch (e) { return false; }
      },
      sfxOk: () => {
        try { return !!JSON.parse(api.muscle_sfx_json()).ok; }
        catch (e) { return false; }
      },
      camInfo: () => scene
        ? { cam: Object.assign({}, scene.cam), center: scene.center.slice(),
            radius: scene.radius }
        : null,
      setCam: (c) => { if (scene && c) Object.assign(scene.cam, c); },
    };
  }

  return { create, CMD };
})();
