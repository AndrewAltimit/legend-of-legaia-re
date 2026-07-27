/* Muscle Dome - the arena contest, drawn from the visitor's disc.
 *
 * Retail presents the Muscle Dome as a STANDARD BATTLE - the normal Legaia
 * battle chrome with three course restrictions (no equipment, no items;
 * magic allowed on Beginner/Expert) - and that is what this panel draws
 * (capture-verified against retail: the black "Welcome to the Muscle Dome!"
 * intro card, the command cluster with the Item chip crossed out, the
 * name/HP/MP plate + AP gauge plate, and the "HYPER ARTS!!" banner).
 *
 * Two layers over one <div> (the same template as the dance / Baka panels):
 *   - a WebGL canvas (the shared TmdRenderer R16UI paletted-VRAM pipeline)
 *     carrying the ARENA SCENE: the Sol arena backdrop (PROT 1225 - the
 *     scene_tmd_stream tail slot of the dome's own `other6.lzs` file, the
 *     fenced dirt ring the retail contest is fought in) plus the retail
 *     battle ground grid (the func_0x801d02c0 flat tiled plane, sampling
 *     the backdrop's own (832,0) page window through CLUT (0,479)); over
 *     it the player's ASSEMBLED BATTLE FORM - retail fields the party's
 *     normal fighter forms here, not the Baka pack: the player battle
 *     file's equipment-id sections assembled + band-0 relocated
 *     (legaia_asset::battle_char_assembly, `muscle_fighter_*`), posed from
 *     the file's own record[0] action streams and per-command swing
 *     records - versus a monster of the PROT 867 archive, its texture pool
 *     relocated to battle texture slot 0 exactly as the retail battle
 *     loader does (FUN_80055468 via `battle_render_mesh`), posed from its
 *     own rigid-part keyframes (docs/formats/monster-animation.md);
 *   - a 2D canvas carrying the battle chrome: the intro card, the command
 *     cluster (Begin + name chips, Item crossed out, Attack / D-pad /
 *     Ra-Seru / Spirit), the name/HP/MP plate, the AP plate, the arts
 *     banner with its speed-lines, damage numbers, the round time meter,
 *     the between-round interval panel and the verdict banners.
 *
 * SOUND: the dome's own cue set, decoded from the disc's SFX banks - the
 * match SM's UI blips (static rows 0x20..0x22, PROT 0868) and the shared
 * battle/duel melee-impact cue (row 0x09, PROT 0869); the BGM (the battle
 * theme the arena inherits) is the page-level MgBgm hook.
 *
 * The RULES are `legaia-engine-core::muscle_dome` + the ported battle
 * formulas, reached through `LegaiaMinigames` (crates/web-viewer/src/
 * minigames_muscle.rs): every committed command resolves through the real
 * arts/physical damage roll (FUN_801dd0ac), the element-affinity scale
 * (FUN_801dd864) and the damage finisher (FUN_801ddb30), against fighter
 * stats read off the disc's own records - the monster's PROT 867 stat block
 * and the player's SCUS new-game template leveled through the growth curves.
 * This file is presentation only; it never computes a damage number itself.
 *
 * Traced vs fitted, stated plainly. TRACED (disc tables + captures): the
 * deal, budget gate, action queue, damage rolls, spirit accrual, the arena
 * backdrop + ground grid texture address, the ABE additive lamp glows (the
 * object-1 dust decal is omitted - the retail match capture shows a
 * mist-free interior; see docs/subsystems/minigame-muscle-dome.md), the
 * time-meter ramp, the idle-phase camera spin rate, the cue id set, the
 * command -> swing-clip pairing (the four card ids 0xC..0xF ARE the swing
 * record slots of the player battle file - the disc's own pairing), the
 * flinch clip (slot 2, the head of the party hit-reaction map FUN_80053CB8
 * writes), the queue -> art resolution (the SCUS arts-name table's own
 * combo strings through the recognizer's greedy walk; kind labels joined
 * from the curated gamedata table), and - new - THE CHROME ITSELF: every
 * chip, plate, badge, digit and banner strip is the retail graphic decoded
 * off the visitor's disc (muscle_hud_json / muscle_hud_sheet_rgba):
 * the chip/plate 3-slice art, D-pad, AP-plate pieces and HP/MP badges from
 * the boot-gap widget TIM (VRAM (896,256), CLUT row 511), chip labels in
 * the boot-gap ASCII battle font ((896,0) through the menu-atlas palette),
 * small digits from the menu-glyph atlas ((960,256)), the red cross-out X +
 * SUPER/HYPER/MIRACLE ARTS!! strips + DAMAGE/HIT/TOTAL words + the big
 * orange damage numerals from the battle-effect bank etim (PROT 0870, VRAM
 * (448,0), CLUT row 476), and the "Welcome to the Muscle Dome!" cursive +
 * INTERVAL/ROUND headings from the dome's own data file (extraction 1220)
 * through the PROT 0977 overlay's sprite descriptor table
 * (engine-ui::other_game_hud). Screen geometry comes from the SCUS-static
 * battle HUD element table (0x80076C10) plus a live PCSX-Redux packet
 * capture of a dome match (command cluster, enemy art, player HYPER ARTS!!
 * playback - scripts/pcsx-redux/autorun_muscle_hud_capture.lua).
 * STILL FITTED: the base camera seat, fighter spacing/facing, which traced
 * blip fires on which page event, the KO clip pick (slot 4 of the pinned
 * reaction family), the small art-name caption + hint lines (page aids),
 * the banner speed-line rays (polygonal in retail, procedural here), the
 * SUPER/MIRACLE banner word composition (atlas layout, only the HYPER
 * strip's draw is packet-pinned), the interval panel's info layout (only
 * its INTERVAL/ROUND headings are the retail art), and the glide-in motion
 * (retail slides chips between the element table's two endpoints; the page
 * draws them parked at the arrived endpoint). Without a disc image the
 * chrome falls back to the old canvas approximation.
 *
 * HONEST GAPS: the rules engine resolves each committed command as a basic
 * strike - retail expands a recognized art sequence through the art records
 * (more damage), so here the arts banner is presentation over the real
 * recognition, not an arts damage model; and the port has no cast path, so
 * the Ra-Seru (magic) chip renders disabled even though retail's Beginner/
 * Expert courses allow magic.
 *
 * Requires webgl-math.js + webgl-shaders.js + webgl-tmd.js first.
 */
