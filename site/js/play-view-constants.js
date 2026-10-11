/* play-view-constants.js - constants and small pure helpers the play page's
 * PlayView files share.
 *
 * The scene-mesh id spaces, the VR scale, the pad-binding aliases, the
 * camera-azimuth conversion and the occlusion-fade geometry tests. Published
 * as window.LegaiaPlayShared; each play-view-*.js file (and play-app.js)
 * takes the names it uses from there.
 *
 * Classic script. Requires pad-bindings.js and field-actors.js first, and
 * loads before play-app.js. */
(function () {
  'use strict';

  const A2R = Math.PI * 2 / 4096;     /* PSX 12-bit angle -> radians */
  const PLAYER_MESH_ID = 900000;      /* scene-mesh id space above any env slot */
  const NPC_MESH_BASE  = 910000;
  const TILE_MESH_BASE = 920000;   /* one mesh per board-owned actor slot */
  /* Battle 3D layer: 930000 backdrop, +1 ground grid, +2 the effect-pool
   * billboard batch, +3 the backdrop's spun slot (the horizon mist ribbon),
   * +16+i actor meshes, +256+tmd the 3D FX model cache
   * (keyed by the engine's global-TMD-pool index, so one upload per distinct
   * effect mesh per fight). */
  const BATTLE_MESH_BASE = 930000;
  const BATTLE_FX_BILLBOARD_MESH = BATTLE_MESH_BASE + 2;
  const BATTLE_BACKDROP_SPUN_MESH = BATTLE_MESH_BASE + 3;
  const BATTLE_FX_MODEL_BASE = BATTLE_MESH_BASE + 256;
  /* Field-frame FX: a scene-pack stager part uploaded on demand (slot i of
   * the env pack that no placement drew), and script-spawned actor meshes
   * keyed by actor slot. Both above every other id space. */
  const FIELD_FX_PACK_MESH_BASE = 1200000;
  const DYN_ACTOR_MESH_BASE = 1300000;
  /* Identity model matrix for draws whose transform the engine already folded
   * into the vertex stream (the FX billboard batch). */
  const IDENTITY_MODEL = new Float32Array([
    1, 0, 0, 0,
    0, 1, 0, 0,
    0, 0, 1, 0,
    0, 0, 0, 1,
  ]);
  /* Wall-clock NPC anim fallback rate, used ONLY against a cached WASM
   * without the engine clip-state API. The live path reads each clip's
   * current frame from the engine, whose playhead advances in sim-tick time
   * (60 Hz ticks, 2 ticks per clip frame = 30 clip fps, the retail cadence). */
  /* VR world scale - same anchor as the full-map view: the ~130-unit character
   * mesh is a 1.7 m human, so a metre is ~76 world units and the headset stands
   * in the town at human height. See docs/subsystems/vr-mode.md. */
  const VR_UNITS_PER_METER = 76;
  /* First-person mode derives its scale from the LIVE player mesh instead of
   * the 130-unit rule of thumb: Vahn is an adult-human ~1.7 m, so
   * units/metre = measured mesh height / 1.7. Eyes sit a little below the top
   * of the head (~94% of standing height) - that is what the `local`
   * reference-space fallback lifts the rig by; a `local-floor` device takes
   * the real headset height instead. */
  const VAHN_HEIGHT_M = 1.7;
  const VR_FP_EYE_HEIGHT_M = 1.6;
  const VR_FP_FALLBACK_MESH_HEIGHT = 130;

  /* Keyboard -> PSX pad bits: the shared table in `js/pad-bindings.js`, read
   * out of the engine (`Mapping::web_default` through `pad_bindings_json`).
   * Not a table in this file, and not a table in any page file - see that
   * module's header for what happened the last time it was. */
  const adoptPadBindings = window.legaiaAdoptPadBindings;
  const padMaskOf = window.legaiaPadMaskOf;
  const padTable = () => window.legaiaPadTable();


  /* The engine takes the camera's azimuth and quantises it to a quarter turn to
   * remap the d-pad ("up" walks away from the camera, "right" walks screen-right).
   *
   * The sense is **opposite** to this camera's `yaw`. Working the shared orbit
   * projection's basis out: the eye sits at
   * `target + d(sinP.sinY, cosP, -sinP.cosY)`, so the camera's right axis is
   * `(-cosY, 0, -sinY)` - and the projection then mirrors screen X (the retail
   * horizontal flip), which lands screen-right on world `(cosY, 0, sinY)` and
   * screen-up on `(-sinY, 0, cosY)`. The engine's quadrant table
   * (`decode_field_direction`) rotates the other way, so the azimuth it wants is
   * `-yaw`. Feed it `+yaw` and the controls come out correct at yaw 0 and
   * inverted at a quarter turn - which is exactly the bug this negation fixes. */
  function azimuthUnits(yaw) {
    const u = Math.round(-yaw / (Math.PI * 2) * 4096) % 4096;
    return (u + 4096) % 4096;
  }

  /* ---------- occluder cull (see the note in `_frame`) ---------- */

  /* Occluder cull, DISABLED. The native renderer draws the whole scene every
   * frame with no distance / frustum / occlusion culling (docs/subsystems/
   * renderer.md), and the browser matches it. The per-frame lens->player slab
   * test below bounded each body by an axis-aligned box over whole terrain
   * tiles / walls / buildings, so neighbouring bodies blinked out as the camera
   * orbited or the player walked (the reported "meshes cull while walking"
   * symptom). Leave this `false`; every body a scene loads is drawn every
   * frame, unconditionally. */
  const OCCLUDER_CULL = false;

  /* Half a character's standing height, in world units. Paired with
   * HALF_CHAR_HEIGHT in crates/web-viewer/src/play.rs, which is the point
   * the occlusion gate (`field_player_occluded`) tests - the fade focus has
   * to be staged at that same point or the hole lands off the character. */
  const HALF_CHAR_HEIGHT = 65;

  /* World AABB of one placement: the mesh's local box carried through the draw
   * model `T(x, y, z) . Ry(rotY) . diag(sc, -sc, sc)` (`placementModelScaledY`).
   * Baked once per placement so the per-frame occluder test is a slab
   * intersection against the body's REAL box - not a bounding sphere of its
   * longest axis, which for a floor slab or a staircase is hundreds of units of
   * empty space and is what used to blink them out. */
  function placementWorldBox(aabb, d) {
    if (!aabb) return null;
    const sc = (d.scale != null) ? d.scale : 1.0;
    const c = Math.cos(d.rotY || 0), s = Math.sin(d.rotY || 0);
    const ax = Math.abs(c), az = Math.abs(s);
    const cx = d.x + sc * (c * aabb.cx + s * aabb.cz);
    const cy = d.y - sc * aabb.cy;               /* the model flips Y */
    const cz = d.z + sc * (-s * aabb.cx + c * aabb.cz);
    const hx = sc * (ax * aabb.sx + az * aabb.sz) * 0.5;
    const hy = sc * aabb.sy * 0.5;
    const hz = sc * (az * aabb.sx + ax * aabb.sz) * 0.5;
    return [cx - hx, cy - hy, cz - hz, cx + hx, cy + hy, cz + hz];
  }

  /* Does the segment `p + t*e`, `t in (OCC_T_MIN, OCC_T_MAX)`, pierce the world
   * box? Standard slab test. `t = 0` is the PLAYER and `t = 1` is the LENS, so
   * the trims are asymmetric: the first `OCC_T_MIN` of the segment is skipped so
   * the body the player stands on / walks past is never an occluder, while the
   * lens end runs all the way to `1` - a body the camera is *inside* (a cliff
   * face, a cave roof) is exactly what has to go. */
  const OCC_T_MIN = 0.12, OCC_T_MAX = 1.0;
  function segmentHitsBox(px, py, pz, ex, ey, ez, box) {
    if (!box) return false;
    let t0 = OCC_T_MIN, t1 = OCC_T_MAX;
    const p = [px, py, pz], e = [ex, ey, ez];
    for (let i = 0; i < 3; i++) {
      const lo = box[i], hi = box[i + 3];
      if (Math.abs(e[i]) < 1e-6) {
        if (p[i] < lo || p[i] > hi) return false;   /* parallel and outside */
        continue;
      }
      let ta = (lo - p[i]) / e[i], tb = (hi - p[i]) / e[i];
      if (ta > tb) { const s = ta; ta = tb; tb = s; }
      if (ta > t0) t0 = ta;
      if (tb < t1) t1 = tb;
      if (t0 > t1) return false;
    }
    return true;
  }

  /* Pose an object-local mesh into `out` from one frame of a clip - the
   * shared actor animator (js/field-actors.js), the one the map viewer's
   * actor layer runs too. */
  const poseInto = window.LegaiaFieldActors.poseInto;

  window.LegaiaPlayShared = {
    A2R, PLAYER_MESH_ID, NPC_MESH_BASE, TILE_MESH_BASE, BATTLE_MESH_BASE,
    BATTLE_FX_BILLBOARD_MESH, BATTLE_BACKDROP_SPUN_MESH,
    BATTLE_FX_MODEL_BASE, FIELD_FX_PACK_MESH_BASE, DYN_ACTOR_MESH_BASE,
    IDENTITY_MODEL, VR_UNITS_PER_METER, VAHN_HEIGHT_M, VR_FP_EYE_HEIGHT_M,
    VR_FP_FALLBACK_MESH_HEIGHT, adoptPadBindings, padMaskOf, padTable,
    azimuthUnits, OCCLUDER_CULL, HALF_CHAR_HEIGHT, placementWorldBox,
    OCC_T_MIN, OCC_T_MAX, segmentHitsBox, poseInto,
  };
})();
