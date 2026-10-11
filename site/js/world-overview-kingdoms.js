/* world-overview-kingdoms.js - per-kingdom data for the world-overview page.
 *
 * The PROT range, camera anchors, top-view framing, fog tint and unplaced-
 * landmark footprint of each of the three kingdoms, plus the fog defaults
 * and the placement-list names and colours. Data only. Published as
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
  /* Per-kingdom baseline fog tint (BGR555 -> linear RGB). Used as the
   * fragment shader's fallback when a captured LUT entry is zero, so the
   * "fog" toggle still produces a kingdom-flavoured gradient before the
   * user supplies fog_probe.lut.bin. The values are mid-tier samples of
   * each kingdom's dev-menu top-view sky colour. */
  const KINGDOM_FOG_TINT = {
    drake:   { r: 0.18, g: 0.22, b: 0.34 },
    sebucus: { r: 0.16, g: 0.26, b: 0.32 },
    karisto: { r: 0.26, g: 0.20, b: 0.22 },
  };
  /* Initial fog defaults: matches the retail probe shape (Z_far = Z >> 5
   * over a ~16384-unit reference). Overwritten when the user uploads a
   * fog_probe.lut.bin via the fog-LUT picker. */
  const DEFAULT_FOG_PARAMS = { enable: 0, zShift: 5, farRef: 12000, color: null };
  const LIST_NAMES = {
    0: 'player/system', 1: 'entities', 2: 'background',
    3: 'reserve_3', 4: 'reserve_4', 5: 'reserve_5', 6: 'extra',
  };
  const LIST_COLORS = { 0: '#56B6F5', 1: '#79D279', 2: '#C0C0C0', 6: '#E07028' };

  window.LegaiaWorldOverviewKingdoms = {
    KINGDOM_PROT, KINGDOM_CAM, KINGDOM_TOPVIEW_CAM, UNPLACED_TARGET_FOOTPRINT,
    KINGDOM_FOG_TINT, DEFAULT_FOG_PARAMS, LIST_NAMES, LIST_COLORS,
  };
})();
