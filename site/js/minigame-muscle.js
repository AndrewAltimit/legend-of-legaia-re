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
 *     the between-LEGS interval panel and the verdict banners.
 *
 * CADENCE: the INTERVAL + score-tally screen is a between-FIGHTS beat, not a
 * between-turns one. A settled turn goes straight back to the command cluster
 * - retail's battle SM writes ctx[6] = 0x14 and re-enters ctx+6 = 0x28 with
 * the arena hub not running at all - so nothing is drawn between turns. Only a
 * finished leg reaches the hub, and even then only a survived one with the
 * course unexhausted (state 0x0A); a lost, run-from or final leg settles. The
 * verdict is the engine's, through `muscle_leg_shows_interval` - the same
 * `leg_boundary_raises_interval` call the native window makes.
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
 * time-meter ramp, the battle seats + battle camera script (the engine's
 * dome surface, shared with the play hosts), the cue id set, the
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
 * screen-element placement table (0x80076C10) plus a live PCSX-Redux packet
 * capture of a dome match (command cluster, enemy art, player HYPER ARTS!!
 * playback - scripts/pcsx-redux/autorun_muscle_hud_capture.lua).
 * The ARTS COMMAND INPUT is packet-pinned end to end (a recomp
 * `gpu_frame_dump` GP0 capture of a live dome input screen + Triangle
 * list; docs/subsystems/minigame-muscle-dome.md "Arts command input"):
 * the High/Left/Right/Low hexagon chips + baked label strips + diamond
 * ends, the maroon input bar with its committed-command pennants, the
 * input-phase AP plate with its gouraud sheen fill, the Triangle-button
 * caption (its green circle is the gap TIM at PROT.DAT 0x7B00), and the
 * Hyper Arts list window (system-UI interior tiles under the retail
 * per-window gouraud, orange sub-palette-15 text/arrows, five rows a
 * page). The flow is capture-pinned too: Attack -> Auto|Command ->
 * direction entry that auto-ends when no command is affordable ->
 * queue review -> Begin|Reselect.
 * STILL FITTED: which traced
 * blip fires on which page event, the KO clip pick (slot 4 of the pinned
 * reaction family), the small art-name caption + hint lines (page aids),
 * the banner speed-line rays (polygonal in retail, procedural here), the
 * SUPER/MIRACLE banner word composition (atlas layout, only the HYPER
 * strip's draw is packet-pinned), the interval panel's caption and key
 * hint (its heading, six tally rows and ringside-still backdrop are the
 * retail emitters'), the glide-in motion
 * (retail slides chips between the element table's two endpoints; the page
 * draws them parked at the arrived endpoint), the pennant spawn anchor +
 * pennant width off the captured 30-cost pitch, the Auto picker (greedy
 * here, unpinned in retail), and the review/confirm screens' geometry
 * (screenshot-read, not packet-pinned). The arts LIST rows show every
 * table art - the page does not model arts learning; retail gates rows on
 * the learned-art constant. Without a disc image the chrome falls back to
 * the old canvas approximation.
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

  /* Ra-Seru names - the retail magic-command chip label per character
   * (capture: Vahn's chip reads "Meta"). */
  const RA_SERU = ['Meta', 'Terra', 'Ozma'];

  function create(api, hudCanvas, glCanvas) {
    const g = hudCanvas.getContext('2d');
    g.imageSmoothingEnabled = false;

    let scene = null;          /* 3D scene (null = text fallback) */
    /* idle|intro|select|playback|interval|decided. `interval` is the arena's
     * BETWEEN-LEGS hub screen (retail state 0x0A); a turn boundary never
     * enters it. */
    let mode = 'idle';
    /* select submode - the retail command flow (recomp phase captures):
     *   menu (0x28 cluster) -> attackmenu (0x78 Auto|Command) ->
     *   input (0x50 direction entry) -> review (0x5a queue shown) ->
     *   confirm (0x6e Begin|Reselect). */
    let selectSub = 'menu';
    let artsPage = -1;         /* Triangle arts list: -1 closed, else page */
    let artsRows = null;       /* muscle_arts_list_json rows (lazy) */
    let confirmSel = 0;        /* confirm menu cursor: 0 Begin, 1 Reselect */
    let magicRows = null;      /* muscle_magic_json rows (lazy, per open) */
    let magicWhy = '';         /* last Ra-Seru refusal name, '' when none */
    let pennantFx = [];        /* committed-pennant glides {cmd,slot,x,y,t,life} */
    let introT = 0;            /* ticks into the intro card */
    let introLive = false;     /* the engine steps the first visit */
    let introPress = false;    /* a press this tick, for the live visit */
    let introFvOk = false;     /* the live visit drew last frame */
    let intervalT = 0;         /* ticks into the INTERVAL + tally screen */
    let intervalHp = [0, 0];   /* [hp, hp_max] the leg ended on (still pick) */
    let tick = 0;
    let banner = null;         /* {text, sub, t, life, cls} */
    let popups = [];           /* {text, x, y, t, life, color} */
    let playQueue = [];        /* remaining round-log events */
    let playT = 0;             /* ticks into the current event */
    let playSeen = false;      /* the surface has started this play-out */
    let playLanded = 0;        /* events landed this play-out */
    let pIdx = 0;              /* player events landed this playback */
    let artsSpans = [];        /* muscle_round_arts_json rows */
    let artsBanner = null;     /* {text, name, t, life} */
    let hpShow = [0, 0];       /* eased HP bar values */
    let lastOpts = null;       /* {char, level, monster} for restart */
    let roster = null;         /* muscle_roster_json rows */
    let meter = 0;             /* round time meter 0..0xC (FUN_801d3444) */
    let tally = null;          /* {attacker, total} - playback damage tally */
    /* The cleared leg's victory caption (FUN_801D8DE8 case 0x59). Retail plays
     * it inside the battle before the hub takes over; the page has no separate
     * victory beat, so the INTERVAL screen carries it - below the tally rows,
     * never over them. */
    let legCaption = '';

    /* --------------------------------------------------- the contest layer
     *
     * A leg is one battle and ends on a KO; the CONTEST is the ladder above
     * it - which (course, round) is staged, what a cleared leg banks, and
     * what the run finally pays. All of that lives in the shared engine
     * kernel (legaia_engine_core::muscle_dome::DomeContest, PROT 0977's
     * FUN_801CEA6C + FUN_801CF870); this page only drives it, exactly as the
     * native play-window does. Nothing here decides a rule.
     *
     * The browser has no save file to read a flag bank out of, so it asks
     * for the gates it wants open and the same kernel rule is applied to it.
     * `unlock` picks the course (its three bits are story flags 0x536 /
     * 0x537 / 0x538); `gates` are the Master course-length flags. */
    let contest = null;        /* muscle_contest_json snapshot, or null */
    let contestFlags = { unlock: 0b001, gates: 0b111 };
    let ladder = null;         /* muscle_course_ladder_json rows (lazy) */
    let settlement = null;     /* the last settled payout, for the card */

    function loadLadder() {
      if (!ladder) {
        try { ladder = JSON.parse(api.muscle_course_ladder_json()); }
        catch (e) { ladder = []; }
      }
      return ladder;
    }

    /* Re-read the contest snapshot. Cheap enough to do per transition. */
    function cst() {
      if (!api.muscle_contest_json) return null;
      try {
        const c = JSON.parse(
          api.muscle_contest_json(contestFlags.unlock, contestFlags.gates));
        contest = c && c.live ? c : null;
        if (c && c.settlement) settlement = c.settlement;
        return contest;
      } catch (e) { contest = null; return null; }
    }

    /* The monster the ladder stages at (course, round). */
    function stagedMonster(course, round) {
      const rows = loadLadder();
      const c = rows.find((r) => r.course === course);
      if (!c || !c.rounds.length) return 0;
      const r = c.rounds[Math.min(round, c.rounds.length - 1)];
      return r ? r.id : 0;
    }

    /* Open a contest on `course` (0 Beginner / 1 Expert / 2 Master). The
     * unlock mask is cumulative because retail's three flags overwrite one
     * seed in order and the highest set one wins. */
    function startContest(course) {
      if (!api.muscle_contest_start) return false;
      contestFlags.unlock = (1 << ((course | 0) + 1)) - 1;
      settlement = null;
      const ok = api.muscle_contest_start(contestFlags.unlock, contestFlags.gates);
      cst();
      return ok;
    }

    /* Hand a finished leg to the contest, then either stage the next leg or
     * settle the run. Returns 'next' | 'settled' | 'none'.
     *
     * 'next' is exactly retail's hub state 0x0A - the between-legs INTERVAL +
     * score-tally screen - and the engine says so, not this file: the verdict
     * comes from `muscle_leg_shows_interval`, the same
     * `leg_boundary_raises_interval` call the native window makes. Only a
     * finished LEG ever asks; a finished turn keeps the leg open and never
     * reaches the arena hub. */
    function reportLeg(survived, outcome, turns, hpMax) {
      if (!api.muscle_report_leg || !contest) return 'none';
      api.muscle_report_leg(
        !!survived, outcome | 0, turns | 0, hpMax | 0,
        contestFlags.unlock, contestFlags.gates);
      const shows = api.muscle_leg_shows_interval
        ? !!api.muscle_leg_shows_interval() : null;
      cst();
      if (shows === true) return 'next';
      if (shows === false) {
        if (api.muscle_contest_settle) {
          api.muscle_contest_settle(contestFlags.unlock, contestFlags.gates, false);
        }
        cst();
        return 'settled';
      }
      if (!contest || contest.over) {
        if (api.muscle_contest_settle) {
          api.muscle_contest_settle(contestFlags.unlock, contestFlags.gates, false);
        }
        cst();
        return 'settled';
      }
      return 'next';
    }

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
    const SHEET_NAMES = ['widget', 'font', 'atlas', 'banner', 'hub0', 'hub1', 'button', 'still'];

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
    function blit(src, pal, u, v, w, h, dx, dy, dw, dh, abr, rgb) {
      const s = sheet(src, pal);
      if (!s) return false;
      /* A hub quad's Gouraud colour (`texel * c / 128`) - its fade level. */
      const mod = rgb ? window.LegaiaUtil.modulatedSprite(s, u, v, w, h, rgb[0], rgb[1]) : null;
      withAbr(g, abr, (sub) => (mod && !sub)
        ? g.drawImage(mod, 0, 0, w, h, dx * 2, dy * 2, (dw || w) * 2, (dh || h) * 2)
        : g.drawImage(sub ? hubSilhouette(s) : s, u, v, w, h,
          dx * 2, dy * 2, (dw || w) * 2, (dh || h) * 2));
      return true;
    }
    /* PSX semi-transparency for a hub quad's `abr` (null = opaque): 0 is
     * `B/2 + F/2`, 1 `B + F`, 3 `B + F/4` - canvas composites (a transparent
     * texel has alpha 0 and adds nothing). 2 is `B - F`, which a 2D canvas has
     * no composite for: `draw` receives `true` and must hand in the sprite's
     * black silhouette (`hubSilhouette`) instead. Every ABR-2 hub quad samples
     * one of the all-white knockout palettes the variant-2 emitter bump
     * selects (7 under 6, 9 under 8, ...), and the arena uploads those
     * STP-set, so `B - white` clamps to black under every texel - which is the
     * silhouette, exactly, at full fade. */
    function withAbr(g, abr, draw) {
      if (abr == null) return draw(false);
      if (abr === 2) return draw(true);
      g.save();
      if (abr === 0) g.globalAlpha *= 0.5;
      else {
        g.globalCompositeOperation = 'lighter';
        if (abr === 3) g.globalAlpha *= 0.25;
      }
      try { return draw(false); } finally { g.restore(); }
    }
    /* The black silhouette of a decoded sheet (every opaque texel -> black,
     * alpha kept), cached per sheet canvas. */
    const hubSilhouettes = new WeakMap();
    function hubSilhouette(c) {
      if (!c) return c;
      let s = hubSilhouettes.get(c);
      if (!s) {
        s = document.createElement('canvas');
        s.width = c.width; s.height = c.height;
        const sg = s.getContext('2d');
        sg.drawImage(c, 0, 0);
        sg.globalCompositeOperation = 'source-in';
        sg.fillStyle = '#000';
        sg.fillRect(0, 0, s.width, s.height);
        hubSilhouettes.set(c, s);
      }
      return s;
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
     * pen stepped by the per-glyph advance (capture-matched). `palOver`
     * selects another CLUT-bank sub-palette (15 = the arts-list orange). */
    function hudText(s, x, y, palOver) {
      const pal = palOver != null ? palOver : hudMeta.pieces.font_pal;
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
    function hudDigits(str, x, y, palOver) {
      const d = hudMeta.pieces.atlas_digits;
      const sl = hudMeta.pieces.slash;
      let pen = x;
      for (const c of str) {
        if (c === '/') {
          blit(0, sl.pal, sl.r[0], sl.r[1], sl.r[2], sl.r[3], pen, y - 2);
          pen += 8;
        } else if (c >= '0' && c <= '9') {
          blit(2, palOver != null ? palOver : d.pal,
            d.x0 + (c.charCodeAt(0) - 48) * d.cell, d.v, 8, d.h, pen, y);
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
    /* A whole PROT 0977 hub screen, placed by retail rather than by us.
     * The engine runs the overlay's own quad emitters (FUN_801D050C /
     * FUN_801D08EC / FUN_801D1308) over the draw list recovered from that
     * entry's call sites, so both the extent and the screen seat of every
     * piece are disc-derived; the page only submits the resulting rects.
     * `screen`: 0 intro card, 1 title art, 2 INTERVAL, 3 ROUND banner,
     * 4 score tally. Returns the number of quads drawn (0 = unavailable,
     * so the caller can fall back to its procedural text). */
    const hubQuadCache = new Map();
    /* Retail's own fade / hold envelope for a hub screen, out of the shared
     * `muscle_dome::HubScreen` kernel the native window ticks - so this page
     * cannot pick a frame count or a brightness of its own. `screen`:
     * 0 intro card, 1 ROUND banner, 2 opponent card, 3 INTERVAL.
     * Returns {brightness, stage, done, total}; `brightness` is the emitter
     * argument (0..0x80, where 0x80 is neutral - 0x100 draws at double). */
    const hubEnvCache = new Map();
    const hubEnvTotals = {};
    function hubEnv(screen, t) {
      const fallback = { brightness: 0x80, stage: 1, done: false, total: 0 };
      if (!api || !api.muscle_hub_screen_json) return fallback;
      /* The envelope is monotone and finishes, so past its total every tick
       * answers the same - clamp so a screen the player leaves up does not
       * grow the cache or the replay. */
      const tot = hubEnvTotals[screen] || 0;
      const tc = tot > 0 ? Math.min(t | 0, tot) : (t | 0);
      const key = screen + ':' + tc;
      let v = hubEnvCache.get(key);
      if (v === undefined) {
        try {
          v = JSON.parse(api.muscle_hub_screen_json(screen, tc, 0));
          if (!hubEnvTotals[screen] && v.total) hubEnvTotals[screen] = v.total;
        } catch (e) { v = fallback; }
        hubEnvCache.set(key, v);
      }
      return v;
    }

    function hubQuads(screen, arg, brightness) {
      if (!api || !api.muscle_hub_quads_json) return 0;
      const key = screen + ':' + (arg | 0) + ':' + (brightness | 0);
      /* Screen 4's numerals come from the LIVE contest, so it must not be
       * memoised - a cached tally would freeze on the first leg's rows. */
      let m = screen === 4 ? undefined : hubQuadCache.get(key);
      if (m === undefined) {
        try {
          m = JSON.parse(api.muscle_hub_quads_json(screen, arg | 0, brightness | 0));
        } catch (e) {
          m = { ok: false };
        }
        if (screen !== 4) hubQuadCache.set(key, m);
      }
      if (!m.ok || !m.quads || !m.quads.length) return 0;
      let n = 0;
      for (const q of m.quads) {
        if (blit(q.sheet, q.pal, q.u, q.v, q.w, q.h, q.x, q.y, q.dw, q.dh, q.abr, q.rgb)) n++;
      }
      return n;
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

    /* --------------------------------------------------------- 3D scene
     *
     * The dome's 3D is the engine's own surface
     * (legaia_engine_core::muscle_dome_scene::MuscleDomeSurface, through
     * muscle_surface_*): the fighter and the opponent on the battle
     * formation seats, the play-out choreography (the closing walk, the
     * swings, the flinch and the knockdown) and the battle camera script -
     * the same kernel the native window and the browser play page draw a
     * dome leg with, so all three hosts frame a fight alike. The page only
     * names which selection screen it has up (`selectCode`), because it
     * drives its own command flow. */
    function selectCode() {
      if (mode !== 'select') return 0;
      if (selectSub === 'attackmenu') return 2;
      if (selectSub === 'confirm') return 0;
      return 1;
    }

    function uploadSurface(s, gen) {
      s.renderer.uploadVram(api.muscle_surface_vram());
      s.renderer.uploadMesh(
        api.muscle_surface_positions(), api.muscle_surface_uvs(),
        api.muscle_surface_cba_tsb(), api.muscle_surface_indices(),
        api.muscle_surface_flat_rgba());
      s.gen = gen;
    }

    function buildSurfaceScene() {
      if (!glCanvas || !window.TmdRenderer) return null;
      if (typeof api.muscle_surface_frame !== 'function') return null;
      const gen = api.muscle_surface_frame(selectCode(), 0);
      if (gen < 0) return null;
      const renderer = new window.TmdRenderer(glCanvas);
      /* Two-pass PSX semi-transparency for the shell's ABE lamp-glow prims
       * (ABR mode 1, additive) - the legacy single pass draws them opaque. */
      renderer.semiTwoPass = true;
      const s = { renderer, gen: -1 };
      uploadSurface(s, gen);
      return s;
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

    /* One frame of the dome surface: step it (the choreography and the
     * camera advance one tick per call), re-read the buffers on a new
     * generation, and draw under the battle camera's matrix. */
    function renderScene() {
      const s = scene;
      if (!s) return;
      const gen = api.muscle_surface_frame(selectCode(), 0);
      if (gen < 0) return;
      if (gen !== s.gen) uploadSurface(s, gen);
      const r = s.renderer;
      r.updatePositions(api.muscle_surface_positions());
      const c = r.canvas || glCanvas;
      const vp = api.muscle_surface_vp(c.width / Math.max(c.height, 1));
      r.mvpOverride = vp.length === 16 ? Float32Array.from(vp) : null;
      r.render(0, 0, 1, 0, 0, [0, 0, 0], 1);
      r.mvpOverride = null;
    }

    /* The play-out beat the surface is on (`muscle_surface_beat_json`):
     * `{kind, play, attacker, at, len}` or null. */
    function surfaceBeat() {
      if (!scene || typeof api.muscle_surface_beat_json !== 'function') return null;
      try { return JSON.parse(api.muscle_surface_beat_json()); }
      catch (e) { return null; }
    }

    /* -------------------------------------------------------- contest flow */

    /* Start a contest. opts = {char, level, monster}; the RNG seed is drawn
     * fresh per contest so replays differ (pass opts.seed to pin one). */
    function start(opts) {
      lastOpts = Object.assign({ char: 0, level: 30, monster: 0 }, opts || {});
      /* A fresh `start` opens a fresh contest unless one is mid-ladder and
       * the caller asked to continue it (`opts.continueRun`), which is what
       * leg-to-leg progression uses. */
      if (!lastOpts.continueRun || !contest) {
        startContest(lastOpts.course | 0);
      }
      let monster = lastOpts.monster | 0;
      /* Which foe is the CONTEST's to say: the ladder names one per
       * (course, round) and retail's `FUN_801D1510` stages exactly that. An
       * explicit `monster` is a page override for the free-play picker. */
      if (contest && !lastOpts.pinMonster) {
        const staged = stagedMonster(contest.course, contest.round);
        if (staged) { monster = staged; lastOpts.monster = staged; }
      }
      if (!monster) {
        const r = loadRoster();
        monster = r.length ? r[0].id : 1;
        lastOpts.monster = monster;
      }
      const seed = (lastOpts.seed != null ? lastOpts.seed
        : (typeof api.minigame_seed === 'function' ? api.minigame_seed('muscle')
          : (Date.now() & 0x7fffffff))) >>> 0;
      if (!api.muscle_start_vs(lastOpts.char, lastOpts.level, monster, seed)) {
        return false;
      }
      /* The surface re-seats the pair and re-reads its buffers itself when
       * the opponent or the fighter changes (a new generation). */
      if (!scene) {
        try { scene = buildSurfaceScene(); }
        catch (e) { scene = null; }
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
      /* The live first visit (the play hosts' `FirstVisitHub`): a press ends
       * a card hold early and the announcer lines play. */
      introLive = !!(api && api.muscle_first_visit_reset && api.muscle_first_visit_step);
      if (introLive) { try { api.muscle_first_visit_reset(); } catch (e) { introLive = false; } }
      introPress = false;
      introFvOk = false;
      intervalT = 0;
      selectSub = 'menu';
      artsPage = -1;
      artsRows = null;      /* re-read: the fighter may have changed */
      pennantFx = [];
      confirmSel = 0;
      legCaption = '';
      return true;
    }

    /* Leave the intro card for this leg's command menu. The banner counts
     * the CONTEST's round, not the battle turn - a leg can run many turns
     * and it is still one round of the ladder. */
    function beginSelect() {
      mode = 'select';
      syncMenu();
      /* A new turn re-prices the Ra-Seru list off the live gauge. */
      magicRows = null;
      magicWhy = '';
      const n = contest ? contest.round + 1 : 1;
      /* The ROUND banner's life is retail's own envelope length - arms
       * 0x15 / 0x16, `HubScreen::opponent_card` - not a page constant; its
       * brightness comes from the same envelope below. */
      setBanner('ROUND ' + n, null, hubEnv(2, 0).total || 70);
    }

    /* The selection is the engine's command flow (`muscle_select`, the
     * session's `select_input` - the call the play hosts' world tick makes):
     * the round prompt, the ring, Auto | Command, the direction entry and
     * its review, the Ra-Seru list and Begin | Reselect, every screen's
     * rule the battle session's. The page only turns keys into pad bits,
     * reads back which screen is up (`muscle_menu_json`) and draws it. It
     * used to run its own copy of that flow over the low-level commit calls
     * - no round prompt, Spirit fighting on the spot, its own screen order. */
    const SELECT_BIT = { left: 1, right: 2, up: 4, down: 8, confirm: 16, back: 32, triangle: 64 };
    function syncMenu() {
      let m = null;
      try { m = JSON.parse(api.muscle_menu_json()); } catch (e) { m = null; }
      if (!m || !m.screen) return;
      if (m.screen === 'magic' && selectSub !== 'magic') magicRows = readMagicRows();
      selectSub = m.screen;
      confirmSel = m.cursor | 0;
      artsPage = m.list_page;
    }
    /* A hand slot's direction, pressed (the page's 1-4 shortcuts). */
    function commit(slot) {
      const hand = st().hand || [];
      const dir = hand[slot] ? (CMD[hand[slot].cmd] || {}).dir : null;
      if (dir && mode === 'select') selectPress(dir);
    }
    function selectPress(name) {
      const bit = SELECT_BIT[name];
      if (!bit) return;
      const before = st();
      const ev = api.muscle_select(bit);
      syncMenu();
      const state = st();
      /* A direction that went onto the AP gauge glides its pennant in. */
      const q0 = (before.queue && before.queue[0]) || [];
      const q1 = (state.queue && state.queue[0]) || [];
      if (q1.length > q0.length) spawnPennant(q1[q1.length - 1], q1.length - 1, state);
      if (q1.length < q0.length) pennantFx = [];
      if (ev === 'cursor') playCue('cursor', 0.4);
      else if (ev === 'confirm' || ev === 'fight') playCue('confirm', 0.5);
      else if (ev === 'refused') {
        magicWhy = selectSub === 'magic' ? 'not_enough_mp' : '';
        playCue('blip', 0.3);
      }
      if (ev !== 'refused') magicWhy = '';
      if (ev === 'fight') fight();
      else if (ev === 'run') {
        /* Run on the round prompt: the leg is given up, as the play hosts
         * report it (the contest settles as a run). */
        mode = 'decided';
        reportLeg(false, 0, state.turn, state.hp_max[0]);
        setBanner('RAN', 'the contest is given up — SPACE to start again', 100000, 'bad');
      }
    }

    /* The learned-arts rows the Triangle list pages through (SCUS
     * arts-name table: name / arrow string / AP). */
    function artsList() {
      if (artsRows === null) {
        try { artsRows = JSON.parse(api.muscle_arts_list_json()); }
        catch (e) { artsRows = []; }
      }
      return artsRows;
    }

    /* Close selection and play the round out. */
    /* The fighter's Ra-Seru rows, priced by the shared session (the cost
     * after the accessory MP-saver bits, which is what the arm charges). */
    function readMagicRows() {
      if (!api.muscle_magic_json) return [];
      try { return JSON.parse(api.muscle_magic_json()); }
      catch (e) { return []; }
    }

    /* Retail's Ra-Seru list over the ring - the port's stand-in for the
     * phase-0x46 screen, whose piece decomposition is not pinned. */
    function drawMagicList(state) {
      const rows = magicRows || (magicRows = readMagicRows());
      const cur = state.magic_cursor | 0;
      const x = 176, y = 24, w = 132, h = Math.max(40, 22 + rows.length * 14);
      g.fillStyle = 'rgba(10,16,32,0.86)';
      g.fillRect(x * 2, y * 2, w * 2, h * 2);
      g.strokeStyle = '#7d8ba8';
      g.lineWidth = 2;
      g.strokeRect(x * 2, y * 2, w * 2, h * 2);
      text('Ra-Seru', x + 8, y + 12, 8, '#ffd98a', 'left', '');
      text('MP ' + (state.mp ? state.mp[0] : 0), x + w - 8, y + 12, 7, '#8fe3d6', 'right', '');
      rows.forEach((r, i) => {
        const ry = y + 26 + i * 14;
        const on = i === cur;
        const ink = !r.affordable ? '#6b7080' : (on ? '#ffffff' : '#c7cede');
        if (on) text('>', x + 4, ry, 8, ink, 'left', '');
        text(r.name, x + 16, ry, 8, ink, 'left', '');
        text(String(r.mp), x + w - 8, ry, 8, ink, 'right', '');
      });
      if (!rows.length) {
        text('(no Seru learned)', x + 16, y + 26, 7, '#8b93a5', 'left', '');
      }
      const why = magicWhy === 'not_enough_mp' ? 'Not enough MP' : '';
      text(why || '↑↓ pick · SPACE cast · ESC back',
        x + w / 2, y + h - 6, 6, why ? '#ff9d9d' : '#aeb6c4', 'center', '');
    }

    function fight() {
      magicRows = null;
      /* Begin closed both fighters' selections inside the engine's flow;
       * the turn resolves through the shared kernel. */
      api.muscle_resolve();
      playQueue = JSON.parse(api.muscle_round_log_json());
      /* The committed queue resolved through the character's real arts
       * tables (SCUS combo strings + curated kind labels) - the spans the
       * retail arts banner covers during playback. */
      try { artsSpans = JSON.parse(api.muscle_round_arts_json()); }
      catch (e) { artsSpans = []; }
      playT = 0;
      playSeen = false;
      playLanded = 0;
      pIdx = 0;
      banner = null;
      artsBanner = null;
      tally = null;
      artsPage = -1;
      pennantFx = [];
      mode = 'playback';
    }

    /* Pad-shaped input from the page: left/right/up/down/back/triangle. */
    function key(name) {
      if (mode === 'intro') { if (introLive && introFvOk) introPress = true; else beginSelect(); return; }
      if (mode !== 'select') return;
      selectPress(name);
    }

    /* SPACE / Confirm: advances whatever the current presentation mode is. */
    function confirm() {
      const state = st();
      if (!state.live) { if (lastOpts) start(lastOpts); return; }
      if (mode === 'intro') {
        /* Retail skips only the two card holds, not the whole visit. */
        if (introLive && introFvOk) introPress = true; else beginSelect();
      } else if (mode === 'select') {
        selectPress('confirm');
      } else if (mode === 'playback') {
        /* Skip: settle every pending event instantly. */
        while (playQueue.length) applyEvent(playQueue.shift(), true);
        artsBanner = null;
        finishPlayback();
      } else if (mode === 'interval' || mode === 'decided') {
        /* A contest still mid-ladder stages its NEXT leg rather than
         * restarting: the ladder has already advanced, so `continueRun`
         * keeps it and lets `start` read the staged foe off it. */
        if (lastOpts) {
          const opts = Object.assign({}, lastOpts,
            { continueRun: !!contest, pinMonster: false, monster: 0 });
          start(opts);
        }
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
      /* The play-out's damage numerals belong to the play-out: the last one
       * lands 34 ticks before this and lives 46, so without this it rode
       * over the command cluster / the INTERVAL tally that follows. Retail
       * shows neither screen with a hit numeral still up - the INTERVAL
       * screen is the arena hub, drawn after the battle has ended. */
      popups = [];
      if (state.phase === 'turn_over') {
        /* A TURN ended, not a fight. Retail's battle SM writes ctx[6] = 0x14
         * and re-enters its own command cluster (ctx+6 = 0x28) - the arena hub
         * is not even running, so there is no screen between turns. Straight
         * back to the cluster, no beat, no keypress. */
        api.muscle_next_turn();
        mode = 'select';
        pennantFx = [];
        syncMenu();
      } else if (state.phase === 'won' || state.phase === 'lost') {
        mode = 'decided';
        if (state.phase === 'won') {
          /* Retail's own victory banner, composed the way retail composes
           * it: the winning fighter's lead-in line from the PROT 0898
           * victory-message table, the reward spell's name, then the fixed
           * suffix - FUN_801D8DE8 case 0x59's three-part assembly, whose
           * standalone twin FUN_801DBA90 the engine decodes. Falls back to
           * the spell name alone when the overlay strings don't resolve. */
          let sub = '';
          try {
            const b = JSON.parse(api.muscle_reward_banner_json());
            if (b && b.ok && b.text) sub = b.text;
          } catch (e) { sub = ''; }
          if (!sub) {
            const spell = api.muscle_spell_name ? api.muscle_spell_name(state.reward_spell) : '';
            sub = spell ? state.names[0] + ' — ' + spell : '';
          }
          /* The caption names a spell; it awards nothing. What the leg is
           * worth is the CONTEST's score cell, banked below. */
          const step = reportLeg(true, 0, state.turn, state.hp_max[0]);
          if (step === 'next') {
            /* THE fight ended and the ladder carries on: this - and only this
             * - is retail's hub state 0x0A, the between-legs INTERVAL +
             * score-tally screen, which carries the verdict itself as the six
             * count-up rows. The victory caption is the BATTLE's, so it plays
             * as a short beat over the KO and expires into the hub screen. */
            mode = 'interval';
            intervalT = 0;
            /* The leg's closing HP picks the ringside still the hub shows
             * (int.tim, or int2.tim below half HP) - latched here, as
             * retail's loader reads it once at the battle end. */
            intervalHp = [state.hp[0], state.hp_max[0]];
            /* A fresh INTERVAL screen rolls its lanes again, so the engine's
             * "already voiced up to step N" high-water mark has to drop with
             * the tick it counts. */
            if (window.MgSpu) MgSpu.tallyVoiceReset(api);
            banner = null;
            legCaption = sub;
            playCue('confirm', 0.5);
          } else {
            setBanner('COURSE CLEARED!',
              (settlement ? settlement.score + ' coins paid' : 'contest over') +
              (settlement && settlement.prize ? '  ·  War God Icon!' : '') +
              ' — SPACE to start again', 100000, 'good');
          }
        } else {
          reportLeg(false, 0, state.turn, state.hp_max[0]);
          setBanner('YOU LOSE',
            (settlement
              ? 'half the run banked: ' + settlement.score + ' coins'
              : '') + ' — SPACE to start again', 100000, 'bad');
        }
      } else {
        mode = 'select';
        syncMenu();
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
    /* The hub's whole first visit (FirstVisitHub): intro strip, the brick
     * wall rising under it with its shade, the course-title zoom and the
     * course card (FUN_801D042C), then the wall draining. Rows come back
     * placed and in paint order from the kernel both play hosts draw with;
     * the page hands over to its ROUND banner - retail's arms 0x15/0x16 -
     * when the walk reaches them. */
    /* The first visit's backdrop shade (FUN_801D1610): an untextured Gouraud
     * quad drawn with ABR 2 (tpage 0x46, `B - F`), top corners 0x64, bottom 0.
     * A 2D canvas has no subtractive composite, so the page does the equation
     * itself on the pixels already down: per canvas row, F is the ramp at that
     * row's logical y and every channel drops by F, clamped at 0. `sx` / `sy`
     * map logical 320x240 pixels to canvas pixels. Returns false when the
     * canvas cannot be read back (the caller then falls back to black bands). */
    function subtractShade(g, q, sx, sy) {
      const cw = g.canvas.width, ch = g.canvas.height;
      const x0 = Math.max(0, Math.round(q.x * sx)), y0 = Math.max(0, Math.round(q.y * sy));
      const x1 = Math.min(cw, Math.round((q.x + q.dw) * sx));
      const y1 = Math.min(ch, Math.round((q.y + q.dh) * sy));
      const w = x1 - x0, h = y1 - y0;
      if (w <= 0 || h <= 0 || !(q.dh > 0)) return true;
      let img;
      try { img = g.getImageData(x0, y0, w, h); } catch (e) { return false; }
      const d = img.data;
      for (let r = 0; r < h; r++) {
        const ly = (y0 + r + 0.5) / sy - q.y;
        const f = Math.round(q.top + (q.bottom - q.top) * (ly / q.dh));
        if (f <= 0) continue;
        for (let i = r * w * 4, e = i + w * 4; i < e; i += 4) {
          d[i] = d[i] > f ? d[i] - f : 0;
          d[i + 1] = d[i + 1] > f ? d[i + 1] - f : 0;
          d[i + 2] = d[i + 2] > f ? d[i + 2] - f : 0;
        }
      }
      g.putImageData(img, x0, y0);
      return true;
    }

    const firstVisitCache = new Map();
    function drawFirstVisit() {
      if (!api || !api.muscle_first_visit_json || !contest) return null;
      const key = introT + ':' + contest.course + ':' + contest.round;
      let m = introLive ? undefined : firstVisitCache.get(key);
      if (introLive) {
        try {
          m = JSON.parse(api.muscle_first_visit_step(introPress,
            contest.course | 0, (contest.round | 0) + 1));
        } catch (e) { m = { ok: false }; }
        introPress = false;
        introFvOk = !!m.ok;
      } else if (m === undefined) {
        try {
          m = JSON.parse(api.muscle_first_visit_json(introT, contest.course | 0, (contest.round | 0) + 1));
        } catch (e) { m = { ok: false }; }
        firstVisitCache.set(key, m);
      }
      if (!m.ok) return null;
      for (const q of m.rows || []) {
        if (q.shade) {
          /* Retail's subtractive Gouraud ramp (ABR 2, `B - F`) on the
           * pixels already down; black bands only when the canvas cannot
           * be read back. */
          if (subtractShade(g, q, 2, 2)) continue;
          const bands = 16;
          for (let b = 0; b < bands; b++) {
            const y0 = q.y + Math.floor(q.dh * b / bands);
            const y1 = q.y + Math.floor(q.dh * (b + 1) / bands);
            const f = q.top + (q.bottom - q.top) * ((b + 0.5) / bands);
            if (y1 <= y0 || f <= 0) continue;
            g.fillStyle = 'rgba(0,0,0,' + (f / 255) + ')';
            g.fillRect(q.x * 2, y0 * 2, q.dw * 2, (y1 - y0) * 2);
          }
          continue;
        }
        blit(q.sheet, q.pal, q.u, q.v, q.w, q.h, q.x, q.y, q.dw, q.dh, q.abr);
      }
      return m;
    }

    function drawIntro() {
      g.fillStyle = '#000';
      g.fillRect(0, 0, hudCanvas.width, hudCanvas.height);
      if (hudOk()) {
        const fv = drawFirstVisit();
        if (fv) {
          if (fv.arm === 'round' || fv.done) beginSelect();
          return true;
        }
      }
      const env = hubEnv(0, introT);
      g.save();
      /* The strip's own fade counter drives the emitter; the canvas alpha
       * stays 1 so the two hosts modulate the same way. */
      g.globalAlpha = 1;
      if (hudOk() && hubQuads(0, 0, env.brightness)) {
        /* drawn - retail seats the strip centred on (160, 120) */
      } else {
        g.globalAlpha = env.brightness / 0x80;
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
      /* The prompt appears once the strip has reached its hold - retail's
       * arm-1 boundary, not a frame count this page chose. */
      if (env.stage >= 1) {
        text('SPACE', 306, 230, 6, 'rgba(174,182,196,0.7)', 'right', '');
      }
    }

    /* Top-left Begin + fighter-name chips (retail command-input header;
     * capture: gold body starts at x=16 / x=68, plates 20 tall at y=8).
     * `withAttack` adds the third "Attack" chip the attack-input phases
     * carry (packet capture: gold plates at x=8/60/103, y=8). */
    function drawHeaderChips(state, withAttack) {
      if (hudOk()) {
        const w = rChip('Begin', 16, 8, 'gold');
        const w2 = rChip(state.names[0], 24 + w + 8, 8, 'gold');
        if (withAttack) rChip('Attack', 24 + w + 8 + w2 + 16, 8, 'gold');
        return;
      }
      chip(6, 6, 40, 13, 'gold', 'Begin');
      chip(52, 6, Math.max(36, state.names[0].length * 7 + 10), 13, 'gold', state.names[0]);
      if (withAttack) chip(104, 6, 46, 13, 'gold', 'Attack');
    }

    /* Which of retail's three mark emitters, if any, lays over each chip
     * this frame - straight off the shared session's gates, so this page and
     * the native window agree about which command is live. Keyed by chip
     * name; the value is `forbidden` / `blocked` / `sealed` or null. */
    function chipMarks(state) {
      const out = { item: null, attack: null, raseru: null, spirit: null };
      const on = { item: true, attack: true, raseru: true, spirit: true };
      const quad = { item: null, attack: null, raseru: null, spirit: null };
      (state.chips || []).forEach((c) => {
        out[c.chip] = c.mark; on[c.chip] = !!c.enabled; quad[c.chip] = c.mark_quad || null;
      });
      return { mark: out, enabled: on, quad };
    }
    /* A chip's mark. The red cross-out X comes placed by the engine's port of
     * its emitter (`mark_quad`, FUN_801DBC30); the page keeps its own seat
     * only for a mark whose emitter is not ported. */
    function drawChipMark(gates, name, fx, fy) {
      const q = gates.quad[name];
      if (q) blit(3, q.pal, q.u, q.v, q.w, q.h, q.x, q.y, q.dw, q.dh);
      else hudWord('red_x', fx, fy);
    }

    /* The retail command cluster: Item on top; Attack + D-pad + Ra-Seru;
     * Spirit below. Anchors + widths are the SCUS element table's arrived
     * glide endpoints (elements 8 / 9 / 0xA / 0xB), the plate/label offsets
     * and the D-pad seat the captured packets. Which chips wear a mark is
     * the session's, not this page's. */
    function drawCommandCluster(state) {
      const raSeru = RA_SERU[state.char] || 'Meta';
      const inAttack = selectSub !== 'menu';
      const gates = chipMarks(state);
      if (hudOk()) {
        const el = (i, dx, dy) => {
          const e = hudMeta.elements[i];
          return e ? { x: e.b[0], y: e.b[1], w: e.w } : { x: dx, y: dy, w: 48 };
        };
        const item = el(8, 204, 34);
        const atk = el(9, 160, 66);
        const ras = el(0xA, 248, 66);
        const spi = el(0xB, 204, 98);
        /* The red cross-out X is retail's course restriction, and nothing
         * else: a command that is merely unavailable keeps a bare plate. */
        rChip('Item', item.x, item.y - 6, 'blue', item.w);
        if (gates.mark.item) drawChipMark(gates, 'item', item.x - 8, item.y - 4);
        /* Attack + the D-pad glyph between it and the Ra-Seru chip. */
        rChip('Attack', atk.x, atk.y - 6, inAttack ? 'gold' : 'blue', atk.w);
        blit(0, hudMeta.pieces.dpad.pal,
          hudMeta.pieces.dpad.r[0], hudMeta.pieces.dpad.r[1], 16, 16,
          (atk.x + atk.w + ras.x - 8) / 2 - 8, atk.y - 4);
        /* Ra-Seru (magic). The label is the `-` retail's element record
         * draws when the member carries none, and no mark rides with it. */
        rChip(gates.enabled.raseru ? raSeru : '-',
          ras.x, ras.y - 6, selectSub === 'magic' ? 'gold' : 'blue', ras.w);
        if (gates.mark.raseru) drawChipMark(gates, 'raseru', ras.x - 8, ras.y - 4);
        /* Spirit - ends selection. */
        rChip('Spirit', spi.x, spi.y - 6, 'blue', spi.w);
        if (!inAttack && !banner) {
          text('←Attack  →Ra-Seru  ↓Spirit  SPACE Begin', 214, 116, 6, '#aeb6c4', 'center', '');
        }
        return;
      }
      chip(196, 20, 60, 13, 'blue', 'Item');
      if (gates.mark.item) crossOut(196, 20, 60, 13);
      chip(150, 48, 54, 14, inAttack ? 'gold' : 'blue', 'Attack');
      dpadGlyph(216, 55, 7);
      chip(228, 48, 50, 14, gates.enabled.raseru ? 'blue' : 'grey',
        gates.enabled.raseru ? raSeru : '-');
      if (gates.mark.raseru) crossOut(228, 48, 50, 14);
      chip(178, 76, 60, 14, 'blue', 'Spirit');
      if (!inAttack && !banner) {
        text('←Attack  →Ra-Seru  ↓Spirit  SPACE Begin', 214, 100, 6, '#aeb6c4', 'center', '');
      }
    }

    /* ---- The retail arts command input (recomp GP0 packet capture) ----
     *
     * Every piece rect / palette / screen seat below is byte-read out of a
     * live dome input screen's captured packet stream (docs/subsystems/
     * minigame-muscle-dome.md "Arts command input"). The direction chips
     * sit at the same screen anchors the status-limb-gating table in
     * arts-command-gauge.md documents. Command id -> label strip. */
    const CMD_CHIP_SEATS = {
      15: { bx: 216, by: 26, label: 'high' },    /* Up */
      12: { bx: 176, by: 58, label: 'left' },
      13: { bx: 256, by: 58, label: 'right' },
      14: { bx: 216, by: 90, label: 'low' },     /* Down */
    };

    function ai() { return hudMeta && hudMeta.arts_input; }

    /* One direction chip: 15-wide gold hexagon caps + 24-wide body, the
     * baked label strip (an FT4 in retail), and the two 9x18 diamond
     * arrows at the pointed ends. */
    function drawCmdChip(cmd) {
      const p = ai();
      const s = CMD_CHIP_SEATS[cmd];
      if (!p || !s) return;
      const c = p.cmd_chip;
      blit(0, c.pal, ...c.cap_l, s.bx - 15, s.by);
      blit(0, c.pal, ...c.body, s.bx, s.by);
      blit(0, c.pal, ...c.cap_r, s.bx + 24, s.by);
      const L = p.cmd_label;
      blit(0, L.pal, L.u, L.v[s.label], L.w, L.h, s.bx, s.by + 4);
      blit(0, p.chip_diamond_l.pal, ...p.chip_diamond_l.r, s.bx - 9, s.by + 4);
      blit(0, p.chip_diamond_r.pal, ...p.chip_diamond_r.r, s.bx + 24, s.by + 4);
    }

    /* The High / Left / Right / Low chip cross + the D-pad glyph (packet:
     * FT4 (220,62)-(235,77) of the pinned 16x16 dpad piece). */
    function drawInputChips() {
      if (!ai()) return;
      for (const cmd of [15, 12, 13, 14]) drawCmdChip(cmd);
      const d = hudMeta.pieces.dpad;
      blit(0, d.pal, d.r[0], d.r[1], 16, 16, 220, 62, 15, 15);
    }

    /* The input bar (the visible AP gauge): pointed left end + tiled body +
     * arrow right end at y=188, maroon sub-palette. Captured at 128 px for
     * a 100-AP pool; other pools scale proportionally (fitted). */
    function drawInputBar(state) {
      const p = ai();
      if (!p) return;
      const pool = state.stats ? state.stats[0].budget_pool : 100;
      const len = Math.max(48, Math.round(128 * pool / 100));
      blit(0, p.bar_end_l.pal, ...p.bar_end_l.r, 0, 188);
      for (let x = 16; x < len - 16; x += 16) {
        const w = Math.min(16, len - 16 - x);
        blit(0, p.bar_body.pal, p.bar_body.r[0], p.bar_body.r[1], w, 18, x, 188);
      }
      blit(0, p.bar_arrow.pal, ...p.bar_arrow.r, len - 18, 188);
    }

    /* One committed-command pennant: 9x18 caps + the 24x18 label strip.
     * Slot x = 7 + the AP cost of everything before it (capture: pitch 30
     * for the 30-cost commands; the cap seat extrapolates for other
     * costs). */
    function drawPennant(cmd, x, y) {
      const p = ai();
      const s = CMD_CHIP_SEATS[cmd];
      if (!p || !s) return;
      blit(0, p.pennant_cap_l.pal, ...p.pennant_cap_l.r, x, y);
      const L = p.cmd_label;
      blit(0, L.pal, L.u, L.v[s.label], L.w, L.h, x + 9, y);
      blit(0, p.pennant_cap_r.pal, ...p.pennant_cap_r.r, x + 33, y);
    }

    /* Bar x seat of committed queue slot `i` (cost-weighted). */
    function pennantSeat(state, i) {
      const hand = state.hand || [];
      const costOf = (cmd) => {
        const h = hand.find(c => c.cmd === cmd);
        return h ? h.cost : 30;
      };
      let x = 7;
      for (let k = 0; k < i; k++) x += costOf(state.queue[0][k]);
      return x;
    }

    /* Spawn the committed pennant near the fighter and glide it into its
     * bar slot (retail spawns it at the fighter's projected position and
     * FUN_801d9bbc glides it in; the spawn anchor here is fitted). */
    function spawnPennant(cmd, slot, state) {
      pennantFx.push({ cmd, slot, t: 0, life: 14, sx: 110, sy: 148 });
    }

    /* Committed pennants: settled ones in the bar, in-flight ones gliding. */
    function drawPennants(state) {
      if (!ai()) return;
      const flying = {};
      pennantFx = pennantFx.filter(f => f.t <= f.life);
      for (const f of pennantFx) flying[f.slot] = f;
      for (let i = 0; i < state.queue[0].length; i++) {
        const cmd = state.queue[0][i];
        const f = flying[i];
        if (f) {
          const k = f.t / f.life;
          const tx = pennantSeat(state, i);
          drawPennant(cmd, Math.round(f.sx + (tx - f.sx) * k),
            Math.round(f.sy + (188 - f.sy) * k));
          f.t++;
        } else {
          drawPennant(cmd, pennantSeat(state, i), 188);
        }
      }
    }

    /* The AP plate at (208,172) - ONE widget with two callers, so they
     * cannot drift: the command menu reads the AP budget, the input screen
     * reads the SPIRIT gauge (the input budget is the bar along the bottom;
     * the plate on the right never drains during entry - packet + RAM
     * capture). Capture decomposition: the 24x16 pointed "AP" label, the
     * 56x16 trough, the 16x16 end box, the 8x16 cap.
     *
     * The meter inside the trough is NOT a sheet tile - the widget sheet
     * carries none. Retail draws it as two 3-px untextured gouraud strips
     * (dark -> gold -> dark sheen) over the pinned span x 235..285, which
     * is what `arts_input.ap_input_fill` hands over. The end box carries
     * the VALUE: the sheet's baked 16x6 "100" tile at a full gauge (the
     * 6-px digit strip has no 3-digit seat, which is why that tile exists),
     * small atlas digits below it. */
    function drawApGaugePlate(value, max) {
      const p = hudMeta.pieces, a = ai();
      const x = 208, y = 172;
      blit(0, p.ap_label.pal, ...p.ap_label.r, x, y);
      blit(0, p.ap_trough.pal, ...p.ap_trough.r, x + 24, y);
      blit(0, p.ap_end.pal, ...p.ap_end.r, x + 80, y);
      blit(0, p.ap_cap.pal, ...p.ap_cap.r, x + 96, y);
      const f = a && a.ap_input_fill;
      const frac = max > 0 ? Math.max(0, Math.min(1, value / max)) : 0;
      if (f && frac > 0) {
        const [lite, dark] = f.rgb;
        const gr = g.createLinearGradient(0, f.rect[1] * 2, 0, (f.rect[1] + f.rect[3]) * 2);
        gr.addColorStop(0, `rgb(${dark[0]},${dark[1]},${dark[2]})`);
        gr.addColorStop(0.5, `rgb(${lite[0]},${lite[1]},${lite[2]})`);
        gr.addColorStop(1, `rgb(${dark[0]},${dark[1]},${dark[2]})`);
        g.fillStyle = gr;
        g.fillRect(f.rect[0] * 2, f.rect[1] * 2,
          Math.round(f.rect[2] * frac) * 2, f.rect[3] * 2);
      }
      const v = Math.max(0, Math.round(value));
      if (v >= 100 && p.gauge_100) {
        const t = p.gauge_100;
        blit(0, t.pal, t.r[0], t.r[1], t.r[2], t.r[3], x + 80, y + 5);
      } else {
        const num = String(v);
        hudDigits(num, x + 95 - num.length * 8, y + 2);
      }
    }

    function drawInputApPlate(state) {
      drawApGaugePlate(state.spirit ? state.spirit[0] : 100, 100);
    }

    /* The Triangle caption: the green Triangle button circle (its own gap
     * TIM at PROT.DAT 0x7B00) + the white battle-font line. Open-list seat
     * (162,154)/(178,156); closed (12,170)/(28,172) - both packet-read. */
    function drawTriCaption(open) {
      const a = ai();
      if (!a || !artsList().length) return;
      const x = open ? 162 : 12, y = open ? 154 : 170;
      blit(6, 0, ...a.tri_button.r, x, y);
      hudText(open ? 'Button: View Next page' : 'Button: View Hyper Arts list',
        x + 16, y + 2);
    }

    /* The Triangle arts-list window at (6,28)-(160,188): system-UI
     * interior tiles under the retail per-window vertical gouraud (0x40
     * top -> 0x88 bottom), gold border strips + corners, five rows per
     * page - art name + AP (orange battle font / atlas digits through
     * sub-palette 15) over the art's arrow string (12x12 atlas glyphs). */
    function drawArtsList(state) {
      const a = ai();
      if (!a || artsPage < 0) return;
      const W0 = 6, Y0 = 28, W1 = 160, Y1 = 188;
      const win = a.list_win;
      /* interior tiles */
      for (let ty = Y0; ty < Y1 - 4; ty += 32) {
        for (let tx = W0; tx < W1; tx += 32) {
          const w = Math.min(32, W1 - tx), h = Math.min(32, Y1 - 4 - ty);
          blit(0, win.pal, win.interior[0], win.interior[1], w, h, tx, ty);
        }
      }
      /* the per-window gouraud modulation (0x40/128 top -> 0x88/128
       * bottom): approximated by a fading black overlay */
      const gr = g.createLinearGradient(0, Y0 * 2, 0, Y1 * 2);
      gr.addColorStop(0, `rgba(0,0,0,${1 - win.grad[0] / 128})`);
      gr.addColorStop(1, 'rgba(0,0,0,0)');
      g.fillStyle = gr;
      g.fillRect(W0 * 2, Y0 * 2, (W1 - W0) * 2, (Y1 - Y0) * 2);
      /* borders */
      for (let tx = W0 + 4; tx < W1 - 4; tx += 24) {
        const w = Math.min(24, W1 - 4 - tx);
        blit(0, win.pal, win.edge_top[0], win.edge_top[1], w, 4, tx, Y0);
        blit(0, win.pal, win.edge_bottom[0], win.edge_bottom[1], w, 4, tx, Y1 - 4);
      }
      for (let ty = Y0 + 4; ty < Y1 - 4; ty += 24) {
        const h = Math.min(24, Y1 - 4 - ty);
        blit(0, win.pal, win.edge_l[0], win.edge_l[1], 4, h, W0, ty);
        blit(0, win.pal, win.edge_r[0], win.edge_r[1], 4, h, W1 - 4, ty);
      }
      blit(0, win.pal, ...win.corner_tl, W0, Y0);
      blit(0, win.pal, ...win.corner_tr, W1 - 4, Y0);
      blit(0, win.pal, ...win.corner_bl, W0, Y1 - 4);
      blit(0, win.pal, ...win.corner_br, W1 - 4, Y1 - 4);
      /* rows */
      const rows = artsList().slice(artsPage * 5, artsPage * 5 + 5);
      const orange = a.arts_text_pal;
      const AR = a.arts_arrows;
      const dirGlyph = { 1: 'left', 2: 'right', 3: 'down', 4: 'up' };
      rows.forEach((row, i) => {
        const y = 36 + 30 * i;
        hudText(row.name, 14, y, orange);
        if (row.ap > 0) {
          const num = String(row.ap);
          hudDigits(num, 152 - num.length * 8, y, orange);
        }
        (row.dirs || []).slice(0, 9).forEach((d, k) => {
          const u = AR.u[dirGlyph[d]];
          if (u != null) blit(2, AR.pal, u, AR.v, AR.w, AR.h, 44 + 12 * k, y + 14);
        });
      });
    }

    /* The Attack sub-menu (phase 0x78): Auto | Command chips around the
     * D-pad glyph at the Attack / Ra-Seru element anchors. */
    function drawAttackMenu() {
      if (!hudOk()) {
        chip(150, 48, 54, 14, 'blue', 'Auto');
        dpadGlyph(216, 55, 7);
        chip(228, 48, 62, 14, 'blue', 'Command');
        return;
      }
      const e9 = hudMeta.elements[9], eA = hudMeta.elements[0xA];
      const atk = e9 ? { x: e9.b[0], y: e9.b[1], w: e9.w } : { x: 160, y: 66, w: 48 };
      const ras = eA ? { x: eA.b[0], y: eA.b[1], w: eA.w } : { x: 248, y: 66, w: 48 };
      rChip('Auto', atk.x, atk.y - 6, 'blue', atk.w);
      blit(0, hudMeta.pieces.dpad.pal,
        hudMeta.pieces.dpad.r[0], hudMeta.pieces.dpad.r[1], 16, 16,
        (atk.x + atk.w + ras.x - 8) / 2 - 8, atk.y - 4);
      rChip('Command', ras.x, ras.y - 6, 'blue', ras.w);
      if (!banner) {
        text('←Auto  →Command  ESC back', 214, 116, 6, '#aeb6c4', 'center', '');
      }
    }

    /* The queue-review / Begin|Reselect confirm (phases 0x5a / 0x6e;
     * screenshot-read geometry - these two screens are not packet-pinned). */
    function drawConfirmMenu(state) {
      if (selectSub === 'confirm') {
        if (hudOk()) {
          rChip('Begin', 96, 84, confirmSel === 0 ? 'gold' : 'blue', 40);
          rChip('Reselect', 176, 84, confirmSel === 1 ? 'gold' : 'blue', 56);
          const w0 = rChip(state.names[0], 16, 164, 'gold');
          rChip('Attack', 24 + w0 + 8, 164, 'gold');
          const fname = state.names[1] || '';
          if (fname) rChip(fname, 304 - hudTextW(fname), 164, 'blue');
        } else {
          chip(88, 78, 48, 14, confirmSel === 0 ? 'gold' : 'blue', 'Begin');
          chip(168, 78, 62, 14, confirmSel === 1 ? 'gold' : 'blue', 'Reselect');
        }
        text('←→ pick · SPACE confirm', 160, 108, 6, '#aeb6c4', 'center', '');
      } else {
        text('SPACE: continue', 240, 160, 6, '#aeb6c4', 'center', '');
      }
    }

    /* Legacy fallback strip (no disc chrome): entered arrows as text. */
    function drawQueueStrip(state) {
      const q = state.queue[0];
      if (!q.length && selectSub === 'menu') return;
      let s = '';
      for (const cmd of q) s += (CMD[cmd] ? CMD[cmd].glyph : '?') + ' ';
      plate(180, 160, 126, 12, 'blue', false);
      text(s || '· · ·', 243, 166, 8, '#ffe9a8', 'center');
      if (selectSub === 'input') {
        text('arrows commit · SPACE fight · ESC back', 306, 152, 6, '#aeb6c4', 'right', '');
      }
    }

    /* The command-menu AP plate - the same widget as the input screen's,
     * reading the AP budget out of its pool. See [drawApGaugePlate]. */
    function drawApPlate(state) {
      const budget = state.budget[0];
      const pool = state.stats ? state.stats[0].budget_pool : budget;
      if (hudOk()) {
        drawApGaugePlate(budget, pool);
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

    /* Opponent name chip: right-aligned blue chip, body ending at x=304.
     * `y` is the seat: the review screen's target-select row (168), or the
     * bar's row (188) during playback - the battle's target plaque
     * (placement record 81), which rises to the status plate's own row and
     * takes it over while the fighter is the one attacking (captured at the
     * HYPER ARTS!! moment; `engine-core::battle_hud::battle_target_plaque`).
     * The playback seat used to be a fitted y=168, which put the plate
     * under the TOTAL tally row (value cells at y=168..183). */
    function drawFoeChip(state, y) {
      const name = state.names[1] || '';
      if (!name) return;
      if (hudOk()) {
        rChip(name, 304 - hudTextW(name), y, 'blue');
        return;
      }
      const w = Math.max(44, name.length * 7 + 12);
      chip(310 - w, y + 8, w, 13, 'blue', name);
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
        g.strokeText(b.name, HUD_W, 132 * 2);
        g.fillStyle = '#ffe9a8';
        g.fillText(b.name, HUD_W, 132 * 2);
      }
      g.restore();
      b.t++;
    }

    /* The round TIME METER (FUN_801d3444): a 0..0xC counter that ramps while
     * the direction-ENTRY phase (`ctx+6 == 0x50`) runs and drains otherwise,
     * mapped to a 160-px bar (`counter * 160 / 12`). The counter comes from
     * the port (`engine-core::muscle_dome::time_meter_step`, through
     * `muscle_tick_time_meter`); only the screen placement is fitted. */
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

    /* Filled horizontal bar in 320x240 screen space (the canvas is 2x). */
    function gaugeBar(x, y, w, h, frac, fill) {
      const f = Math.max(0, Math.min(1, frac || 0));
      g.fillStyle = 'rgba(0,0,0,0.55)';
      g.fillRect(x * 2, y * 2, w * 2, h * 2);
      g.fillStyle = fill;
      g.fillRect(x * 2, y * 2, Math.round(w * f) * 2, h * 2);
      g.strokeStyle = 'rgba(255,255,255,0.35)';
      g.strokeRect(x * 2 + 0.5, y * 2 + 0.5, w * 2, h * 2);
    }

    /* The between-LEGS screen: retail hub state 0x0A, reached only once the
     * fight is over and the ladder has another round to stage. It is not a
     * between-turns screen - a turn boundary keeps the leg open and the arena
     * hub never runs (`MusclePhase::ends_turn`), which is why nothing calls
     * this from `finishPlayback`'s turn arm.
     *
     * The retail composition is two emitter runs off the PROT 0977 sprite
     * table: the 192x32 INTERVAL heading (record 16) and the six count-up rows
     * (`score_tally_quads`) - the four lanes FUN_801D1184 computes, then the
     * running tally and the coin bank they drain into. The native window draws
     * this screen through the same two builders. */
    /* The re-entered hub's backdrop: the party's ringside still (PROT 1221 /
     * 1222, the 320x256 16bpp image the battle end streams into VRAM
     * (384, 0)) as retail's two FUN_801D00F8 quads, at the hub's backdrop
     * level - the engine replays both (`muscle_interval_still_json`: the
     * HubBackdrop arms 0x0A..0x0C beside the INTERVAL envelope). The hub
     * draws no arena, so the frame is black around it. Returns whether the
     * still drew. */
    function drawIntervalStill() {
      if (!api.muscle_interval_still_json || !hudOk()) return false;
      let m = null;
      try {
        m = JSON.parse(api.muscle_interval_still_json(intervalT, intervalHp[0], intervalHp[1]));
      } catch (e) { m = null; }
      if (!m || !m.ok) return false;
      g.fillStyle = '#000';
      g.fillRect(0, 0, 320 * 2, 240 * 2);
      for (const q of m.quads || []) {
        if (!blit(q.sheet, q.pal, q.u, q.v, q.w, q.h, q.x, q.y, q.dw, q.dh, null)) return false;
        /* An opaque packet modulated by its level (`texel * c / 128`): below
         * neutral that is the image darkened toward black. */
        if (typeof q.bright === 'number' && q.bright < 128) {
          g.fillStyle = 'rgba(0,0,0,' + (1 - q.bright / 128) + ')';
          g.fillRect(q.x * 2, q.y * 2, q.dw * 2, q.dh * 2);
        }
      }
      return true;
    }

    function drawInterval(state) {
      drawIntervalStill();
      const env = hubEnv(3, intervalT);
      const heading = hudOk() && hubQuads(2, 0, env.brightness);
      /* Screen 4 takes the interval tick as its argument: the six rows are
       * the score roll `other_game_overlay::ScoreTallyRamp` replays to that
       * tick, the same kernel the native window steps per frame. */
      const rows = hudOk() && hubQuads(4, intervalT, env.brightness);
      /* The tally's own voice. It names no cue id - `FUN_801D1288` resolves a
       * whole voice-attr set per drained lane - so it cannot come out of the
       * offline per-cue PCM path above; it needs the live SPU the engine owns
       * for this page (js/minigame-bgm.js `MgSpu`). Driven off the same screen
       * tick the rows are, and the engine keys only the steps it has not. */
      if (window.MgSpu) MgSpu.tallyVoice(api, intervalT);
      if (heading && rows) {
        if (legCaption) text(legCaption, 160, 200, 7, '#e8ecf2', 'center', '');
        text('SPACE: next round', 160, 214, 8, '#2dcca7', 'center');
        return;
      }
      /* No hub art on this image: a text stand-in with the same six numbers. */
      g.fillStyle = 'rgba(4,6,10,0.82)';
      g.fillRect(30 * 2, 52 * 2, 260 * 2, 124 * 2);
      g.strokeStyle = 'rgba(255,255,255,0.25)';
      g.strokeRect(30 * 2 + 0.5, 52 * 2 + 0.5, 260 * 2, 124 * 2);
      if (!heading) text('INTERVAL', 160, 64, 10, '#ffd166', 'center');
      text('round cleared in ' + state.turn + ' turns',
        160, 78, 6, '#aeb6c4', 'center', '');
      /* Three of the four lanes are HP recovery scaled by max HP, and only
       * `score` is money - a dome LEG pays nothing, the contest pays coins
       * when it settles. */
      const r = contest ? contest.rows : { round: 0, turns: 0, outcome: 0, score: 0 };
      text('round ' + r.round + '  ·  turns ' + r.turns +
        '  ·  outcome ' + r.outcome, 160, 96, 7, '#7798d4', 'center', '');
      text('recovered ' + (r.round + r.turns + r.outcome) + ' HP',
        160, 110, 7, '#2dcca7', 'center', '');
      text('score ' + r.score, 160, 126, 9, '#ffd166', 'center');
      if (contest) {
        text('course ' + (contest.course + 1) +
          '  ·  next round ' + (contest.round + 1) + '/' + contest.length +
          '  ·  banked ' + contest.tally + ' coins',
          160, 144, 7, '#ffd166', 'center', '');
        text('coin bank ' + contest.coins, 160, 158, 7, '#aeb6c4', 'center', '');
      }
      if (legCaption) text(legCaption, 160, 172, 6, '#e8ecf2', 'center', '');
      text('SPACE: next round', 160, 186, 8, '#2dcca7', 'center');
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
      /* The retail banner is FUN_801D02F0: the ROUND word centred on
       * (120, 120) and the round number's glyphs at x=240 (and x=264 for a
       * second digit), every piece drawn twice - variant 1 then variant 2 -
       * which is what gives the word its two-tone edge. The digit glyph is
       * record 1, not the decimal readout's record 9, and its column is
       * digit*24 (FUN_801D15C8). */
      const round = /^ROUND (\d+)$/.exec(banner.text);
      const roundEnv = hubEnv(2, banner.t);
      /* The retail art carries the fade in its own emitter brightness, so the
       * canvas alpha steps out of the way for it - two ramps would compound. */
      if (round) g.globalAlpha = 1;
      if (round && hudOk() && hubQuads(3, parseInt(round[1], 10), roundEnv.brightness)) {
        /* drawn */
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
     * orange numerals, at the battle overlay's own combo-cluster seats
     * (`legaia_engine_vm::battle_value_readout`: COMBO_TOTAL_LABEL_SEAT
     * (216, 170), 16-px value cells ending at x=304 on row 168 - the
     * frame-oracle-pinned seats every battle draws the cluster at; the
     * dome reuses the battle overlay wholesale). */
    function drawTally() {
      if (!tally || !tally.total) return;
      if (hudOk()) {
        const num = String(tally.total);
        hudWord('word_total', 216, 170);
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
      if (mode === 'interval') intervalT++;
      const state = st();

      /* Playback on the surface's own schedule (turn_timeline: the closing
       * walk, one swing per play, the done tail after each attacker's
       * string): each event lands on its swing's first tick and its impact
       * cue on the connect, 12 ticks in. Without a 3D surface, one event
       * every 34 ticks. */
      if (mode === 'playback') {
        const beat = surfaceBeat();
        if (beat) {
          playSeen = true;
          if (beat.kind === 'swing') {
            while (playQueue.length && playLanded <= beat.play) {
              applyEvent(playQueue.shift(), false);
              playLanded++;
              setTimeoutTick(12, () => { if (mode === 'playback') playHit(); });
            }
          }
          playT++;
        } else if (scene && !playSeen && playT < 4) {
          /* The surface steps once per drawn frame; give it the frame that
           * starts the schedule. */
          playT++;
        } else if (!scene && playQueue.length) {
          if (playT === 0) {
            applyEvent(playQueue[0], false);
            setTimeoutTick(12, () => { if (mode === 'playback') playHit(); });
          }
          playT++;
          if (playT >= 34) { playQueue.shift(); playT = 0; }
        } else {
          while (playQueue.length) applyEvent(playQueue.shift(), true);
          finishPlayback();
        }
      }
      runTickTimers();

      /* Round time meter (FUN_801d3444): the ported ramp owns it - it climbs
       * while the DIRECTION-ENTRY phase runs (retail gates on ctx+6 == 0x50)
       * and drains otherwise. An earlier revision here ramped during
       * PLAYBACK, which is the inverse of retail. */
      if (api.muscle_tick_time_meter) {
        api.muscle_tick_time_meter(1);
        meter = st().time_meter | 0;
      }

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

      if (mode === 'select') {
        if (!hudOk()) {
          /* No disc chrome: the legacy approximation. */
          drawHeaderChips(state, selectSub !== 'menu');
          if (selectSub === 'attackmenu') drawAttackMenu();
          else if (selectSub === 'menu') drawCommandCluster(state);
          drawQueueStrip(state);
          drawApPlate(state);
          drawStatusPlate(state);
          if (selectSub === 'confirm') drawConfirmMenu(state);
        } else if (selectSub === 'target') {
          /* Auto's target cursor (the battle picker over the one foe). */
          drawHeaderChips(state, true);
          drawFoeChip(state, 168);
          drawStatusPlate(state);
          if (!banner) text('SPACE target · ESC back', 160, 108, 6, '#aeb6c4', 'center', '');
        } else if (selectSub === 'prompt') {
          /* The round prompt every turn opens on (`0x1E`): Begin | Run. */
          drawHeaderChips(state);
          rChip('Begin', 96, 84, confirmSel === 0 ? 'gold' : 'blue', 40);
          rChip('Run', 176, 84, confirmSel === 1 ? 'gold' : 'blue', 40);
          drawStatusPlate(state);
          if (!banner) text('←→ pick · SPACE confirm', 160, 108, 6, '#aeb6c4', 'center', '');
        } else if (selectSub === 'menu') {
          drawHeaderChips(state);
          drawCommandCluster(state);
          drawApPlate(state);
          drawStatusPlate(state);
        } else if (selectSub === 'magic') {
          drawHeaderChips(state, true);
          drawCommandCluster(state);
          drawMagicList(state);
          drawStatusPlate(state);
        } else if (selectSub === 'attackmenu') {
          drawHeaderChips(state, true);
          drawAttackMenu();
          drawApPlate(state);
          drawStatusPlate(state);
        } else if (selectSub === 'input') {
          /* Retail parks the status plate off-screen here (packet: its
           * draws move to y=230). */
          drawHeaderChips(state, true);
          drawInputChips();
          drawInputBar(state);
          drawPennants(state);
          drawInputApPlate(state);
          drawTriCaption(artsPage >= 0);
          drawArtsList(state);
          if (artsPage < 0 && !banner) {
            text('arrows enter · T arts list · SPACE done · ESC back',
              160, 216, 6, '#aeb6c4', 'center', '');
          }
        } else if (selectSub === 'review' || selectSub === 'confirm') {
          if (selectSub === 'review') {
            drawHeaderChips(state, true);
            drawFoeChip(state, 168);
          } else {
            /* Retail's 0x6e screen keeps a lone Begin chip top-left. */
            if (hudOk()) rChip('Begin', 16, 8, 'gold');
            else chip(6, 6, 40, 13, 'gold', 'Begin');
          }
          drawInputBar(state);
          drawPennants(state);
          drawConfirmMenu(state);
        }
      } else if (mode === 'playback') {
        drawAttackerChip(state.names[tally ? tally.attacker : 0] || state.names[0]);
        /* The bar's row carries one plate at a time, as in any battle's
         * action phase: the opponent's plaque while the fighter attacks it
         * (the readout bar is parked - `battle_readout_bar_slot` opens it
         * only for a party *target*), and the fighter's status plate while
         * the opponent attacks. Retail hides the AP plate during playback. */
        if (tally && tally.attacker === 0) drawFoeChip(state, 188);
        else if (!artsBanner) drawStatusPlate(state);
        drawTally();
        drawArtsBanner();
      } else if (mode === 'interval') {
        /* The arena hub, not the battle: no battle status plate here. */
        drawInterval(state);
      } else if (mode === 'decided') {
        drawStatusPlate(state);
      }
      if (meter > 0) drawTimeMeter();
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
      /* The contest layer (course / round / tally / settlement), so the page
       * can put the ladder and the payout on screen. */
      contest: () => cst(),
      startContest,
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
      /* The battle camera's matrix the last frame drew with (the camera is
       * the engine's battle script; the page no longer orbits its own). */
      camInfo: () => {
        if (!scene || !glCanvas) return null;
        const vp = api.muscle_surface_vp(glCanvas.width / Math.max(glCanvas.height, 1));
        return vp.length === 16 ? { vp: Array.from(vp) } : null;
      },
    };
  }

  return { create, CMD };
})();
