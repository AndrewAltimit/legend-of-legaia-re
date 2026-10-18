/* world-overview-kingdoms.js - per-kingdom data for the world-overview page.
 *
 * The PROT range, camera anchors, top-view framing, walk-camera depth
 * terms, Mist script state and unplaced-landmark footprint of each of the
 * three kingdoms.
 * Data only. Published as
 * window.LegaiaWorldOverviewKingdoms.
 *
 * Classic script; loads before world-overview-app.js. */

(function () {
  /* ---------- PROT ranges per kingdom (from CDNAME map) ----------- */
  const KINGDOM_PROT = {
    drake:   { start: 85,  end: 93,  label: 'Drake Kingdom',   cdname: 'map01' },
    sebucus: { start: 244, end: 253, label: 'Sebucus Islands', cdname: 'map02' },
    karisto: { start: 391, end: 398, label: 'Karisto Kingdom', cdname: 'map03' },
  };

  /* ---------- Retail camera defaults (from .mc{1,2,3}) -------------
   * Captured per-kingdom from mednafen save states in walk-view mode.
   * The X/Z scrolls live at `_DAT_80089120` / `_DAT_80089118` and are
   * stored as negated map-origin coordinates - the engine writes
   * `_DAT_80089118 = -(int)*(short *)(actor + 0x14)` etc., so the world
   * camera target is the negation of the raw cell. The captured saves
   * are all in walk-view (DAT_801F2B94 = 0), so these reflect the
   * load-time spawn rather than an interactively-scrolled dev-menu
   * camera; they're still useful as per-kingdom anchor points.
   *   mednafen-state world-map-camera --table <save> ...
   * regenerates these values. */
  const KINGDOM_CAM = {
    drake:   { centerX: 8832, centerZ: 8832, zoom: 0x170 },
    sebucus: { centerX: 7936, centerZ: 9344, zoom: 0x170 },
    karisto: { centerX: 7936, centerZ: 9088, zoom: 0x170 },
  };
  /* Top-view camera anchors per kingdom. The "lock to retail top-view"
   * button slams the world-cam to these centres + sets the zoom so the
   * frame matches the in-game world-map camera. Values mirror the
   * walk-view load-time spawn anchors decoded by `mednafen-state
   * world-map-camera --table <save>` against per-kingdom saves; the
   * dev-menu's free-camera top-view (DAT_801F2B94 != 0) starts from the
   * same anchor before user input scrolls it, so these double as
   * top-view defaults until a captured dev-menu state refines them. */
  const KINGDOM_TOPVIEW_CAM = {
    drake:   { centerX: 8832, centerZ: 8832, azimuth: 0x0000, zoom: 0x0170 },
    sebucus: { centerX: 7936, centerZ: 9344, azimuth: 0x0000, zoom: 0x0170 },
    karisto: { centerX: 7936, centerZ: 9088, azimuth: 0x0000, zoom: 0x0170 },
  };
  /* Class-conditional target footprints for the unplaced-slot1 grid.
   * Each unplaced TMD is normalised so its larger XZ extent maps to this
   * target, then centroid-anchored at its assigned grid slot. Numbers
   * are world units (Drake is 16320x16320). The renderer falls back to
   * MESH_SCALE when normalisation is disabled or the AABB collapses to
   * zero. Matches the four classes in slot1_classification.toml. */
  const UNPLACED_TARGET_FOOTPRINT = {
    landmark:   600,
    decoration: 200,
    ground_tile: 1200,
    unknown:    600,
  };
  /* The walk-view camera each kingdom's zone record composes, read off the
   * resident overworld states (docs/subsystems/world-map.md#walk-view-camera-retail-model-ram-pinned):
   * pitch (4096 = full turn) and the eye trio's Z (TRZ). The distance haze
   * stands SZ in as eyeZ + 6 * cos(pitch) * distance - the 6x world scale
   * of the base matrix DAT_8007BF10 times the pitch's forward component. */
  const KINGDOM_WALK_EYE = {
    drake:   { pitch: 370, eyeZ: 8875 },
    sebucus: { pitch: 360, eyeZ: 9139 },
    karisto: { pitch: 476, eyeZ: 11041 },
  };
  /* The Mist as each kingdom shows it before any area's Mist lifts. Each
   * fog region draws while its story flag is clear (op 4C C1 rewrites the
   * enable byte from the flag's inverse); `flagsSet` are the region flags
   * the kingdom's own scene system script (MAN P1[0]) sets before that
   * rewrite on a first arrival: map02 sets 0x51B unless 0x1D5 is set,
   * map03 sets 0x526 unless 0x36F is set. `yControl` is the control word
   * the same script hands op 4C 49 (sub 9) - the camera vertical offset
   * the fog heights are measured against is that word less the player's
   * floor height (map01 60, map02 128, map03 152). */
  const KINGDOM_MIST = {
    drake:   { flagsSet: [],      yControl: 60 },
    sebucus: { flagsSet: [0x51B], yControl: 128 },
    karisto: { flagsSet: [0x526], yControl: 152 },
  };
  window.LegaiaWorldOverviewKingdoms = {
    KINGDOM_PROT, KINGDOM_CAM, KINGDOM_TOPVIEW_CAM, UNPLACED_TARGET_FOOTPRINT,
    KINGDOM_WALK_EYE, KINGDOM_MIST,
  };
})();
