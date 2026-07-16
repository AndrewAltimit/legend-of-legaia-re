/* Noa's dance - the retail presentation layer.
 *
 * Everything drawn here is the game's own art, decoded from the visitor's
 * disc at runtime by `LegaiaMinigames` (crates/web-viewer/src/minigames_dance.rs):
 *
 *   - the HUD page is the PROT 1230 pack's (512,0) TIM through its own
 *     row-500 CLUT strip - score boxes, blue digit font, the beat track,
 *     the READY/GO/FINISH and rating banners;
 *   - every element's cell + palette comes from the dance overlay's own
 *     34-record widget table (rodata at 0x801D46CC, `FUN_801d2f38`), and
 *     every position is the traced emitter's immediate (`dance_layout_json`);
 *   - the dancer faces are the real face-stamp windows: the overlay
 *     animates its dancers by MoveImage-blitting eye/mouth cells inside
 *     per-dancer VRAM strips (`FUN_801d03c4`), and the insets below the
 *     score boxes replay exactly those blits (dancer 0 = Noa's own field
 *     atlas, PROT 0874 §2);
 *   - the sounds are the overlay's own cues (miss 0x210, combo tier
 *     0x202/0x203/0x205, start 0x201) out of its efect.dat descriptor
 *     block (PROT 1228) + sample VAB (PROT 1231), plus the direct-keyed
 *     good-step stings (`FUN_801d3d78`: rand()%3 picks a paired voice),
 *     and the BGM is the real SEQ+VAB pair rendered through the clean-room
 *     SPU.
 *
 * The dancers are the retail cast: the overlay's spawner (FUN_801d0190)
 * names each floor slot's dancer kind in a baked descriptor table - Noa's
 * own field mesh in the centre, the dance-hall scene module's dedicated
 * dancer NPCs left and right - and every clip they play here (the idle,
 * the dance-groove loop, the judge-triggered moves) is a record of that
 * scene's choreography ANM bundle (PROT 1229), exactly the clip the
 * descriptor names for the event (`dance_cast_json`).
 *
 * The dance hall around them is the real one too: retail hosts the minigame
 * inside the dance-hall field scene, and the hall - the raised stage, the
 * yellow/black checkered dance floor, the portrait banners on the walls,
 * the spotlight cones, the speaker / lamp fixtures - is that scene's own
 * environment mesh pack instanced by its .MAP placement + terrain layers,
 * handed over pre-baked as one static mesh in the dancer frame
 * (`dance_env_*`) and drawn in the same call as the posed cast.
 *
 * The two AI dancers are running for real: retail feeds them the same chart
 * through the same judge and award routine the player's presses take
 * (FUN_801d1358 -> FUN_801d4040 -> FUN_801d1af4), so their score boxes climb
 * off their own kind's bonus row and they spend their three triangles on the
 * disc's own schedule. The rules engine simulates them; this layer draws
 * their scores and fires their groovy move when they spend one.
 *
 * The face-stamp insets under the score boxes only appear in the HUD-only
 * fallback (no WebGL / no cast): retail maps those windows onto the 3D
 * dancers' heads, not the screen.
 */
