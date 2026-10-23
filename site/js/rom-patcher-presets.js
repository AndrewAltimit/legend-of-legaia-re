/* rom-patcher-presets.js - the ROM patcher's preset tables.
 *
 * The enemy difficulty scale's stat and group vocabularies (and the parser
 * for the string they serialise to), and the named presets: each one a full
 * configuration of the page's controls.
 *
 * Module script, loaded ahead of rom-patcher-app.js; publishes on
 * window.LegaiaRomPatcher. Data and one pure function; no DOM access. */

// The enemy difficulty scale's per-stat (advanced-mode) sliders.
//
// Each entry is the stat key the patcher's own parser accepts
// (`StatScale::parse` in crates/patcher/src/monster_stats.rs), and the element
// ids are derived from it - so the strings this page emits are the same ones
// `legaia-patcher --enemy-stat-scale hp=2,attack=1.5` takes on the command line,
// and there is no separate browser vocabulary to keep in sync. The order matches
// that module's `STAT_FIELDS`, so an emitted list reads back in the order the
// patcher prints it.
//
// Simple mode is not a different feature: it sends one bare multiplier, which is
// the same type with every field equal. One parser, one planner, one set of bytes.
const SCALE_STATS = [
  ['hp', 'HP'],
  ['mp', 'MP'],
  ['attack', 'Attack'],
  ['defense_high', 'Defense, upper'],
  ['defense_low', 'Defense, lower'],
  ['intelligence', 'Intelligence'],
  ['speed', 'Speed'],
];

// The two enemy groups the scale addresses separately, in the order the emitted
// string lists them. Same widening again: a run that moves both groups together
// sends one unscoped scale, and only a genuine split sends `regular:...|boss:...`
// - so every setting written before the split existed still means what it did.
// The group keys are `ScaleProfile::parse`'s own, so nothing is translated here.
const SCALE_GROUPS = [
  ['regular', 'Random encounters'],
  ['boss', 'Bosses'],
];

// Split an emitted scale string into `{regular, boss}` bodies. The mirror of
// `ScaleProfile::parse`, and deliberately only as much of it as this page can
// itself emit plus what a preset may carry: `|`-separated `group:scale`
// segments, with a bare segment as the `all` base. Anything it can't read comes
// back as retail on both halves rather than a half-applied guess - the sliders
// are what the outgoing string is rebuilt from, so a value nothing reads would
// silently patch a different difficulty than the config asked for.
function splitScaleGroups(text) {
  const out = { regular: '', boss: '' };
  const t = String(text || '').trim();
  if (!t) return out;
  if (!t.includes(':')) return { regular: t, boss: t };
  let base = '';
  const named = {};
  for (const seg of t.split('|')) {
    const s = seg.trim();
    if (!s) continue;
    const at = s.indexOf(':');
    const group = at < 0 ? 'all' : s.slice(0, at).trim().toLowerCase();
    const body = at < 0 ? s : s.slice(at + 1).trim();
    if (group === 'all' || group === 'both' || group === 'every') base = body;
    else if (group === 'regular' || group === 'normal' || group === 'random' || group === 'common') named.regular = body;
    else if (group === 'boss' || group === 'bosses') named.boss = body;
    else return { regular: '', boss: '' }; // unknown group - don't guess
  }
  return { regular: named.regular ?? base, boss: named.boss ?? base };
}

// --- Presets ---------------------------------------------------------------
// Each preset is a full configuration: every control gets a value, so applying
// one is unambiguous. Keys map to control names / element ids below.
const PRESET_BASE = {
  drops: 'none', encounters: 'none', encounter_scope: 'scene', soloStrong: false, fleeExp: false, chests: 'none',
  shops: 'none', casino: 'none', steals: 'none', arts: 'none', doors: 'none',
  door_coupling: 'coupled', houseDoors: false, equipmentDrops: false, seruTrade: false,
  enemyAlly: false, enemyHpBar: false, shinySeru: false, showSuperArts: false, superArtsPack: false, jewelFix: false, approachFix: false, fortressFix: false, conkramFix: false, delilasChallenge: false, customItems: false, fishingPrice: '', renameLocation: '', earthEggPrice: '', artsPower: '', superArtPower: '', artsApGrant: '', spiritAp: '', damageAp: '', oscillatingAp: '', enemyStatScale: '', expScale: '', seruCatchRate: '', delilasParty: '', delilasArtsVoice: 'original', delilasMoves: 'hybrid', attackCount: '',
  startingItems: 0, doorOfWind: false, incense: false,
  speedChain: false, chickenHeart: false, goodLuckBell: false,
  allWarps: false,
  unusedEnemies: false, unusedItems: false,
  monster_stats: 'none', move_power: 'none', element_affinity: 'none',
  spell_cost: 'none', equip_bonus: 'none', weaponSpecialty: false,
  startingLevel: 0,
};