window.MgMuscle = (function () {
  'use strict';

  const A2R = (Math.PI * 2) / 4096;   /* PSX angle units -> radians */
  const HUD_W = 320, HUD_H = 240;     /* retail frame; canvas is 2x */

  /* The four swing-command ids and their directions - the runtime
   * action-constant space (crates/art queue.rs: 0x0C Left, 0x0D Right,
   * 0x0E Down, 0x0F Up). */
  const CMD = {
    12: { name: 'Left',  glyph: '←', dir: 'left' },
    13: { name: 'Right', glyph: '→', dir: 'right' },
    14: { name: 'Down',  glyph: '↓', dir: 'down' },
    15: { name: 'Up',    glyph: '↑', dir: 'up' },
  };

  /* Player battle-form clip slots (player battle file record[0] + swing
   * records; crates/web-viewer muscle_fighter_* APIs). Slot 0 = idle; the
   * four swings live AT the card ids 0xC..0xF (the disc's own pairing);
   * slot 2 = the light flinch (head of the party hit-reaction map
   * [2,3,4,5,0xB] FUN_80053CB8 writes to +0x1EF..); slot 4 = the
   * knockdown-family pick for the KO hold (fitted within that pinned map). */
  const P_ANIM = { IDLE: 0, HIT: 2, KO: 4 };

  /* Ra-Seru names - the retail magic-command chip label per character
   * (capture: Vahn's chip reads "Meta"). */
  const RA_SERU = ['Meta', 'Terra', 'Ozma'];

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
    let mode = 'idle';         /* idle|intro|select|playback|interval|decided */
    let selectSub = 'menu';    /* select submode: menu | attack */
    let introT = 0;            /* ticks into the intro card */
    let tick = 0;
    let banner = null;         /* {text, sub, t, life, cls} */
    let popups = [];           /* {text, x, y, t, life, color} */
    let playQueue = [];        /* remaining round-log events */
    let playT = 0;             /* ticks into the current event */
    let pIdx = 0;              /* player events landed this playback */
    let artsSpans = [];        /* muscle_round_arts_json rows */
    let artsBanner = null;     /* {text, name, t, life} */
    let hpShow = [0, 0];       /* eased HP bar values */
    let lastOpts = null;       /* {char, level, monster} for restart */
    let roster = null;         /* muscle_roster_json rows */
    let meter = 0;             /* round time meter 0..0xC (FUN_801d3444) */
    let tally = null;          /* {attacker, total} - playback damage tally */

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

    /* -------------------------------------------- retail chrome (disc art)
     *
     * Sheets are the retail texture pages decoded through their captured
     * palettes on the Rust side (muscle_hud_sheet_rgba); hudMeta carries
     * the capture-pinned piece rects, the SCUS element-table anchors, the
     * PROT 0977 hub sprite records and the font advances. See the header
     * note for the per-sheet disc sources. */
    let hudMeta;               /* undefined until asked; null = unavailable */
    const hudSheets = {};      /* "src:pal" -> canvas (null = failed) */
    const SHEET_NAMES = ['widget', 'font', 'atlas', 'banner', 'hub0', 'hub1'];

    function hudOk() {
      if (hudMeta === undefined) {
        try {
          const m = JSON.parse(api.muscle_hud_json());
          hudMeta = m && m.ok ? m : null;
        } catch (e) { hudMeta = null; }
      }
      return !!hudMeta;
    }

    function sheet(src, pal) {
      const key = src + ':' + pal;
      if (hudSheets[key] !== undefined) return hudSheets[key];
      let c = null;
      try {
        const dims = hudMeta.sheets[SHEET_NAMES[src]];
        const rgba = api.muscle_hud_sheet_rgba(src, pal);
        if (dims && rgba && rgba.length === dims[0] * dims[1] * 4) {
          c = document.createElement('canvas');
          c.width = dims[0]; c.height = dims[1];
          const cg = c.getContext('2d');
          const id = cg.createImageData(dims[0], dims[1]);
          id.data.set(rgba);
          cg.putImageData(id, 0, 0);
        }
      } catch (e) { c = null; }
      hudSheets[key] = c;
      return c;
    }

    /* Blit sheet rect (u,v,w,h) to retail-space (dx,dy), optional dest size. */
    function blit(src, pal, u, v, w, h, dx, dy, dw, dh) {
      const s = sheet(src, pal);
      if (!s) return false;
      g.drawImage(s, u, v, w, h, dx * 2, dy * 2, (dw || w) * 2, (dh || h) * 2);
      return true;
    }

    function hudAdv(ch) {
      const i = ch.charCodeAt(0) - 0x20;
      return (hudMeta.advance && hudMeta.advance[i]) || 6;
    }
    function hudTextW(s) {
      let w = 0;
      for (const c of s) w += hudAdv(c);
      return w;
    }
    /* The retail ASCII battle font: 16x16 cells drawn as 14x15 sprites,
     * pen stepped by the per-glyph advance (capture-matched). */
    function hudText(s, x, y) {
      const pal = hudMeta.pieces.font_pal;
      let pen = x;
      for (const c of s) {
        const i = c.charCodeAt(0) - 0x20;
        if (i > 0 && i < 96) {
          blit(1, pal, (i % 16) * 16, ((i / 16) | 0) * 16, 14, 15, pen, y);
        }
        pen += hudAdv(c);
      }
      return pen - x;
    }
    /* Menu-atlas small digits (8x12, u = digit*8) + the widget '/'. */
    function hudDigits(str, x, y) {
      const d = hudMeta.pieces.atlas_digits;
      const sl = hudMeta.pieces.slash;
      let pen = x;
      for (const c of str) {
        if (c === '/') {
          blit(0, sl.pal, sl.r[0], sl.r[1], sl.r[2], sl.r[3], pen, y - 2);
          pen += 8;
        } else if (c >= '0' && c <= '9') {
          blit(2, d.pal, d.x0 + (c.charCodeAt(0) - 48) * d.cell, d.v, 8, d.h, pen, y);
          pen += 8;
        } else {
          pen += 8;
        }
      }
      return pen - x;
    }
    /* The etim big orange numerals: 24x24 cells at v=64, u=(d-1)*24, 0 at
     * u=216 (packet-pinned); default screen size 16x15 (the TOTAL row),
     * 24x23 for the flying hit numbers. */
    function hudBigDigits(str, x, y, dw, dh) {
      const v = hudMeta.pieces.digit24_v;
      let pen = x;
      for (const c of str) {
        if (c >= '0' && c <= '9') {
          const dgt = c.charCodeAt(0) - 48;
          const u = dgt === 0 ? 216 : (dgt - 1) * 24;
          blit(3, 3, u, v, 24, 24, pen, y, dw || 16, dh || 15);
        }
        pen += (dw || 16);
      }
      return pen - x;
    }
    function hudWord(name, x, y, dw, dh) {
      const p = hudMeta.pieces[name];
      if (!p) return 0;
      blit(3, p.pal, p.r[0], p.r[1], p.r[2], p.r[3], x, y, dw, dh);
      return dw || p.r[2];
    }
    /* One dome-hub sprite (PROT 0977 descriptor record) at 1:1. */
    function hubSprite(i, dx, dy, dw, dh) {
      const r = hudMeta.hub && hudMeta.hub[i];
      if (!r) return false;
      return blit(r.sheet, r.pal, r.uv[0], r.uv[1], r.wh[0], r.wh[1], dx, dy, dw, dh);
    }

    /* Retail chip: 3-slice plate (8px caps + 16px body slices, 20 tall)
     * with the label left-aligned at the body start - exactly the captured
     * packet decomposition. (x, y) anchor the BODY's top-left (the element
     * table's chip anchor minus the retail (8, 6) label offset is applied
     * by the callers). bodyW defaults to the label width. */
    function rChip(label, x, y, style, bodyW) {
      const p = style === 'gold' ? hudMeta.pieces.plate_gold : hudMeta.pieces.plate_blue;
      const bw = Math.max(bodyW || 0, hudTextW(label));
      blit(0, p.pal, p.cap_l[0], p.cap_l[1], 8, 20, x - 8, y);
      for (let bx = 0; bx < bw; bx += 16) {
        const w = Math.min(16, bw - bx);
        blit(0, p.pal, p.body[0], p.body[1], w, 20, x + bx, y);
      }
      blit(0, p.pal, p.cap_r[0], p.cap_r[1], 8, 20, x + bw, y);
      hudText(label, x, y + 4);
      return bw;
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
      if (!api.muscle_scene_ready || !api.muscle_scene_ready(monsterId, charSlot)) return null;

      /* The player's assembled battle form (fighter form - the retail dome
       * roster), not the Baka pack. */
      const P = {
        pos: api.muscle_fighter_positions(charSlot),
        uvs: api.muscle_fighter_uvs(charSlot),
        ct: api.muscle_fighter_cba_tsb(charSlot),
        idx: api.muscle_fighter_indices(charSlot),
        oid: api.muscle_fighter_object_ids(charSlot),
        flat: api.muscle_fighter_flat_rgba(charSlot),
        parts: api.muscle_fighter_part_count(charSlot),
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

      /* Player clips out of the battle form's own action streams: idle
       * (record[0] slot 0), flinch (slot 2), KO family (slot 4), and the
       * four per-command swings AT the card ids 0xC..0xF - the disc's own
       * card -> clip pairing. Rates follow the entry's +0x78 byte through
       * the same rate/8-per-tick scale as the monster clips. */
      let pAnims = [];
      try { pAnims = JSON.parse(api.muscle_fighter_anims_json(charSlot)); }
      catch (e) { pAnims = []; }
      const pClip = (slot) => {
        const row = pAnims.find(a => a.slot === slot);
        if (!row || !row.frame_count) return null;
        const frames = api.muscle_fighter_pose_frames(charSlot, slot, P.parts);
        if (!frames.length) return null;
        return {
          frames, frameCount: row.frame_count, parts: P.parts,
          rate: Math.max(1, (row.rate || 1) * 2),
        };
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
      const pIdle = pClip(P_ANIM.IDLE);
      const pHit = pClip(P_ANIM.HIT);
      const clips = [
        { idle: pIdle, hit: pHit || pIdle,
          ko: pClip(P_ANIM.KO) || pHit || pIdle,
          byCmd: Object.fromEntries([12, 13, 14, 15]
            .map(c => [c, pClip(c)])) },
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
      /* Two-pass PSX semi-transparency for the shell's ABE lamp-glow prims
       * (ABR mode 1, additive) - the legacy single pass draws them opaque
       * (the dance-hall smoke defect shape; see webgl-tmd.js semiTwoPass).
       * The stream's OTHER additive set - the object-1 wall-base dust
       * decal - is omitted on the Rust side (muscle_arena_hybrid): its
       * texels are genuinely bright, so any draw of it reads as a cloud
       * band, and the retail match capture shows a mist-free interior. */
      renderer.semiTwoPass = true;
      renderer.uploadVram(api.muscle_vram(monsterId, charSlot));
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
      s.dragging = () => drag;
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
      artsSpans = [];
      artsBanner = null;
      banner = null;
      /* Retail contest entry: the black "Welcome to the Muscle Dome!" card,
       * then straight into round 1's command menu. Skippable. */
      mode = 'intro';
      introT = 0;
      selectSub = 'menu';
      return true;
    }

    /* Leave the intro card for round 1's command menu. */
    function beginSelect() {
      mode = 'select';
      selectSub = 'menu';
      setBanner('ROUND 1', null, 70);
    }

    function commit(slot) {
      if (mode !== 'select') return false;
      selectSub = 'attack';   /* a commit is directional input */
      const ok = api.muscle_commit(slot);
      /* Confirm blip on a committed command; cursor blip on a rejected one
       * (overspend / queue full) - fitted assignment over the traced ids. */
      playCue(ok ? 'confirm' : 'cursor', ok ? 0.5 : 0.35);
      return ok;
    }

    /* Commit the hand card matching a pad direction (the retail attack
     * input: directions go straight onto the AP gauge). */
    function commitDir(dir) {
      const state = st();
      if (!state.live) return false;
      const hand = state.hand || [];
      const i = hand.findIndex(c => (CMD[c.cmd] || {}).dir === dir);
      return i >= 0 ? commit(i) : false;
    }

    /* Close selection and play the round out. */
    function fight() {
      api.muscle_end_selection();
      api.muscle_resolve();
      playQueue = JSON.parse(api.muscle_round_log_json());
      /* The committed queue resolved through the character's real arts
       * tables (SCUS combo strings + curated kind labels) - the spans the
       * retail arts banner covers during playback. */
      try { artsSpans = JSON.parse(api.muscle_round_arts_json()); }
      catch (e) { artsSpans = []; }
      playT = 0;
      pIdx = 0;
      banner = null;
      artsBanner = null;
      tally = null;
      mode = 'playback';
    }

    /* Pad-shaped input from the page: left/right/up/down/back. */
    function key(name) {
      if (mode === 'intro') { beginSelect(); return; }
      if (mode !== 'select') return;
      if (selectSub === 'menu') {
        if (name === 'left') {            /* Attack: directional input */
          selectSub = 'attack';
          playCue('cursor', 0.4);
        } else if (name === 'down') {     /* Spirit: end selection, fight */
          playCue('confirm', 0.5);
          fight();
        } else if (name === 'up' || name === 'right') {
          /* Item (crossed out) / Ra-Seru: disabled here - Item by the
           * course rules, magic by the port's missing cast path. */
          playCue('blip', 0.3);
        }
      } else {
        if (name === 'back') { selectSub = 'menu'; playCue('cursor', 0.4); }
        else if (name === 'left' || name === 'right' ||
                 name === 'up' || name === 'down') commitDir(name);
      }
    }

    /* SPACE / Confirm: advances whatever the current presentation mode is. */
    function confirm() {
      const state = st();
      if (!state.live) { if (lastOpts) start(lastOpts); return; }
      if (mode === 'intro') {
        beginSelect();
      } else if (mode === 'select') {
        fight();
      } else if (mode === 'playback') {
        /* Skip: settle every pending event instantly. */
        while (playQueue.length) applyEvent(playQueue.shift(), true);
        artsBanner = null;
        finishPlayback();
      } else if (mode === 'interval') {
        api.muscle_next_round();
        const s2 = st();
        if (s2.phase === 'select') {
          mode = 'select';
          selectSub = 'menu';
          setBanner('ROUND ' + (s2.round + 1), null, 70);
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
          play(0, scene.clips[0].byCmd[ev.cmd] || scene.clips[0].idle);
        } else {
          play(1, scene.clips[1].attack);
        }
      }
      /* Retail arts banner: when this player event starts a recognized art
       * sequence, raise the class banner over the whole span. */
      if (ev.attacker === 0) {
        if (!instant) {
          const span = artsSpans.find(a => a.start === pIdx);
          if (span) {
            const kind = String(span.kind || 'regular');
            const text = (kind === 'regular' ? '' : kind.toUpperCase() + ' ') + 'ARTS!!';
            artsBanner = { text, kind, name: span.name || '', t: 0, life: span.len * 34 };
          }
        }
        pIdx++;
      }
      hpShow[defender] = ev.hp[defender];
      /* The running damage tally of the current attacker's sequence -
       * retail draws it as yellow numerals ("TOTAL n") in the lower-right
       * while the queued commands play out; it resets when the other
       * fighter takes over. */
      if (!tally || tally.attacker !== ev.attacker) {
        tally = { attacker: ev.attacker, total: 0 };
      }
      tally.total += ev.damage;
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
          play(loser, loser === 1 ? scene.clips[1].ko : scene.clips[0].ko, true);
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
        selectSub = 'menu';
      }
    }

    /* ------------------------------------------------------------ HUD draw
     *
     * Canvas approximations of the retail battle chrome (blue-marble plates
     * with gold borders, bevelled gold chips, the crossed-out Item chip, the
     * pointed AP / status plates). Geometry + colours are FITTED to the
     * retail captures; the wording is the captures' own. */

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

    /* One chrome plate. style: 'blue' marble / 'gold' bevel / 'grey'
     * (disabled). pointed: extend hexagonal points on both ends. */
    function plate(x, y, w, h, style, pointed) {
      const X = x * 2, Y = y * 2, W = w * 2, H = h * 2, P = pointed ? H / 2 : 0;
      g.save();
      /* Outline as a Path2D so the border strokes stay on the plate even
       * after the mottling loop replaces the context's current path. */
      const outline = new Path2D();
      if (pointed) {
        outline.moveTo(X - P, Y + H / 2);
        outline.lineTo(X, Y); outline.lineTo(X + W, Y);
        outline.lineTo(X + W + P, Y + H / 2);
        outline.lineTo(X + W, Y + H); outline.lineTo(X, Y + H);
      } else {
        const r = Math.min(7, H / 2);
        outline.moveTo(X + r, Y);
        outline.lineTo(X + W - r, Y); outline.quadraticCurveTo(X + W, Y, X + W, Y + r);
        outline.lineTo(X + W, Y + H - r);
        outline.quadraticCurveTo(X + W, Y + H, X + W - r, Y + H);
        outline.lineTo(X + r, Y + H); outline.quadraticCurveTo(X, Y + H, X, Y + H - r);
        outline.lineTo(X, Y + r); outline.quadraticCurveTo(X, Y, X + r, Y);
      }
      outline.closePath();
      const grad = g.createLinearGradient(0, Y, 0, Y + H);
      if (style === 'gold') {
        grad.addColorStop(0, '#d8b268'); grad.addColorStop(0.45, '#b98f3e');
        grad.addColorStop(1, '#8a6526');
      } else if (style === 'grey') {
        grad.addColorStop(0, '#6b6f7e'); grad.addColorStop(1, '#494c58');
      } else {
        grad.addColorStop(0, '#7d82c8'); grad.addColorStop(0.5, '#565b9e');
        grad.addColorStop(1, '#3c4084');
      }
      g.fillStyle = grad;
      g.fill(outline);
      /* Marble mottling on the blue plates (cheap, deterministic). */
      if (style === 'blue') {
        g.save(); g.clip(outline);
        g.fillStyle = 'rgba(255,255,255,0.10)';
        for (let i = 0; i < Math.max(2, (w / 18) | 0); i++) {
          const mx = X + ((i * 73 + x * 31 + y * 17) % Math.max(1, W));
          const my = Y + ((i * 41 + x * 13) % Math.max(1, H));
          g.beginPath(); g.ellipse(mx, my, 9, 4, 0.6, 0, Math.PI * 2); g.fill();
        }
        g.restore();
      }
      g.lineWidth = 2.5;
      g.strokeStyle = style === 'gold' ? '#5d431a'
        : style === 'grey' ? '#2e3038' : '#c8a24a';
      g.stroke(outline);
      g.lineWidth = 1;
      g.strokeStyle = 'rgba(255,244,200,0.5)';
      g.stroke(outline);
      g.restore();
    }

    /* A command chip: plate + centred label. */
    function chip(x, y, w, h, style, label, labelCol) {
      plate(x, y, w, h, style);
      const col = labelCol || (style === 'gold' ? '#2e1f06'
        : style === 'grey' ? '#b9bcc6' : '#f2f4fa');
      text(label, x + w / 2, y + h / 2 + 0.5, Math.min(8, h - 6), col, 'center');
    }

    /* The retail Item chip's red cross-out. */
    function crossOut(x, y, w, h) {
      g.save();
      g.strokeStyle = '#c41f1f';
      g.lineWidth = 7;
      g.lineCap = 'round';
      g.beginPath();
      g.moveTo((x - 2) * 2, (y - 1) * 2); g.lineTo((x + w + 2) * 2, (y + h + 1) * 2);
      g.moveTo((x + w + 2) * 2, (y - 1) * 2); g.lineTo((x - 2) * 2, (y + h + 1) * 2);
      g.stroke();
      g.restore();
    }

    /* The grey D-pad glyph between Attack and the Ra-Seru chip. */
    function dpadGlyph(cx, cy, r) {
      const X = cx * 2, Y = cy * 2, R = r * 2, a = R * 0.38;
      g.save();
      g.fillStyle = '#cfd2da';
      g.strokeStyle = '#5a5d68';
      g.lineWidth = 2;
      g.beginPath();
      g.moveTo(X - a, Y - R); g.lineTo(X + a, Y - R); g.lineTo(X + a, Y - a);
      g.lineTo(X + R, Y - a); g.lineTo(X + R, Y + a); g.lineTo(X + a, Y + a);
      g.lineTo(X + a, Y + R); g.lineTo(X - a, Y + R); g.lineTo(X - a, Y + a);
      g.lineTo(X - R, Y + a); g.lineTo(X - R, Y - a); g.lineTo(X - a, Y - a);
      g.closePath();
      g.fill(); g.stroke();
      g.fillStyle = '#9a9daa';
      g.beginPath(); g.arc(X, Y, a * 0.7, 0, Math.PI * 2); g.fill();
      g.restore();
    }

    /* Retail intro card: pure black with the "Welcome to the Muscle Dome!"
     * cursive strip - the 240x18 hub sprite (PROT 0977 record 3) off the
     * dome data file (extraction 1220), drawn 1:1 (its glow is baked into
     * the texels). Falls back to a system cursive without disc chrome. */
    function drawIntro() {
      g.fillStyle = '#000';
      g.fillRect(0, 0, hudCanvas.width, hudCanvas.height);
      const a = Math.min(1, introT / 25);
      g.save();
      g.globalAlpha = a;
      if (hudOk() && hubSprite(3, (320 - 240) / 2, 112)) {
        /* drawn */
      } else {
        g.font = 'italic ' + (15 * 2) + 'px "Brush Script MT", "Segoe Script", "Comic Sans MS", cursive';
        g.textAlign = 'center';
        g.textBaseline = 'middle';
        g.shadowColor = 'rgba(176,186,255,0.95)';
        g.shadowBlur = 16;
        g.fillStyle = '#f4f6ff';
        g.fillText('Welcome to the Muscle Dome!', HUD_W, HUD_H - 14);
        g.shadowBlur = 6;
        g.fillText('Welcome to the Muscle Dome!', HUD_W, HUD_H - 14);
      }
      g.restore();
      if (introT > 90) {
        text('SPACE', 306, 230, 6, 'rgba(174,182,196,0.7)', 'right', '');
      }
    }

    /* Top-left Begin + fighter-name chips (retail command-input header;
     * capture: gold body starts at x=16 / x=68, plates 20 tall at y=8). */
    function drawHeaderChips(state) {
      if (hudOk()) {
        const w = rChip('Begin', 16, 8, 'gold');
        rChip(state.names[0], 24 + w + 8, 8, 'gold');
        return;
      }
      chip(6, 6, 40, 13, 'gold', 'Begin');
      chip(52, 6, Math.max(36, state.names[0].length * 7 + 10), 13, 'gold', state.names[0]);
    }

    /* The retail command cluster: Item (crossed out) on top; Attack +
     * D-pad + Ra-Seru; Spirit below. Anchors + widths are the SCUS element
     * table's arrived glide endpoints (elements 8 / 9 / 0xA / 0xB), the
     * plate/label offsets and the D-pad seat the captured packets. */
    function drawCommandCluster(state) {
      const raSeru = RA_SERU[state.char] || 'Meta';
      const inAttack = selectSub === 'attack';
      if (hudOk()) {
        const el = (i, dx, dy) => {
          const e = hudMeta.elements[i];
          return e ? { x: e.b[0], y: e.b[1], w: e.w } : { x: dx, y: dy, w: 48 };
        };
        const item = el(8, 204, 34);
        const atk = el(9, 160, 66);
        const ras = el(0xA, 248, 66);
        const spi = el(0xB, 204, 98);
        /* Item - crossed out with the retail 64x16 red X (etim page). */
        rChip('Item', item.x, item.y - 6, 'blue', item.w);
        hudWord('red_x', item.x - 8, item.y - 4);
        /* Attack + the D-pad glyph between it and the Ra-Seru chip. */
        rChip('Attack', atk.x, atk.y - 6, inAttack ? 'gold' : 'blue', atk.w);
        blit(0, hudMeta.pieces.dpad.pal,
          hudMeta.pieces.dpad.r[0], hudMeta.pieces.dpad.r[1], 16, 16,
          (atk.x + atk.w + ras.x - 8) / 2 - 8, atk.y - 4);
        /* Ra-Seru (magic) - crossed out too: the port has no cast path
         * (honest gap; retail crosses it only on magic-forbidden courses). */
        rChip(raSeru, ras.x, ras.y - 6, 'blue', ras.w);
        hudWord('red_x', ras.x - 8, ras.y - 4);
        /* Spirit - ends selection. */
        rChip('Spirit', spi.x, spi.y - 6, 'blue', spi.w);
        if (!inAttack && !banner) {
          text('←Attack  ↓Spirit  SPACE Begin', 214, 116, 6, '#aeb6c4', 'center', '');
        }
        return;
      }
      chip(196, 20, 60, 13, 'blue', 'Item');
      crossOut(196, 20, 60, 13);
      chip(150, 48, 54, 14, inAttack ? 'gold' : 'blue', 'Attack');
      dpadGlyph(216, 55, 7);
      chip(228, 48, 50, 14, 'grey', raSeru);
      chip(178, 76, 60, 14, 'blue', 'Spirit');
      if (!inAttack && !banner) {
        text('←Attack  ↓Spirit  SPACE Begin', 214, 100, 6, '#aeb6c4', 'center', '');
      }
    }

    /* Committed directional input (the retail arts strip: arrows appear as
     * you enter them), drawn over the AP plate while selecting. The strip
     * itself is a page aid (retail consumes the AP gauge instead). */
    function drawQueueStrip(state) {
      const q = state.queue[0];
      if (!q.length && selectSub !== 'attack') return;
      let s = '';
      for (const cmd of q) s += (CMD[cmd] ? CMD[cmd].glyph : '?') + ' ';
      plate(180, 160, 126, 12, 'blue', false);
      text(s || '· · ·', 243, 166, 8, '#ffe9a8', 'center');
      if (selectSub === 'attack') {
        text('arrows commit · SPACE fight · ESC back', 306, 152, 6, '#aeb6c4', 'right', '');
      }
    }

    /* The retail AP plate. Capture decomposition at (208, 172): the 24x16
     * pointed "AP" label piece, the 56x16 gauge trough, the 16x16 end
     * piece + 8x16 cap, the 16x6 orange fill tile, small numerals. */
    function drawApPlate(state) {
      const budget = state.budget[0];
      const pool = state.stats ? state.stats[0].budget_pool : budget;
      if (hudOk()) {
        const x = 208, y = 172, p = hudMeta.pieces;
        blit(0, p.ap_label.pal, ...p.ap_label.r, x, y);
        blit(0, p.ap_trough.pal, ...p.ap_trough.r, x + 24, y);
        blit(0, p.ap_end.pal, ...p.ap_end.r, x + 80, y);
        blit(0, p.ap_cap.pal, ...p.ap_cap.r, x + 96, y);
        const frac = pool ? Math.max(0, Math.min(1, budget / pool)) : 0;
        if (frac > 0) {
          blit(0, p.gauge_fill.pal, p.gauge_fill.r[0], p.gauge_fill.r[1],
            p.gauge_fill.r[2], p.gauge_fill.r[3], x + 28, y + 5, 48 * frac, 6);
        }
        const num = String(budget);
        hudDigits(num, x + 96 - num.length * 8 + 6, y + 3);
        return;
      }
      plate(190, 188, 112, 12, 'blue', true);
      text('AP', 196, 194, 7, '#e2453a');
      bar(210, 191, 64, 6, pool ? budget / pool : 0, '#f0a428', 'rgba(20,16,40,0.8)');
      text(String(budget), 298, 194, 7, '#ffd166', 'right');
    }

    /* The retail bottom status plate. Capture decomposition: caps at
     * (8, 188)/(304, 188), 16x20 body slices across, name in the battle
     * font at (16, 192), HP/MP badges at (80, 194)/(192, 194), menu-atlas
     * digits with the widget '/' at the captured columns. */
    function drawStatusPlate(state) {
      const mp = (state.mp_max && state.mp_max[0]) || 0;
      const cur = String(Math.max(0, Math.round(hpShow[0])));
      const max = String(state.hp_max[0]);
      if (hudOk()) {
        const p = hudMeta.pieces, y = 188;
        blit(0, p.plate_blue.pal, p.plate_blue.cap_l[0], p.plate_blue.cap_l[1], 8, 20, 8, y);
        for (let bx = 16; bx < 304; bx += 16) {
          blit(0, p.plate_blue.pal, p.plate_blue.body[0], p.plate_blue.body[1],
            Math.min(16, 304 - bx), 20, bx, y);
        }
        blit(0, p.plate_blue.pal, p.plate_blue.cap_r[0], p.plate_blue.cap_r[1], 8, 20, 304, y);
        hudText(state.names[0], 16, y + 4);
        blit(0, p.hp_badge.pal, ...p.hp_badge.r, 80, y + 6);
        hudDigits(cur, 134 - cur.length * 8, y + 4);
        hudDigits('/', 134, y + 4);
        hudDigits(max, 144, y + 4);
        blit(0, p.mp_badge.pal, ...p.mp_badge.r, 192, y + 6);
        const mps = String(mp);
        hudDigits(mps, 238 - mps.length * 8, y + 4);
        hudDigits('/', 238, y + 4);
        hudDigits(mps, 248, y + 4);
        return;
      }
      plate(8, 214, 304, 16, 'blue', true);
      text(state.names[0], 16, 222, 8, '#f2f4fa');
      text('HP', 96, 222, 8, '#ffd23e');
      text(cur + '/' + max, 118, 222, 8, '#f2f4fa');
      text('MP', 208, 222, 8, '#37d3b1');
      text(mp + '/' + mp, 230, 222, 8, '#f2f4fa');
    }

    /* Defender name chip (retail shows it during playback at the element
     * table's arrived endpoint (200, 162); enemy HP is never drawn). */
    function drawFoeChip(state) {
      const name = state.names[1] || '';
      if (!name) return;
      if (hudOk()) {
        const e = hudMeta.elements[0x29];
        rChip(name, e ? e.b[0] : 200, (e ? e.b[1] : 162) - 6, 'blue',
          e ? e.w : undefined);
        return;
      }
      const w = Math.max(44, name.length * 7 + 12);
      chip(310 - w, 196, w, 13, 'blue', name);
    }

    /* Attacker name chip, top-left gold (retail arts-playback header). */
    function drawAttackerChip(name) {
      if (hudOk()) { rChip(name, 16, 8, 'gold'); return; }
      chip(6, 6, Math.max(40, name.length * 7 + 12), 13, 'gold', name);
    }

    /* The retail arts banner: orange-gradient block capitals with a dark
     * outline over white radial speed-lines. */
    function drawArtsBanner() {
      const b = artsBanner;
      if (!b) return;
      if (b.t > b.life) { artsBanner = null; return; }
      const a = b.t < 6 ? b.t / 6 : b.t > b.life - 10 ? (b.life - b.t) / 10 : 1;
      g.save();
      g.globalAlpha = Math.max(0, Math.min(1, a));
      /* White radial speed-line rays. */
      const cx = HUD_W, cy = HUD_H;
      g.save();
      g.translate(cx, cy);
      g.rotate(b.t * 0.004);
      g.fillStyle = 'rgba(255,255,255,0.30)';
      const R = 460;
      for (let i = 0; i < 18; i++) {
        const ang = (i / 18) * Math.PI * 2;
        const halfW = 0.045;
        g.beginPath();
        g.moveTo(0, 0);
        g.lineTo(Math.cos(ang - halfW) * R, Math.sin(ang - halfW) * R);
        g.lineTo(Math.cos(ang + halfW) * R, Math.sin(ang + halfW) * R);
        g.closePath();
        g.fill();
      }
      g.restore();
      /* The banner strip. Retail draws the etim word row as two textured
       * quads at (52,144)-(268,178) - 1:1 horizontally, 24 texels
       * stretched to 34 px vertically (packet-pinned for HYPER ARTS!!).
       * SUPER / MIRACLE compose their word + the ARTS!! strip from the
       * same atlas rows (layout-inferred - only HYPER is packet-pinned);
       * a regular art raises the ARTS!! strip alone. */
      const pop = b.t < 6 ? 0.7 + 0.3 * (b.t / 6) : 1;
      let drawn = false;
      if (hudOk()) {
        const kind = b.kind || 'regular';
        g.save();
        g.translate(HUD_W * (1 - pop), (161 * 2) * (1 - pop));
        g.scale(pop, pop);
        const vs = 34 / 24;   /* the captured vertical stretch */
        const seat = (w) => (320 - w) / 2;
        if (kind === 'hyper') {
          /* The packet-pinned draw: uv (0,176)-(215,199) at (52,144),
           * 216 wide 1:1, stretched to 34 px tall. */
          const p = hudMeta.pieces.word_hyper;
          drawn = blit(3, p.pal, 0, 176, 216, 24, 52, 144, 216, 34);
        } else if (kind === 'super' || kind === 'miracle') {
          const word = hudMeta.pieces[kind === 'super' ? 'word_super' : 'word_miracle'];
          const q = hudMeta.pieces.word_arts;
          const total = word.r[2] + 8 + q.r[2];
          const x0 = seat(total);
          drawn = blit(3, word.pal, ...word.r, x0, 144, word.r[2], word.r[3] * vs);
          blit(3, q.pal, ...q.r, x0 + word.r[2] + 8, 144, q.r[2], q.r[3] * vs);
        } else {
          const q = hudMeta.pieces.word_arts;
          drawn = blit(3, q.pal, ...q.r, seat(q.r[2]), 144, q.r[2], q.r[3] * vs);
        }
        g.restore();
      }
      if (!drawn) {
        g.translate(cx, cy + 24);
        g.scale(pop, pop);
        g.font = 'bold ' + (26 * 2) + 'px "Arial Black", ui-sans-serif, sans-serif';
        g.textAlign = 'center';
        g.textBaseline = 'middle';
        const grad = g.createLinearGradient(0, -30, 0, 30);
        grad.addColorStop(0, '#ffe98a');
        grad.addColorStop(0.55, '#ffab2e');
        grad.addColorStop(1, '#f2600f');
        g.lineWidth = 8;
        g.strokeStyle = '#4a1404';
        g.strokeText(b.text, 0, 0);
        g.fillStyle = grad;
        g.fillText(b.text, 0, 0);
      }
      /* Small art-name caption - a page aid (retail names the move in a
       * floating caption near the fighter). */
      if (b.name) {
        g.font = 'bold ' + (8 * 2) + 'px ui-monospace, monospace';
        g.textAlign = 'center';
        g.textBaseline = 'middle';
        g.lineWidth = 3;
        g.strokeStyle = '#4a1404';
        g.strokeText(b.name, HUD_W, 200 * 2);
        g.fillStyle = '#ffe9a8';
        g.fillText(b.name, HUD_W, 200 * 2);
      }
      g.restore();
      b.t++;
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

    function drawInterval(state) {
      g.fillStyle = 'rgba(4,6,10,0.82)';
      g.fillRect(30 * 2, 52 * 2, 260 * 2, 136 * 2);
      g.strokeStyle = 'rgba(255,255,255,0.25)';
      g.strokeRect(30 * 2 + 0.5, 52 * 2 + 0.5, 260 * 2, 136 * 2);
      /* "INTERVAL" is retail's own between-round heading - the 192x32 hub
       * sprite (PROT 0977 record 16) off the dome data file. The panel's
       * info layout below is a page aid (the retail interval screen's own
       * layout is uncaptured). */
      if (!(hudOk() && hubSprite(16, (320 - 192) / 2, 56))) {
        text('INTERVAL', 160, 64, 10, '#ffd166', 'center');
      }
      text('round ' + (state.round + 1) + ' settled', 160, 75, 6, '#aeb6c4', 'center', '');
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
      text('commands played   you ' + q0 + '  ·  foe ' + q1,
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
      /* "ROUND n": the retail hub art - the 144x32 ROUND word (PROT 0977
       * record 0) + the hub 24x32 digit strip (record 1, u = digit*24). */
      const round = /^ROUND (\d+)$/.exec(banner.text);
      if (round && hudOk() && hudMeta.hub && hudMeta.hub[1]) {
        const num = round[1];
        const total = 144 + 10 + num.length * 24;
        const x0 = (320 - total) / 2;
        hubSprite(0, x0, 92);
        const d1 = hudMeta.hub[1];
        for (let i = 0; i < num.length; i++) {
          const dgt = num.charCodeAt(i) - 48;
          blit(d1.sheet, d1.pal, dgt * 24, d1.uv[1], 24, 32,
            x0 + 154 + i * 24, 92);
        }
      } else {
        const col = banner.cls === 'good' ? '#2dcca7'
          : banner.cls === 'bad' ? '#d84b4b' : '#ffd166';
        text(banner.text, 160, 108, 16, col, 'center');
      }
      if (banner.sub) text(banner.sub, 160, 126, 7, '#e8ecf2', 'center', '');
      g.restore();
      banner.t++;
    }

    /* The retail play-out damage tally: the etim TOTAL word + the big
     * orange numerals, at the captured seat (word at (184, 170), 16x15
     * digit cells ending at x=304 - run5 packet listing). */
    function drawTally() {
      if (!tally || !tally.total) return;
      if (hudOk()) {
        const num = String(tally.total);
        hudWord('word_total', 184, 170);
        hudBigDigits(num, 304 - num.length * 16, 168);
        return;
      }
      text('TOTAL ' + tally.total, 296, 186, 9, '#ffd166', 'right');
    }

    /* Flying damage numbers: the etim numerals at the captured hit size
     * (24x23, drawn 1:1 off the 24x24 atlas cells). */
    function drawPopups() {
      popups = popups.filter(p => p.t < p.life);
      for (const p of popups) {
        const rise = Math.min(p.t, 20) * 0.8;
        g.save();
        g.globalAlpha = p.t > p.life - 12 ? (p.life - p.t) / 12 : 1;
        const num = String(p.text).replace(/^-/, '');
        if (hudOk() && /^\d+$/.test(num)) {
          hudBigDigits(num, p.x - num.length * 12, p.y - rise - 12, 24, 23);
        } else {
          text(p.text, p.x, p.y - rise, 12, p.color, 'center');
        }
        g.restore();
        p.t++;
      }
    }

    /* ------------------------------------------------------- per-frame tick */

    function frame() {
      tick++;
      if (mode === 'intro') {
        introT++;
        drawIntro();
        return;
      }
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

      /* The rotating dome camera: retail's idle/terminal phases tick a spin
       * azimuth global +2/frame (FUN_801d0748, phases 0x1e/0x32/0x6e/0xfe -
       * 2 PSX angle units = 2*2pi/4096 rad). Mirrored during the idle
       * presentation modes; a user drag pauses it. */
      if (scene && (mode === 'interval' || mode === 'decided') &&
          !(scene.dragging && scene.dragging())) {
        scene.cam.yaw += 2 * A2R;
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

      if (mode === 'select') {
        drawHeaderChips(state);
        drawCommandCluster(state);
        drawQueueStrip(state);
        drawApPlate(state);
        drawStatusPlate(state);
      } else if (mode === 'playback') {
        drawAttackerChip(state.names[tally ? tally.attacker : 0] || state.names[0]);
        drawFoeChip(state);
        drawApPlate(state);
        drawStatusPlate(state);
        drawTally();
        drawArtsBanner();
      } else if (mode === 'interval') {
        drawStatusPlate(state);
        drawInterval(state);
      } else if (mode === 'decided') {
        drawStatusPlate(state);
      }
      if (meter > 0 && mode === 'playback') drawTimeMeter();
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
      loadRoster, start, commit, confirm, frame, key,
      state: st,
      mode: () => mode,
      selectSub: () => selectSub,
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