window.MgDance = (function () {
  'use strict';

  const SCALE = 2;                 /* retail 320x240 -> 640x480 canvas */
  const A2R = (Math.PI * 2) / 4096; /* PSX angle units -> radians */

  /* Widget ids, as named in legaia_asset::dance_art (traced draw sites). */
  const W = {
    READY: 0, DIGIT: 1, LV_LABEL: 6, LV_DIGIT: 7, SCORE_BOX: 8,
    MISS: 10, GOOD: 11, GO: 12, NOTE_BASE: 13,
    CAP_L: 16, CAP_R: 17, ARROW: 18,
    COOL: 19, GREAT: 20, FEVER: 21, STAR: 22, FINISH: 23,
    COUNT: [24, 25, 26], BODY: 30, STOCK: 31,
  };
  /* Runtime CLUT swaps (FUN_801d2524): palette column of the row-500 strip. */
  const PAL_TRACK_IDLE = 8, PAL_TRACK_FLASH = 13, PAL_NOTE = 14;

  function create(api, canvas, glCanvas) {
    const g = canvas.getContext('2d');
    g.imageSmoothingEnabled = false;

    let widgets = null;      /* the overlay's widget table */
    let layout = null;       /* traced emitter geometry */
    let castMeta = null;     /* the overlay's cast tables (dance_cast_json) */
    let pages = {};          /* palette -> HUD-page canvas */
    let faces = null;        /* [dancer][pose] -> canvas, + meta */
    let body = null;         /* the 3D dancer-body scene (null = HUD only) */
    let sfx = null;          /* { ctx, buffers, stings } */
    let sfxIds = null;
    let bgmInfo = null;

    /* ---- run-local presentation state ---- */
    let banners = [];        /* {w, x, y, t, life, rise} */
    let poses = [0, 0, 0, 0];   /* face pose per rig (0 = Noa, 1..3 = NPCs) */
    let poseT = [0, 0, 0, 0];
    let blinkT = [40, 90, 140, 190];
    let flashT = 0;          /* track hit flash */
    let intro = null;        /* pre-run count-in: {t} or null */
    let finished = false;

    function rgbaCanvas(bytes, w, h) {
      if (!bytes || bytes.length !== w * h * 4) return null;
      const c = document.createElement('canvas');
      c.width = w; c.height = h;
      c.getContext('2d').putImageData(
        new ImageData(new Uint8ClampedArray(bytes), w, h), 0, 0);
      return c;
    }

    function page(pal) {
      if (!(pal in pages)) pages[pal] = rgbaCanvas(api.dance_hud_page_rgba(pal), 256, 256);
      return pages[pal];
    }

    /* Decode everything once after a disc lands. Returns false when the art
     * pack / widget table did not decode (the caller falls back + says so). */
    function loadAssets() {
      if (!api.dance_art_ready()) return false;
      widgets = JSON.parse(api.dance_widgets_json());
      layout = JSON.parse(api.dance_layout_json());
      /* Pre-warm the palettes the widget table names + the runtime swaps. */
      const pals = new Set([PAL_TRACK_IDLE, PAL_TRACK_FLASH, PAL_NOTE]);
      widgets.forEach(w => pals.add(w.palette));
      pals.forEach(p => page(p));

      /* The face windows: rig 0 = Noa's field atlas, rigs 1..3 = the pack's
       * dancer strips (Mary + the two competitors), every pose pre-composed
       * through the traced MoveImage blits. The floor cast picks its rig by
       * dancer kind (kind = rig, FUN_801d03c4). */
      const meta = JSON.parse(api.dance_face_meta_json());
      faces = { meta, imgs: [] };
      for (let d = 0; d < meta.length; d++) {
        const m = meta[d];
        const per = [];
        if (m && m.ok) {
          for (let p = 0; p < m.poses; p++) {
            per.push(rgbaCanvas(api.dance_face_rgba(d, p), m.w, m.h));
          }
        }
        faces.imgs.push(per);
      }

      sfxIds = null;
      const cues = JSON.parse(api.dance_sfx_json());
      if (cues.length) sfxIds = JSON.parse(api.dance_sfx_cue_ids());
      bgmInfo = JSON.parse(api.dance_bgm_ready_json());
      try {
        castMeta = api.dance_cast_json ? JSON.parse(api.dance_cast_json()) : null;
      } catch (e) { castMeta = null; }

      /* The dancer bodies: Noa's own field-view model plus the two AI dancers,
       * drawn on a WebGL canvas behind the HUD (the same TmdRenderer the Baka
       * duel uses). Absent when no gl canvas was handed in or PROT 0874 didn't
       * decode - the HUD then sits on a neutral ground and the note says so. A
       * WebGL failure must not take the HUD art down with it. */
      try {
        body = buildBodyScene();
      } catch (e) {
        body = null;
      }
      return true;
    }

    /* ------------- 3D dancer bodies (behind the HUD) ------------- */

    /* Assemble the retail floor cast into one posed buffer + camera, the
     * browser twin of minigame-baka.js's fighter scene. Returns null (HUD
     * only) when the cast or the WebGL context aren't available. */
    function buildBodyScene() {
      if (!glCanvas || !window.TmdRenderer) return null;
      if (!api.dance_body_ready || !api.dance_body_ready()) return null;
      const count = api.dance_body_count();
      if (!count) return null;
      const cast = api.dance_cast_json ? JSON.parse(api.dance_cast_json()) : null;
      if (!cast) return null;

      const dancers = [];
      for (let d = 0; d < count; d++) {
        const pos = api.dance_body_positions(d);
        if (!pos.length) return null;
        dancers.push({
          pos,
          uvs: api.dance_body_uvs(d),
          ct: api.dance_body_cba_tsb(d),
          idx: api.dance_body_indices(d),
          oid: api.dance_body_object_ids(d),
          flat: api.dance_body_flat_rgba(d),
          parts: api.dance_body_part_count(d),
          kind: cast.dancers[d] ? cast.dancers[d].kind : 0,
        });
      }

      /* Every choreography clip the descriptor names (0 = idle, 1 = the
       * dance-groove loop, 2.. = the judge-triggered moves), with the retail
       * cursor rate (1/16-frame units per tick, FUN_800204F8). */
      const clip = (d, c, parts) => {
        const dims = api.dance_body_anim_dims(d, c);
        if (!dims[0] || !dims[1]) return null;
        const frames = api.dance_body_pose_frames(d, c, parts);
        if (!frames.length) return null;
        const meta = (cast.dancers[d].clips || [])[c] || {};
        return { frames, frameCount: dims[1], parts, rate: meta.rate || 8 };
      };
      const clips = dancers.map((f, d) => {
        const per = [];
        for (let c = 0; c < (cast.dancers[d].clips || []).length; c++) {
          per.push(clip(d, c, f.parts));
        }
        return per;
      });
      /* Per-dancer playback state: cursor in 1/16-frame units over the
       * current clip; `move` = a one-shot move clip overriding the loop. */
      const anim = dancers.map(() => ({ cursor: 0, move: null }));

      /* The dance hall: the scene's own placed geometry, baked in the same
       * dancer frame (human spawn at the origin). Static - appended after
       * the posed cast so one buffer + one draw renders hall and dancers,
       * and poseInto never touches its vertices. */
      let env = null;
      if (api.dance_env_positions) {
        const ep = api.dance_env_positions();
        if (ep.length) {
          env = {
            pos: ep,
            uvs: api.dance_env_uvs(),
            ct: api.dance_env_cba_tsb(),
            idx: api.dance_env_indices(),
            flat: api.dance_env_flat_rgba(),
          };
        }
      }

      /* Vertical extent of the rest pose, for camera framing. */
      const halfOf = (f, cl) => {
        const c = cl[0] || cl[1];
        if (!c) return 200;
        const out = new Float32Array(f.pos);
        poseInto(out, f.pos, f.oid, c, 0, 0, 0, 0);
        let lo = Infinity, hi = -Infinity;
        for (let i = 0; i < out.length; i += 3) {
          if (out[i] < lo) lo = out[i];
          if (out[i] > hi) hi = out[i];
        }
        return (hi - lo) / 2 || 200;
      };
      const maxHalf = Math.max.apply(null, dancers.map((f, d) => halfOf(f, clips[d])));
      const human = api.dance_body_human_index();
      /* Floor offsets: the overlay's own spawn table (qualifier mode), the
       * human's slot at the origin - retail spacing, not a fitted gap. */
      const humanX = cast.dancers[human] ? cast.dancers[human].x : 0;
      const dx = dancers.map((_, d) =>
        cast.dancers[d] ? (cast.dancers[d].x - humanX) : 0);
      const spread = Math.max.apply(null, dx.map(Math.abs)) || maxHalf;

      /* Combined vertex buffers (left dancer, human, right dancer, then the
       * static hall geometry). */
      const vertBases = [];
      let total = 0;
      for (const f of dancers) { vertBases.push(total); total += f.pos.length / 3; }
      const envBase = total;
      if (env) total += env.pos.length / 3;
      const pos = new Float32Array(total * 3);
      const uvs = new Uint8Array(total * 2);
      const ct = new Uint16Array(total * 2);
      const flat = new Uint8Array(total * 4);
      const idxArr = [];
      for (let d = 0; d < dancers.length; d++) {
        const f = dancers[d], vb = vertBases[d];
        pos.set(f.pos, vb * 3);
        uvs.set(f.uvs, vb * 2);
        ct.set(f.ct, vb * 2);
        flat.set(f.flat, vb * 4);
        for (const ix of f.idx) idxArr.push(ix + vb);
      }
      if (env) {
        pos.set(env.pos, envBase * 3);
        uvs.set(env.uvs, envBase * 2);
        ct.set(env.ct, envBase * 2);
        flat.set(env.flat, envBase * 4);
        for (const ix of env.idx) idxArr.push(ix + envBase);
      }
      const idx = new Uint32Array(idxArr);

      const renderer = new window.TmdRenderer(glCanvas);
      renderer.uploadVram(api.dance_body_vram());
      renderer.uploadMesh(pos, uvs, ct, idx, flat);
      if (env) {
        /* Retail's NCLIP pass culls the hall's away-facing panels - the
         * audience/crowd billboard sits right behind the retail camera spot
         * and only disappears from the shot under the same rule. */
        renderer.cullBackfaces = true;
        renderer.cullFrontFace = 'ccw';
        /* The hall's smoke columns / spotlight glows are ABE prims; the
         * two-pass render draws them additively instead of as grey slabs. */
        renderer.semiTwoPass = true;
      }

      /* Default framing. With the hall baked in, the retail composition:
       * the camera on the audience side of the stage (the -Z half of the
       * hall - the marquee/backdrop wall at +Z reads behind the dancers),
       * pulled back and slightly above so the raised stage edge, the
       * checkered floor, the spotlight cones and the ceiling banners all
       * frame the trio. Without the hall, the old close-up. */
      const defCam = env
        ? { yaw: Math.PI, pitch: 0.24, distance: 2.7 }
        : { yaw: 0.0, pitch: 0.12, distance: 1.9 };
      const scene = {
        renderer, dancers, clips, anim, moves: cast.moves, dx, vertBases,
        base: pos.slice(),      /* pristine object-local vertices */
        out: pos,               /* per-frame posed copy (uploaded buffer) */
        lastBeat: -1,
        rivalTri: {},           /* per-rival triangle stock, to catch a spend */
        env: !!env,
        defCam,
        cam: Object.assign({}, defCam),
        center: [0, -maxHalf * (env ? 1.6 : 0.85), 0],
        radius: spread * 1.15 + maxHalf * 1.05,
        /* Interior framing wants the PSX-like narrow field of view; the
         * default 1.2 rad reads as a fisheye inside the hall. */
        fov: env ? 0.85 : undefined,
        /* World yaw applied to every dancer. The field meshes' rest facing
         * is -Z: on the retail floor they face the audience half of the
         * hall (-Z, where the default camera sits); the hall-less fallback
         * keeps its +Z camera, so there they spin PI to face it. */
        faceYaw: env ? 0 : Math.PI,
      };
      attachOrbit(scene);
      return scene;
    }

    /* Start a one-shot move clip on dancer `d` (a judge-triggered pair from
     * the kind descriptor); it plays through once, then the dance loop
     * resumes. Ignored when that clip didn't decode. */
    function triggerMove(d, clipId) {
      const b = body;
      if (!b || !b.clips[d] || !b.clips[d][clipId]) return;
      b.anim[d].move = clipId;
      b.anim[d].cursor = 0;
    }

    /* Advance dancer `d`'s cursor by its clip rate and return the frame to
     * pose this render. Mirrors the retail clip driver's cursor semantics:
     * cursor is in 1/16-frame units, one-shot moves clamp at the last frame
     * then hand back to the loop, loops wrap. */
    function advance(d, live) {
      const b = body, a = b.anim[d];
      const loopId = live ? 1 : 0;             /* dance-groove loop / idle */
      let clipId = a.move !== null ? a.move : loopId;
      let c = b.clips[d][clipId] || b.clips[d][loopId] || b.clips[d][0];
      if (!c) return { clip: null, frame: 0 };
      const last = c.frameCount * 16 - 1;
      if (a.move !== null && a.cursor >= last) {
        a.move = null;                          /* move done -> loop */
        a.cursor = 0;
        clipId = loopId;
        c = b.clips[d][clipId] || b.clips[d][0];
        if (!c) return { clip: null, frame: 0 };
      }
      const frame = Math.min(a.cursor >> 4, c.frameCount - 1);
      a.cursor += c.rate;
      if (a.move === null && a.cursor > last) a.cursor = 0;
      return { clip: c, frame };
    }

    /* Pose every dancer and render the 3D floor on the gl canvas. The AI
     * dancers are simulated (the rules engine runs retail's chart auto-feed
     * through the same judge), so their bodies play what their own run does:
     * the sequence move for the beat's direction symbol on their own lane, and
     * the groovy move the frame their triangle stock drops. */
    function bodyRender(st, chart) {
      const b = body;
      const live = !!(st && st.live);
      const rivals = (st && st.rivals) || [];
      if (live && chart && b.moves) {
        const beat = st.beat | 0;
        const human = api.dance_body_human_index();
        /* The cast is handed over in floor order (left / centre / right) while
         * the rules engine numbers its dancers in the overlay's spawn-slot
         * order, so pair them by dancer *kind* - the one id both spaces carry. */
        const rivalOf = (d) => rivals.find(r => r.kind === b.dancers[d].kind);
        for (let d = 0; d < b.dancers.length; d++) {
          if (d === human) continue;
          const rv = rivalOf(d);
          if (!rv) continue;
          const lane = Math.min(rv.lane | 0, chart.rows.length - 1);
          /* A rival that just spent a triangle throws its groovy move now -
           * on its own lane, exactly as the judge's return picks the clip. */
          const was = b.rivalTri[d];
          b.rivalTri[d] = rv.triangles;
          if (was !== undefined && rv.triangles < was) {
            triggerMove(d, b.moves.beat[Math.min(lane, 2)]);
          } else if (beat !== b.lastBeat) {
            const row = chart.rows[lane];
            const sym = row ? row[beat % row.length] : 0;
            if (sym === 1) triggerMove(d, b.moves.seq_square[Math.min(lane, 2)]);
            else if (sym === 2) triggerMove(d, b.moves.seq_circle[Math.min(lane, 2)]);
          }
        }
        b.lastBeat = beat;
      } else {
        b.lastBeat = -1;
        b.rivalTri = {};
      }
      for (let d = 0; d < b.dancers.length; d++) {
        const st_ = advance(d, live);
        if (!st_.clip) continue;
        poseInto(b.out, b.base, b.dancers[d].oid, st_.clip, st_.frame,
                 b.vertBases[d], b.dx[d], b.faceYaw);
      }
      b.renderer.updatePositions(b.out);
      b.renderer.render(b.cam.yaw, b.cam.pitch, b.cam.distance,
                        0, 0, b.center, b.radius, b.fov);
    }

    /* Drag-to-orbit on the gl canvas (the HUD canvas over it is
     * pointer-events:none, so drags reach here); dblclick re-frames. */
    function attachOrbit(scene) {
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
        scene.cam.yaw -= (e.clientX - lx) * 0.006;
        scene.cam.pitch = Math.max(-1.0, Math.min(1.0,
          scene.cam.pitch - (e.clientY - ly) * 0.006));
        lx = e.clientX; ly = e.clientY;
      });
      c.addEventListener('dblclick', () => {
        scene.cam = Object.assign({}, scene.defCam);
      });
      c.addEventListener('wheel', (e) => {
        e.preventDefault();
        scene.cam.distance = Math.max(1.0, Math.min(6,
          scene.cam.distance * (e.deltaY > 0 ? 1.1 : 0.9)));
      }, { passive: false });
    }

    /* ---------------- sound ---------------- */

    function audioReady() {
      /* Page-level sound gate (js/audio-toggle.js): every cue, sting and
       * the BGM render path funnel through here, so one check mutes all. */
      if (window.LegaiaSound && !LegaiaSound.isSoundOn()) return null;
      if (!sfxIds) return null;
      if (!sfx) {
        const Ctx = window.AudioContext || window.webkitAudioContext;
        if (!Ctx) return null;
        const ctx = new Ctx();
        const buffers = {};
        for (const [name, id] of Object.entries(sfxIds)) {
          const pcm = api.dance_sfx_pcm(id);
          const rate = api.dance_sfx_rate(id);
          if (!pcm.length || !rate) continue;
          const buf = ctx.createBuffer(1, pcm.length, rate);
          const ch = buf.getChannelData(0);
          for (let i = 0; i < pcm.length; i++) ch[i] = pcm[i] / 32768;
          buffers[name] = buf;
        }
        /* The good-step stings: 3 random picks x 2 layers keyed together. */
        const stings = [];
        for (let r = 0; r < 3; r++) {
          const pair = [];
          for (let l = 0; l < 2; l++) {
            const pcm = api.dance_sting_pcm(r, l);
            const rate = api.dance_sting_rate(r, l);
            if (pcm.length && rate) {
              const buf = ctx.createBuffer(1, pcm.length, rate);
              const ch = buf.getChannelData(0);
              for (let i = 0; i < pcm.length; i++) ch[i] = pcm[i] / 32768;
              pair.push(buf);
            }
          }
          stings.push(pair);
        }
        sfx = { ctx, buffers, stings };
      }
      if (sfx.ctx.state === 'suspended') sfx.ctx.resume();
      return sfx;
    }

    function play(name, gain = 0.5) {
      const a = audioReady();
      if (!a || !a.buffers[name]) return;
      const src = a.ctx.createBufferSource();
      src.buffer = a.buffers[name];
      const gn = a.ctx.createGain();
      gn.gain.value = gain;
      src.connect(gn).connect(a.ctx.destination);
      src.start();
    }

    function playSting() {
      const a = audioReady();
      if (!a || !a.stings.length) return;
      const r = (Math.random() * 3) | 0;
      for (const buf of a.stings[r] || []) {
        const src = a.ctx.createBufferSource();
        src.buffer = buf;
        const gn = a.ctx.createGain();
        gn.gain.value = 0.45;
        src.connect(gn).connect(a.ctx.destination);
        src.start();
      }
    }

    /* The disco jukebox track the dance plays. The dance overlay itself loads
     * one of two mode-selected tracks (extraction 1048/1054 = global BGM
     * 2058/2064); the page can swap in any Sol-disco-floor track from the same
     * bank via setBgmTrack(). Default = overlay track A. */
    let bgmTrack = 2058;
    let bgmSrc = null;

    /* Render + start the selected BGM as a seamless loop (SEQ+VAB through the
     * clean-room SPU, one true loop period repeated). Rendering ~40 s of SPU
     * output blocks for a moment, so the caller shows a status line first and
     * calls this from a timeout. */
    function startBgm(seconds) {
      stopBgm();
      const a = audioReady();
      if (!a || !window.MgBgm2) return false;
      const entry = MgBgm2.render(api, a.ctx, bgmTrack, Math.min(seconds || 45, 45));
      if (!entry) return false;
      bgmSrc = MgBgm2.start(a.ctx, entry, 0.55);
      return !!bgmSrc;
    }

    function stopBgm() {
      if (bgmSrc) { try { bgmSrc.stop(); } catch (e) { /* done */ } bgmSrc = null; }
    }

    /* Pick the jukebox track (a global BGM id from dance_jukebox_json). If the
     * dance is already sounding, swap to it immediately. */
    function setBgmTrack(bgm) {
      bgm = bgm | 0;
      if (bgm === bgmTrack) return;
      bgmTrack = bgm;
      if (bgmSrc) startBgm(45);
    }

    /* Muting mid-song also stops the already-started BGM buffer (one-shot
     * cues are short enough to just let ring). */
    if (window.LegaiaSound) {
      LegaiaSound.onChange((on) => { if (!on) stopBgm(); });
    }

    /* ---------------- drawing ---------------- */

    /* Draw widget `id` centred at retail (x, y) - exactly the emitter's
     * contract - through palette `pal` (default: the record's own). */
    function wdraw(id, x, y, pal, bright) {
      const w = widgets[id];
      if (!w) return;
      const img = page(pal === undefined ? w.palette : pal);
      if (!img) return;
      const dx = (x - w.w / 2) * SCALE, dy = (y - w.h / 2) * SCALE;
      g.drawImage(img, w.u, w.v, w.w, w.h, dx, dy, w.w * SCALE, w.h * SCALE);
      if (bright) {          /* the 0xFF-brightness pass (texel * c / 128) */
        g.save();
        g.globalCompositeOperation = 'lighter';
        g.globalAlpha = 0.5;
        g.drawImage(img, w.u, w.v, w.w, w.h, dx, dy, w.w * SCALE, w.h * SCALE);
        g.restore();
      }
    }

    /* The big blue digit font: widget 1's cell with u0 = digit * 16
     * (FUN_801d32f8). Only significant digits draw; the ones digit sits in
     * slot 7, so a score of 0 is a single '0' at base + 112. */
    function drawScore(baseX, y, value) {
      const s = String(Math.max(0, value | 0));
      for (let i = 0; i < s.length; i++) {
        const slot = 8 - s.length + i;
        const d = s.charCodeAt(i) - 48;
        const w = widgets[W.DIGIT];
        const img = page(w.palette);
        const x = baseX + slot * 16;
        g.drawImage(img, d * 16, w.v, w.w, w.h,
          (x - w.w / 2) * SCALE, (y - w.h / 2) * SCALE, w.w * SCALE, w.h * SCALE);
      }
    }

    function drawFace(slot, x, y) {
      const m = faces && faces.meta[slot];
      const imgs = faces && faces.imgs[slot];
      if (!m || !m.ok || !imgs || !imgs.length) return;
      const img = imgs[Math.min(poses[slot], imgs.length - 1)];
      if (!img) return;
      const [fx, fy, fw, fh] = m.face;
      const s = 48 / fh;     /* normalise the windows to a 48 px-tall inset */
      const dw = fw * s * SCALE * 0.75, dh = 48 * SCALE * 0.75;
      g.save();
      g.imageSmoothingEnabled = false;
      g.drawImage(img, fx, fy, fw, fh, (x * SCALE) - dw / 2, y * SCALE, dw, dh);
      g.restore();
    }

    /* The beat track (FUN_801d2524, at its traced anchor). Retail inserts
     * every prim at the same OT slot, and PSX AddPrim inserts at the bucket
     * head, so the LAST-emitted prims draw FIRST: the scissor (E3/E4, clip
     * `[x, x + 0x50)`) applies to the body tiles and the scrolling notes,
     * while the end caps and the marker arrow draw unclipped ON TOP - that
     * clip + overdraw is what rounds the pill's ends. */
    function drawTrack(st, chart) {
      const T = layout.track;
      const boundary = (st.beat & layout.flash.beat_mask) === layout.flash.beat_mask
        && st.phase < layout.flash.phase_lt;
      const trackPal = boundary ? PAL_TRACK_FLASH : PAL_TRACK_IDLE;

      /* 1. body tiles + notes, under the retail scissor window. */
      g.save();
      g.beginPath();
      g.rect(T.x * SCALE, (T.y - 10) * SCALE, T.clip_w * SCALE, 20 * SCALE);
      g.clip();
      for (let i = 0; i < T.body_tiles; i++) {
        wdraw(W.BODY, T.x + i * T.body_step, T.y, trackPal);
      }
      if (chart && chart.rows.length) {
        /* Note for relative beat i: x + i*16 - (phase*16/281 + 5) - 4. */
        const row = chart.rows[Math.min(st.lane, chart.rows.length - 1)];
        const off = Math.floor(st.phase * 16 / st.period) + 5;
        for (let i = 0; i < 8; i++) {
          const beat = (st.beat + i - 1) & 31;
          const sym = row[beat % row.length];
          const x = T.x + i * T.note_step - off - 4;
          wdraw(W.NOTE_BASE + sym, x, T.y, PAL_NOTE, i === 1 && flashT > 0);
        }
      }
      g.restore();

      /* 2. the caps + the marker arrow, unclipped, over the body. */
      wdraw(W.CAP_L, T.cap_l, T.y, trackPal);
      wdraw(W.CAP_R, T.cap_r, T.y, trackPal);
      wdraw(W.ARROW, T.arrow[0], T.arrow[1]);

      /* The stock markers under the track are the remaining triangles: retail
       * draws exactly `DAT_801d534c` of them (FUN_801d2524's tail loop), so
       * they tick down as the groovy moves are spent. */
      const stock = st.triangles === undefined ? 3 : st.triangles;
      for (let i = 0; i < stock; i++) {
        wdraw(W.STOCK, T.x + i * T.stock_step, T.stock_y);
      }
    }

    /* The groovy-move window: while the spin runs, nothing the player presses
     * is judged (retail's actor handler stops calling the award routine until
     * the move clip ends). Say so, over the track. */
    function drawGroovyWindow(st) {
      if (!st.lock) return;
      const L = layout, T = L.track;
      const w = 108, h = 13, x = T.x - 4, y = T.y - 26;
      g.save();
      g.globalAlpha = 0.72;
      g.fillStyle = '#000';
      g.fillRect(x * SCALE, y * SCALE, w * SCALE, h * SCALE);
      g.globalAlpha = 1;
      /* Progress bar for the remaining spin. */
      const frac = Math.max(0, Math.min(1, st.lock / 64));
      g.fillStyle = st.feedback === false ? '#c9636b' : '#2dcca7';
      g.fillRect(x * SCALE, (y + h - 2) * SCALE, w * frac * SCALE, 2 * SCALE);
      g.fillStyle = '#fff';
      g.font = `bold ${8 * SCALE}px monospace`;
      g.textAlign = 'center';
      g.textBaseline = 'middle';
      g.fillText('GROOVY MOVE', (x + w / 2) * SCALE, (y + h / 2 - 1) * SCALE);
      g.restore();
    }

    function spawnBanner(id, x, y, life) {
      banners.push({ w: id, x, y, t: 0, life: life || 45 });
    }

    function drawBanners() {
      banners = banners.filter(b => b.t < b.life);
      for (const b of banners) {
        const rise = Math.min(b.t, 12) * 0.5;
        g.save();
        g.globalAlpha = b.t > b.life - 10 ? (b.life - b.t) / 10 : 1;
        wdraw(b.w, b.x, b.y - rise);
        g.restore();
        b.t++;
      }
    }

    /* ---- events from the rules engine ---- */

    function onPress(result, st, sym) {
      const B = layout.banners;
      /* Noa's body plays the judge-triggered move clip the kind descriptor
       * names for the event (see bodyRender's doc for the pair semantics). */
      if (body && body.moves && st) {
        const lane = Math.min(st.lane | 0, 2);
        const M = body.moves, human = api.dance_body_human_index();
        if (result === 'miss') {
          if (sym === 1) triggerMove(human, M.miss_square);
          else if (sym === 2) triggerMove(human, M.miss_circle);
        } else if (result === 'groovy' || result === 'groovy_off') {
          /* The wildcard's clip is the on-beat step for the lane (move pair
           * 8 + lane) - retail returns lane*2 + 0x10 from the 0x10 branch. */
          triggerMove(human, M.beat[lane]);
        } else if (result === 'sequence') {
          triggerMove(human, (sym === 2 ? M.seq_circle : M.seq_square)[lane]);
        }
      }
      if (result === 'ignored' || result === 'none') return;   /* mid-groovy-move */
      if (result === 'no_charge') {
        play('miss', 0.35);
        return;
      }
      if (result === 'groovy' || result === 'groovy_off') {
        /* Landed on the 4-beat combo slot = the big multiplier (and the gauge
         * step that promotes the lane): the combo banner tier by lane. Off the
         * slot it is worth (lane+1)*3 and gets no banner - retail only spawns
         * one when DAT_801d570c says it landed. */
        poses[0] = Math.min(3, (faces && faces.meta[0] && faces.meta[0].poses - 1) || 2);
        poseT[0] = 30;
        if (result === 'groovy') {
          flashT = 12;
          const tier = Math.min(st.lane | 0, 2);        /* Cool! / Great!! / Fever!!! */
          const id = tier === 0 ? W.COOL : tier === 1 ? W.GREAT : W.FEVER;
          play(tier === 0 ? 'cool' : tier === 1 ? 'great' : 'fever', 0.55);
          spawnBanner(id, B.rating[0], B.rating[1]);
          const so = tier === 0 ? B.star_off.cool : B.star_off.great;
          spawnBanner(W.STAR, B.rating[0] - so, B.rating[1]);
          spawnBanner(W.STAR, B.rating[0] + so, B.rating[1]);
        } else {
          playSting();
        }
        return;
      }
      if (result === 'miss') {
        play('miss', 0.5);
        poses[0] = 1; poseT[0] = 24;
        spawnBanner(W.MISS, B.miss[0], B.miss[1]);
      } else if (result === 'hit' || result === 'sequence') {
        flashT = 10;
        poses[0] = Math.min(2 + (result === 'sequence' ? 1 : 0),
          (faces && faces.meta[0] && faces.meta[0].poses - 1) || 2);
        poseT[0] = 24;
        if (result === 'sequence') {
          play('cool', 0.5);
          spawnBanner(W.GOOD, B.rating[0], B.rating[1]);
          spawnBanner(W.STAR, B.rating[0] - B.good_star_off, B.rating[1]);
          spawnBanner(W.STAR, B.rating[0] + B.good_star_off, B.rating[1]);
        } else {
          playSting();
          spawnBanner(W.GOOD, B.rating[0], B.rating[1]);
        }
      }
    }

    function startRun(songSeconds) {
      banners = [];
      poses = [0, 0, 0, 0]; poseT = [0, 0, 0, 0];
      if (body) {
        body.lastBeat = -1;
        for (const a of body.anim) { a.cursor = 0; a.move = null; }
      }
      flashT = 0; finished = false;
      if (body) body.rivalTri = {};
      intro = { t: 0 };
      play('start', 0.5);
      startBgm(songSeconds);
    }

    function stopRun() {
      stopBgm();
      intro = null;
    }

    /* The count-in overlay: READY... slides, then 1 / 2 / 3, then GO!.
     * Returns true while the engine clock should hold. */
    function introActive() { return intro !== null; }

    /* Face rigs of the floor cast, left/centre/right (kind = rig id,
     * FUN_801d03c4); the retail qualifier cast when the tables didn't decode. */
    function castRigs() {
      if (castMeta && castMeta.dancers) return castMeta.dancers.map(d => d.kind);
      return [2, 0, 3];
    }

    function tickCosmetics(st) {
      /* Blink + the competitors' on-beat face pose (cosmetic; their scoring
       * runs are simulated by the rules engine, not by these poses). */
      for (let d = 0; d < 4; d++) {
        if (poseT[d] > 0 && --poseT[d] === 0) poses[d] = 0;
        if (--blinkT[d] <= 0) {
          blinkT[d] = 80 + ((Math.random() * 80) | 0);
          if (poseT[d] === 0) { poses[d] = 1; poseT[d] = 8; }
        }
      }
      if (st && !st.dead_zone && (st.beat & 3) === 3 && st.phase < 40) {
        for (const rig of castRigs()) {
          if (rig !== 0 && poseT[rig] === 0) { poses[rig] = 2; poseT[rig] = 16; }
        }
      }
      if (flashT > 0) flashT--;
    }

    /* One full frame. `st` may be null before the first run. */
    function draw(st, chart) {
      const Wpx = canvas.width, Hpx = canvas.height;
      if (body) {
        /* The dancers are drawn in 3D on the gl canvas behind this one -
         * Noa's own field-view model plus the dance-hall scene's dancer
         * NPCs, playing the scene's choreography clips. Keep the HUD canvas
         * transparent so the bodies show through. */
        bodyRender(st, chart);
        g.clearRect(0, 0, Wpx, Hpx);
      } else {
        /* No body scene (no WebGL, or PROT 0874 didn't decode): the HUD sits
         * on a neutral ground rather than a faked room, and the note says so. */
        g.fillStyle = '#0b0b10';
        g.fillRect(0, 0, Wpx, Hpx);
        g.fillStyle = '#11131c';
        g.fillRect(0, 150 * SCALE, Wpx, Hpx - 150 * SCALE);
      }

      if (!widgets) return;
      const L = layout;
      /* Retail's active draw environment carries a global offset (pinned at
       * +4 lines against the VRAM capture) - apply it to the whole HUD. */
      g.save();
      g.translate(L.screen_offset[0] * SCALE, L.screen_offset[1] * SCALE);

      /* Score boxes: the human dancer is the CENTRE box (FUN_801d231c), the
       * two competitors left and right - and their scores are live, off their
       * own simulated runs (retail auto-feeds them the same chart). */
      for (let i = 0; i < 3; i++) {
        wdraw(W.SCORE_BOX, L.score_boxes.xs[i], L.score_boxes.y);
      }
      const rv = (st && st.rivals) || [];
      const scores = [
        rv[0] ? rv[0].score : 0,
        st ? st.score : 0,
        rv[1] ? rv[1].score : 0,
      ];
      for (let i = 0; i < 3; i++) {
        drawScore(L.digit_bases.xs[i], L.digit_bases.y, scores[i]);
      }

      /* The face-stamp insets are a fallback, not a retail element: retail
       * MoveImage-blits those windows onto the 3D dancers' head textures,
       * never onto the screen. With the cast rendered in 3D behind the HUD
       * they'd cover the hall's wall banners, so they only draw when the
       * body scene is absent (rig per box = the floor cast's dancer kind;
       * retail qualifier: left = rig 2, centre = Noa rig 0, right = rig 3). */
      if (!body) {
        const rigs = castRigs();
        drawFace(rigs[0], L.score_boxes.xs[0], 46);
        drawFace(rigs[1], L.score_boxes.xs[1], 46);
        drawFace(rigs[2], L.score_boxes.xs[2], 46);
      }

      if (st) {
        /* Groove gauge: Lv. label + the level digit (u0 = 0xD0 + lane*8). */
        wdraw(W.LV_LABEL, L.gauge.lv_x, L.gauge.y);
        const lv = widgets[W.LV_DIGIT];
        const img = page(lv.palette);
        g.drawImage(img, 0xD0 + st.lane * 8, lv.v, lv.w, lv.h,
          (L.gauge.digit_x - lv.w / 2) * SCALE, (L.gauge.y - lv.h / 2) * SCALE,
          lv.w * SCALE, lv.h * SCALE);

        drawTrack(st, chart);
        drawGroovyWindow(st);
      }

      /* Count-in overlay (banner widgets at the traced centre spawn). */
      if (intro) {
        const C = L.banners.centre;
        const t = intro.t++;
        if (t < 55) {
          const slide = Math.max(0, 55 - t) * 6;
          wdraw(W.READY, C[0] + (t < 28 ? slide : 0), C[1] - 1);
        } else if (t < 85) wdraw(W.COUNT[0], C[0], C[1]);
        else if (t < 115) wdraw(W.COUNT[1], C[0], C[1]);
        else if (t < 145) wdraw(W.COUNT[2], C[0], C[1]);
        else if (t < 175) wdraw(W.GO, C[0], C[1], undefined, true);
        else intro = null;
        if (t >= 145) intro && (intro.go = true);
      }

      if (st && st.over && !finished) {
        finished = true;
        spawnBanner(W.FINISH, L.banners.centre[0], L.banners.centre[1], 120);
        stopBgm();
      }

      drawBanners();
      g.restore();
      tickCosmetics(st);
    }

    return {
      loadAssets, startRun, stopRun, onPress, draw, introActive, setBgmTrack,
      get introGate() { return intro !== null && !intro.go; },
      sfxCount() { return sfxIds ? Object.keys(sfxIds).length : 0; },
      bgmOk() { return !!(bgmInfo && bgmInfo.ok); },
      bodyOk() { return !!body; },
      hallOk() { return !!(body && body.env); },
      /* Camera inspection/override (headless verification + tuning). */
      camInfo() {
        return body
          ? { cam: Object.assign({}, body.cam), center: body.center.slice(), radius: body.radius }
          : null;
      },
      setCam(c) { if (body && c) Object.assign(body.cam, c); },
      stopAll() { stopBgm(); },
    };
  }

  /* Pose `base` (object-local verts) through `clip` at `frame` into `out`,
   * then spin the whole figure `yaw` about Y and shift it `dx` along X - the
   * retail per-object composition Rz.Ry.Rx . v + T with a world transform on
   * top. Identical to minigame-baka.js's poser (the pose stream shape matches
   * `baka_anim_pose_frames`). `vertBase` selects the dancer's slice of the
   * combined buffers. */
  function poseInto(out, base, oids, clip, frame, vertBase, dx, yaw) {
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
      const wx = x * wcos + z * wsin;
      const wz = -x * wsin + z * wcos;
      out[vi] = wx + (dx || 0);
      out[vi + 1] = y;
      out[vi + 2] = wz;
    }
  }

  return { create };
})();