// Both gameplay presets hand the player a generous start: every convenience
// item + accessory, and 5 random consumables on top. The starting LEVEL is not
// part of the bundle - each preset names its own, because the two want
// different curves (see below).
//
// `allWarps` is deliberately NOT in the bundle. It presets the visited-towns
// bitmask, so every Door-of-Wind destination is reachable before the player has
// been there - that skips the route the shuffled chests / encounters / doors are
// meant to be discovered along, and it is a route spoiler in itself. It stays a
// one-click opt-in on the toggle grid; no preset turns it on.
const STARTING_BUNDLE = {
  startingItems: 5,
  doorOfWind: true, incense: true,
  speedChain: true, chickenHeart: true, goodLuckBell: true,
};

// Equipment drops are additive - a code hook grants one extra random gear piece
// on a low per-battle chance, on top of the normal drop - so every gameplay
// preset turns them on; only vanilla leaves them off.
//
// `monster_stats` is Full Chaos only: reshuffled enemy stat blocks move fights
// off the vanilla difficulty curve that the rest of Balanced - kingdom-scope
// encounters, shuffled loot, the level-5 start - is balanced against.
//
// The two presets start at different levels on purpose. Balanced keeps the
// early curve legible: level 5 clears the opening difficulty spike without
// skipping past the fights the shuffled encounter tables are tuned around.
// Full Chaos starts at 10 because its randomized monster stats and world-scope
// encounters can seat a wildly over-levelled fight in the first region, and the
// higher floor is what keeps that survivable.
const PRESETS = {
  vanilla: { ...PRESET_BASE },
  items: {
    ...PRESET_BASE,
    drops: 'shuffle', chests: 'shuffle', shops: 'shuffle',
    casino: 'shuffle', steals: 'shuffle', equipmentDrops: true,
  },
  balanced: {
    ...PRESET_BASE,
    drops: 'shuffle', encounters: 'shuffle', encounter_scope: 'kingdom',
    soloStrong: true, fleeExp: true,
    chests: 'shuffle', steals: 'shuffle', arts: 'shuffle',
    equip_bonus: 'shuffle', equipmentDrops: true,
    seruTrade: true, enemyAlly: true, shinySeru: true, jewelFix: true, approachFix: true, fortressFix: true, conkramFix: true,
    delilasChallenge: true, customItems: true,
    ...STARTING_BUNDLE, startingLevel: 5,
  },
  chaos: {
    ...PRESET_BASE,
    drops: 'random', encounters: 'random', encounter_scope: 'world',
    soloStrong: true, fleeExp: true,
    chests: 'random', shops: 'random', casino: 'random', steals: 'random',
    arts: 'random', doors: 'random', door_coupling: 'coupled',
    houseDoors: true, unusedEnemies: true, unusedItems: true,
    monster_stats: 'random', move_power: 'random', element_affinity: 'random',
    spell_cost: 'random', equip_bonus: 'random', weaponSpecialty: true,
    equipmentDrops: true, seruTrade: true, enemyAlly: true, shinySeru: true, jewelFix: true, approachFix: true, fortressFix: true, conkramFix: true,
    delilasChallenge: true, customItems: true,
    ...STARTING_BUNDLE, startingLevel: 10,
  },
};

window.LegaiaRomPatcher = Object.assign(window.LegaiaRomPatcher || {}, {
  SCALE_STATS, SCALE_GROUPS, splitScaleGroups, PRESETS,
});
