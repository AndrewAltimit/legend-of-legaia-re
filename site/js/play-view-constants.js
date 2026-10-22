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
   * +16+i actor meshes, +128+i an actor's shatter companion (the body
   * broken into its triangles, LegaiaFieldActors.shardMesh), +256+tmd the
   * 3D FX model cache
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
    azimuthUnits, poseInto,
  };
})();
