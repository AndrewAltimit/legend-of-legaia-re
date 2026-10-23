/* layout.js - shared app-shell layout for the multi-page site.
 *
 * Each page calls injectLayout({ active: 'subsystems/script-vm' }).
 * The site is split into two zones:
 *   - EXPLORE: the interactive pages (play, viewers, tables, patcher).
 *     App chrome only - top bar + icon rail, no sidebar, full width.
 *   - DOCS: guides / write-ups / subsystems / formats / tooling / reference.
 *     Same top bar + rail, plus a docs-scoped sidebar and the in-page TOC.
 *
 * injectLayout builds:
 *   - Fixed top bar: brand, Explore/Docs zone pills, search, disc chip, GitHub.
 *   - Fixed left icon rail (zone shortcuts).
 *   - Docs pages only: collapsible docs sidebar + TOC rail + prev/next footer.
 *   - Search overlay (filters NAV + headings + page snippets) on every page.
 *   - Mobile drawer toggle for the docs sidebar.
 *
 * The structure of NAV below is the single source of truth for nav ordering.
 */

/* Master output trim for every page that makes sound - play, the minigames,
 * the media browser.
 *
 * This is a LOUDNESS setting, not a mix control. The pages were simply too
 * loud on arrival, and the fix belongs at the output stage rather than in the
 * per-sound levels: the visible gain sliders (1x..10x on the media browser)
 * keep their whole range and their labels, and every relative balance between
 * BGM, SFX and one-shots is preserved, because everything downstream is
 * scaled by the same factor.
 *
 * Applied at each final GainNode -> destination hop. The WASM audio path
 * (the play page) carries the same trim in `engine-audio`'s WebAudioOut, so
 * a page mixing both sources stays balanced.
 *
 * Call sites use `window.LEGAIA_MASTER_TRIM ?? 0.25` - the literal there is a
 * fallback for a page that somehow loads without this file, not a second
 * source of truth. Change the value HERE.
 */
window.LEGAIA_MASTER_TRIM = 0.25;

const NAV = [
  {
    label: 'overview',
    title: 'Start here',
    items: [
      { href: 'index.html', text: 'Home', key: 'home' },
      { href: 'architecture.html', text: 'How it stacks', key: 'architecture' },
      { href: 'quickstart.html', text: 'Quick start', key: 'quickstart' },
      { href: 'crates.html', text: 'Crate map', key: 'crates' },
    ],
  },
  {
    label: 'guides',
    title: 'Guides',
    items: [
      { href: 'guides/getting-started.html', text: 'Getting started', key: 'guides/getting-started' },
      { href: 'guides/desktop-apps.html', text: 'Desktop apps', key: 'guides/desktop-apps' },
      { href: 'guides/extracting-assets.html', text: 'Extracting assets', key: 'guides/extracting-assets' },
      { href: 'guides/playing-and-viewing.html', text: 'Playing and viewing', key: 'guides/playing-and-viewing' },
      { href: 'guides/modding-and-translation.html', text: 'Modding and translation', key: 'guides/modding-and-translation' },
      { href: 'guides/translating.html', text: 'Translating the game', key: 'guides/translating', indent: true },
      { href: 'guides/vrchat-world.html', text: 'VRChat world', key: 'guides/vrchat-world' },
    ],
  },
  {
    label: 'explore',
    title: 'Explore',
    items: [
      { href: 'play.html',                      text: 'Play the port',            key: 'play' },
      { href: 'viewer.html',                    text: 'Asset viewer',             key: 'viewer' },
      { href: 'media.html',                     text: 'Media browser',            key: 'media' },
      { href: 'tooling/rom-patcher.html',       text: 'ROM patcher',              key: 'tooling/rom-patcher' },
      { href: 'tooling/translation-workbench.html', text: 'Translation workbench', key: 'tooling/translation-workbench' },
      { href: 'world.html',                     text: 'Game world',               key: 'world' },
      { href: 'shops.html',                     text: 'Shops & vendors',          key: 'shops' },
      { href: 'minigames.html',                 text: 'Minigames',                key: 'minigames' },
      { href: 'arts.html',                      text: 'Tactical Arts',            key: 'arts' },
      { href: 'magic.html',                     text: 'Seru magic & summons',     key: 'magic' },
      { href: 'monsters.html',                  text: 'Enemy table',              key: 'monsters' },
      { href: 'characters.html',                text: 'Characters',               key: 'characters' },
      { href: 'npcs.html',                      text: 'NPCs',                     key: 'npcs' },
      { href: 'world-overview.html',            text: 'World overview',           key: 'world-overview' },
      { href: 'disc-map.html',                  text: 'Disc map',                 key: 'disc-map' },
    ],
  },
  {
    label: 'write-ups',
    title: 'Write-ups',
    items: [
      { href: 'writeups/index.html', text: 'Technical write-ups', key: 'writeups/index' },
      { heading: 'Investigations' },
      { href: 'writeups/gaza-orbit-softlock.html', text: 'The endless camera orbit', key: 'writeups/gaza-orbit-softlock' },
      { href: 'writeups/spirit-fish.html', text: 'The Spirit fish gate', key: 'writeups/spirit-fish' },
      { heading: 'Glitches and oversights' },
      { href: 'writeups/glitches/index.html', text: 'Glitches, traced', key: 'writeups/glitches/index' },
      { href: 'writeups/glitches/name-centering.html', text: 'Off-centre attack names', key: 'writeups/glitches/name-centering' },
      { href: 'writeups/glitches/turn-order-and-arts.html', text: 'Turn order and double attacks', key: 'writeups/glitches/turn-order-and-arts' },
      { href: 'writeups/glitches/status-and-damage.html', text: 'Status rolls and immunities', key: 'writeups/glitches/status-and-damage' },
      { href: 'writeups/glitches/lapis-and-cort.html', text: 'Lapis and Mystic Shield Cort', key: 'writeups/glitches/lapis-and-cort' },
      { href: 'writeups/glitches/summons-and-jette.html', text: 'Ra-Seru levels and Jette\'s Clone', key: 'writeups/glitches/summons-and-jette' },
      { href: 'writeups/glitches/items-and-equipment.html', text: 'Items, stats and equipment', key: 'writeups/glitches/items-and-equipment' },
      { href: 'writeups/glitches/field-sequence-breaks.html', text: 'Incense and door skips', key: 'writeups/glitches/field-sequence-breaks' },
      { href: 'writeups/glitches/field-oddities.html', text: 'WORK ERR and script slips', key: 'writeups/glitches/field-oddities' },
      { heading: 'Disc patching' },
      { href: 'writeups/disc-patching/index.html', text: 'Patching a sealed disc', key: 'writeups/disc-patching/index' },
      { href: 'writeups/disc-patching/a-static-tables.html', text: 'Tier A: static tables', key: 'writeups/disc-patching/a-static-tables' },
      { href: 'writeups/disc-patching/b-lzs-slots.html', text: 'Tier B: editing inside LZS', key: 'writeups/disc-patching/b-lzs-slots' },
      { href: 'writeups/disc-patching/c-field-vm-operands.html', text: 'Tier C: field-VM bytecode', key: 'writeups/disc-patching/c-field-vm-operands' },
      { href: 'writeups/disc-patching/d-man-relocation.html', text: 'Tier D: MAN relocation', key: 'writeups/disc-patching/d-man-relocation' },
      { href: 'writeups/disc-patching/e-rodata-gap-code.html', text: 'Tier E: rodata-gap code', key: 'writeups/disc-patching/e-rodata-gap-code' },
      { href: 'writeups/disc-patching/f-overlay-dead-region.html', text: 'Tier F: overlay dead region', key: 'writeups/disc-patching/f-overlay-dead-region' },
      { href: 'writeups/disc-patching/g-ram-payload-carrier.html', text: 'Tier G: RAM-patch carrier', key: 'writeups/disc-patching/g-ram-payload-carrier' },
    ],
  },
  {
    label: 'subsystems',
    title: 'How the game works',
    /* Groups follow a player's path through the game: power on, walk a town,
       the interpreters behind it, a fight, the overworld, the menus, the side
       games, then the sound and picture under all of it. Pages about the
       Rust port and the site's viewers sit apart in "The port", so the retail
       groups describe only the shipped game.

       Label rules: every label stands alone (it is also the search result and
       the prev / next text), so it names its subsystem. A companion page is
       indented under its parent and labelled "<parent>: <topic>" - ": internals"
       for the reference tables, ": capture notes" for the measurements. */
    items: [
      { href: 'subsystems/index.html', text: 'Subsystems index', key: 'subsystems/index' },
      { heading: 'Startup and loading' },
      { href: 'subsystems/boot.html', text: 'Boot path', key: 'subsystems/boot' },
      { href: 'subsystems/boot-internals.html', text: 'Boot path: internals', key: 'subsystems/boot-internals', indent: true },
      { href: 'subsystems/asset-loader.html', text: 'Asset loader', key: 'subsystems/asset-loader' },
      { heading: 'Field: towns and dungeons' },
      { href: 'subsystems/field-locomotion.html', text: 'Field locomotion', key: 'subsystems/field-locomotion' },
      { href: 'subsystems/field-ambient-fx.html', text: 'Field ambient effects', key: 'subsystems/field-ambient-fx' },
      { href: 'subsystems/tile-board.html', text: 'Tile-board grid mode', key: 'subsystems/tile-board' },
      { heading: 'Script engines' },
      { href: 'subsystems/vm-inventory.html', text: 'VM inventory', key: 'subsystems/vm-inventory' },
      { href: 'subsystems/script-vm.html', text: 'Field / event VM', key: 'subsystems/script-vm' },
      { href: 'subsystems/script-vm-internals.html', text: 'Field VM: internals', key: 'subsystems/script-vm-internals', indent: true },
      { href: 'subsystems/script-vm-menuctrl.html', text: 'Field VM: op 0x4C', key: 'subsystems/script-vm-menuctrl', indent: true },
      { href: 'subsystems/motion-vm.html', text: 'Motion VMs', key: 'subsystems/motion-vm' },
      { href: 'subsystems/move-vm.html', text: 'Move-table VM', key: 'subsystems/move-vm' },
      { href: 'subsystems/move-vm-overlay-ext.html', text: 'Move VM: op 0x2F extension', key: 'subsystems/move-vm-overlay-ext', indent: true },
      { href: 'subsystems/actor-vm.html', text: 'Window-widget VM', key: 'subsystems/actor-vm' },
      { href: 'subsystems/effect-vm.html', text: 'Effect VM', key: 'subsystems/effect-vm' },
      { heading: 'Battle: rules' },
      { href: 'subsystems/battle.html', text: 'Battle overview', key: 'subsystems/battle' },
      { href: 'subsystems/battle-internals.html', text: 'Battle: internals', key: 'subsystems/battle-internals', indent: true },
      { href: 'subsystems/history-battle.html', text: 'Battle: capture notes', key: 'subsystems/history-battle', indent: true },
      { href: 'subsystems/battle-round-loop.html', text: 'Battle round loop', key: 'subsystems/battle-round-loop' },
      { href: 'subsystems/battle-command-flow.html', text: 'Battle command flow', key: 'subsystems/battle-command-flow' },
      { href: 'subsystems/arts-command-gauge.html', text: 'Arts command gauge', key: 'subsystems/arts-command-gauge', indent: true },
      { href: 'subsystems/battle-action.html', text: 'Battle action state machine', key: 'subsystems/battle-action' },
      { href: 'subsystems/battle-action-helpers.html', text: 'Battle action: helpers', key: 'subsystems/battle-action-helpers', indent: true },
      { href: 'subsystems/battle-action-exit-gates.html', text: 'Battle action: exit gates', key: 'subsystems/battle-action-exit-gates', indent: true },
      { href: 'subsystems/battle-action-queue.html', text: 'Battle action queue', key: 'subsystems/battle-action-queue' },
      { href: 'subsystems/cast-module.html', text: 'Cast modules', key: 'subsystems/cast-module' },
      { href: 'subsystems/battle-formulas.html', text: 'Battle formulas', key: 'subsystems/battle-formulas' },
      { href: 'subsystems/level-up.html', text: 'Level-up', key: 'subsystems/level-up' },
      { heading: 'Battle: on screen' },
      { href: 'subsystems/battle-stage-camera.html', text: 'Battle stage and camera', key: 'subsystems/battle-stage-camera' },
      { href: 'subsystems/battle-actor-rendering.html', text: 'Battle actor rendering', key: 'subsystems/battle-actor-rendering' },
      { href: 'subsystems/battle-hud.html', text: 'Battle HUD', key: 'subsystems/battle-hud' },
      { heading: 'World map and cutscenes' },
      { href: 'subsystems/world-map.html', text: 'World map', key: 'subsystems/world-map' },
      { href: 'subsystems/world-map-internals.html', text: 'World map: internals', key: 'subsystems/world-map-internals', indent: true },
      { href: 'subsystems/history-world-map.html', text: 'World map: chapter-1 sweep', key: 'subsystems/history-world-map', indent: true },
      { href: 'subsystems/cutscene.html', text: 'Cutscenes and movies', key: 'subsystems/cutscene' },
      { href: 'subsystems/cutscene-internals.html', text: 'Cutscenes: internals', key: 'subsystems/cutscene-internals', indent: true },
      { heading: 'Menus and town services' },
      { href: 'subsystems/field-menu.html', text: 'Pause menu', key: 'subsystems/field-menu' },
      { href: 'subsystems/inventory.html', text: 'Inventory', key: 'subsystems/inventory' },
      { href: 'subsystems/save-screen.html', text: 'Save screen', key: 'subsystems/save-screen' },
      { href: 'subsystems/shop.html', text: 'Shop', key: 'subsystems/shop' },
      { href: 'subsystems/inn.html', text: 'Inn', key: 'subsystems/inn' },
      { heading: 'Minigames' },
      { href: 'subsystems/minigame-fishing.html', text: 'Fishing', key: 'subsystems/minigame-fishing' },
      { href: 'subsystems/minigame-slot-machine.html', text: 'Slot machine', key: 'subsystems/minigame-slot-machine' },
      { href: 'subsystems/minigame-baka-fighter.html', text: 'Baka Fighter', key: 'subsystems/minigame-baka-fighter' },
      { href: 'subsystems/minigame-dance.html', text: 'Dance', key: 'subsystems/minigame-dance' },
      { href: 'subsystems/minigame-muscle-dome.html', text: 'Muscle Dome', key: 'subsystems/minigame-muscle-dome' },
      { heading: 'Sound and picture' },
      { href: 'subsystems/renderer.html', text: 'Renderer', key: 'subsystems/renderer' },
      { href: 'subsystems/renderer-internals.html', text: 'Renderer: internals', key: 'subsystems/renderer-internals', indent: true },
      { href: 'subsystems/shading.html', text: 'Shading and palettes', key: 'subsystems/shading' },
      { href: 'subsystems/audio.html', text: 'Audio', key: 'subsystems/audio' },
      { href: 'subsystems/audio-internals.html', text: 'Audio: internals', key: 'subsystems/audio-internals', indent: true },
      { heading: 'The port' },
      { href: 'subsystems/engine.html', text: 'Engine reimplementation', key: 'subsystems/engine' },
      { href: 'subsystems/renderer-port.html', text: 'Port renderer (wgpu)', key: 'subsystems/renderer-port' },
      { href: 'subsystems/browser-host.html', text: 'Browser host', key: 'subsystems/browser-host' },
      { href: 'subsystems/vr-mode.html', text: 'VR mode (WebXR)', key: 'subsystems/vr-mode' },
      { href: 'subsystems/world-overview-viewer.html', text: 'World-overview viewer', key: 'subsystems/world-overview-viewer' },
    ],
  },
  {
    label: 'formats',
    title: 'File formats',
    /* Groups follow the disc's layering, outermost first: the archive, how
       an entry is unpacked and routed, the bundles a scene or a battle loads
       as a unit, then single assets by kind, the static tables, and the
       archive entries that hold code. formats/index.html and
       docs/formats/overview.md use the same groups in the same order. */
    items: [
      { href: 'formats/index.html', text: 'Formats index', key: 'formats/index' },
      { heading: 'Disc and archive' },
      { href: 'formats/disc.html', text: 'PSX disc geometry', key: 'formats/disc' },
      { href: 'formats/prot.html', text: 'PROT.DAT archive', key: 'formats/prot' },
      { href: 'formats/cdname.html', text: 'CDNAME.TXT name map', key: 'formats/cdname' },
      { href: 'formats/dmy.html', text: 'DMY.DAT dev fixtures', key: 'formats/dmy' },
      { href: 'formats/pochi.html', text: 'Pochi fill slots', key: 'formats/pochi' },
      { heading: 'Compression and dispatch' },
      { href: 'formats/lzs.html', text: 'LZS compression', key: 'formats/lzs' },
      { href: 'formats/data-field.html', text: 'DATA_FIELD chunk stream', key: 'formats/data-field' },
      { href: 'formats/asset-type.html', text: 'Asset type dispatcher', key: 'formats/asset-type' },
      { href: 'formats/asset-descriptor.html', text: 'Asset descriptor', key: 'formats/asset-descriptor' },
      { href: 'formats/pack.html', text: 'Pack format', key: 'formats/pack' },
      { href: 'formats/tim-pack.html', text: 'TIM-pack reader', key: 'formats/tim-pack' },
      { href: 'formats/field-pack.html', text: 'Field-pack (not a format)', key: 'formats/field-pack' },
      { heading: 'Scene and battle bundles' },
      { href: 'formats/scene-bundles.html', text: 'Scene bundles', key: 'formats/scene-bundles' },
      { href: 'formats/field-map.html', text: 'Field map (.MAP)', key: 'formats/field-map' },
      { href: 'formats/scene-v12-table.html', text: 'Walk-on triggers (.PCH)', key: 'formats/scene-v12-table' },
      { href: 'formats/encounter.html', text: 'Encounter record', key: 'formats/encounter' },
      { href: 'formats/man-relocation.html', text: 'MAN relocation', key: 'formats/man-relocation' },
      { href: 'formats/world-map-overlay.html', text: 'World-map animation bank', key: 'formats/world-map-overlay' },
      { href: 'formats/str-fmv-table.html', text: 'FMV dispatch table', key: 'formats/str-fmv-table' },
      { href: 'formats/effect.html', text: 'Effect bundles', key: 'formats/effect' },
      { href: 'formats/summon-readef.html', text: 'Summon and readef slots', key: 'formats/summon-readef' },
      { href: 'formats/navmesh.html', text: 'Not a navmesh: scratch buffer', key: 'formats/navmesh' },
      { heading: 'Textures and models' },
      { href: 'formats/tim.html', text: 'PSX TIM', key: 'formats/tim' },
      { href: 'formats/tmd.html', text: 'Legaia TMD', key: 'formats/tmd' },
      { href: 'formats/npc-palette.html', text: 'NPC palettes (row 479)', key: 'formats/npc-palette' },
      { href: 'formats/save-icon.html', text: 'Save-slot portraits', key: 'formats/save-icon' },
      { href: 'formats/ringside-still.html', text: 'Ringside stills', key: 'formats/ringside-still' },
      { heading: 'Characters and animation' },
      { href: 'formats/character-mesh.html', text: 'Character meshes', key: 'formats/character-mesh' },
      { href: 'formats/battle-data-pack.html', text: 'Player battle files', key: 'formats/battle-data-pack' },
      { href: 'formats/anm.html', text: 'ANM animation', key: 'formats/anm' },
      { href: 'formats/mdt.html', text: 'MDT clip bank', key: 'formats/mdt' },
      { href: 'formats/monster-animation.html', text: 'Monster animation', key: 'formats/monster-animation' },
      { href: 'formats/art-data.html', text: 'Art data', key: 'formats/art-data' },
      { heading: 'Sound' },
      { href: 'formats/vab.html', text: 'VAB sound bank', key: 'formats/vab' },
      { href: 'formats/seq.html', text: 'SEQ sequences', key: 'formats/seq' },
      { href: 'formats/xa.html', text: 'XA-ADPCM audio', key: 'formats/xa' },
      { href: 'formats/sfx-table.html', text: 'SFX table', key: 'formats/sfx-table' },
      { href: 'formats/bse-dat.html', text: 'bse.dat battle SFX bank', key: 'formats/bse-dat' },
      { href: 'formats/sound-driver.html', text: 'Sound-driver files', key: 'formats/sound-driver' },
      { heading: 'Text' },
      { href: 'formats/mes.html', text: 'MES dialog', key: 'formats/mes' },
      { href: 'formats/dialog-font.html', text: 'Dialog font', key: 'formats/dialog-font' },
      { href: 'formats/place-names.html', text: 'Place names', key: 'formats/place-names' },
      { heading: 'Static tables' },
      { href: 'formats/item-table.html', text: 'Item-name table', key: 'formats/item-table' },
      { href: 'formats/item-effect-table.html', text: 'Item-effect table', key: 'formats/item-effect-table' },
      { href: 'formats/equipment-table.html', text: 'Equipment stats table', key: 'formats/equipment-table' },
      { href: 'formats/accessory-passive-table.html', text: 'Accessory passives', key: 'formats/accessory-passive-table' },
      { href: 'formats/spell-table.html', text: 'Spell table', key: 'formats/spell-table' },
      { href: 'formats/steal-table.html', text: 'Steal table', key: 'formats/steal-table' },
      { href: 'formats/move-power.html', text: 'Move-power table', key: 'formats/move-power' },
      { href: 'formats/battle-attack-camera-table.html', text: 'Attack-camera table', key: 'formats/battle-attack-camera-table' },
      { href: 'formats/new-game-table.html', text: 'New-game party', key: 'formats/new-game-table' },
      { href: 'formats/save-record.html', text: 'Character save record', key: 'formats/save-record' },
      { href: 'formats/window-script.html', text: 'Window widget scripts', key: 'formats/window-script' },
      { heading: 'Code overlays' },
      { href: 'formats/mips-overlay.html', text: 'MIPS overlay code', key: 'formats/mips-overlay' },
      { href: 'formats/overlay-ptr-table.html', text: 'Overlay pointer tables', key: 'formats/overlay-ptr-table' },
      { href: 'formats/slot-b-module-layout.html', text: 'Slot-B module layout', key: 'formats/slot-b-module-layout' },
    ],
  },
  {
    label: 'tooling',
    title: 'Tools and methods',
    /* Task groups, in the order tooling/index.html presents them. The two
       in-browser tools (ROM patcher, translation workbench) are explore
       pages and live in the rail, not here. */
    items: [
      { href: 'tooling/index.html', text: 'Tooling index', key: 'tooling/index' },
      { heading: 'Extract assets' },
      { href: 'tooling/extraction.html', text: 'Extraction CLIs', key: 'tooling/extraction' },
      { heading: 'Patch a disc' },
      { href: 'tooling/randomizer.html', text: 'Randomizer and disc patcher', key: 'tooling/randomizer' },
      { href: 'tooling/randomizer-internals.html', text: 'Randomizer: internals', key: 'tooling/randomizer-internals', indent: true },
      { href: 'tooling/randomizer-delilas.html', text: 'Delilas Challenge and party swap', key: 'tooling/randomizer-delilas', indent: true },
      { href: 'tooling/translation.html', text: 'Translation packs', key: 'tooling/translation' },
      { href: 'tooling/pal-localizations.html', text: 'Official PAL localizations', key: 'tooling/pal-localizations', indent: true },
      { heading: 'Dig in Ghidra' },
      { href: 'tooling/ghidra.html', text: 'Ghidra in Docker', key: 'tooling/ghidra' },
      { href: 'tooling/static-overlay-pipeline.html', text: 'Static overlay pipeline', key: 'tooling/static-overlay-pipeline' },
      { href: 'tooling/address-reference-scan.html', text: 'Address-reference scan', key: 'tooling/address-reference-scan' },
      { heading: 'Capture the emulator' },
      { href: 'tooling/overlay-capture.html', text: 'Overlay capture', key: 'tooling/overlay-capture' },
      { href: 'tooling/mednafen-automation.html', text: 'Mednafen automation', key: 'tooling/mednafen-automation' },
      { href: 'tooling/pcsx-redux-automation.html', text: 'PCSX-Redux automation', key: 'tooling/pcsx-redux-automation' },
      { href: 'tooling/spine-flag-writers-capture.html', text: 'Spine flag-writers capture', key: 'tooling/spine-flag-writers-capture', indent: true },
      { heading: 'Measure the port' },
      { href: 'tooling/port-catalog.html', text: 'Port catalog', key: 'tooling/port-catalog' },
      { href: 'tooling/disc-coverage.html', text: 'Disc coverage', key: 'tooling/disc-coverage' },
      { href: 'tooling/byte-accounting.html', text: 'Byte accounting', key: 'tooling/byte-accounting', indent: true },
      { href: 'tooling/recomp-differential.html', text: 'Recomp differential', key: 'tooling/recomp-differential' },
      { href: 'tooling/determinism-replay.html', text: 'Determinism replay', key: 'tooling/determinism-replay' },
      { href: 'tooling/retail-compare.html', text: 'Retail comparison corpus', key: 'tooling/retail-compare' },
      { href: 'tooling/full-game-ladder.html', text: 'Full-game ladder', key: 'tooling/full-game-ladder' },
      { href: 'tooling/soak-harness.html', text: 'Soak harness', key: 'tooling/soak-harness' },
    ],
  },
  {
    label: 'reference',
    title: 'Reference',
    items: [
      { href: 'reference/index.html', text: 'Reference index', key: 'reference/index' },
      { heading: 'Code and memory' },
      { href: 'reference/functions.html', text: 'Key functions', key: 'reference/functions' },
      { href: 'reference/memory-map.html', text: 'PSX RAM map', key: 'reference/memory-map' },
      { href: 'reference/builds.html', text: 'Builds and debug features', key: 'reference/builds' },
      { heading: 'Names and labels' },
      { href: 'reference/gamedata.html', text: 'Game-data tables', key: 'reference/gamedata' },
      { href: 'reference/cheats.html', text: 'Cheat databases', key: 'reference/cheats' },
      { href: 'reference/music-tracks.html', text: 'Music tracks', key: 'reference/music-tracks' },
      { href: 'reference/scene-names.html', text: 'Scene names', key: 'reference/scene-names' },
      { heading: 'State of the research' },
      { href: 'reference/open-rev-eng-threads.html', text: 'Open RE threads', key: 'reference/open-rev-eng-threads' },
      { href: 'reference/re-settled-threads.html', text: 'Settled RE threads', key: 'reference/re-settled-threads' },
      { href: 'reference/re-do-not-re-walk.html', text: 'Do not re-walk', key: 'reference/re-do-not-re-walk' },
    ],
  },
];

/* ---------- Zones ---------- */
/* Interactive pages get app chrome (no sidebar); everything else is docs. */
const EXPLORE_KEYS = new Set([
  'home', 'play', 'viewer', 'media', 'tooling/rom-patcher', 'tooling/translation-workbench', 'world', 'shops',
  'minigames', 'arts', 'magic', 'monsters', 'characters', 'npcs', 'world-overview', 'disc-map',
]);
/* Docs sections that open collapsed unless they hold the current page (or the
   reader opened them before): each is a long reference list, and five of
   them expanded at once bury the short sections a newcomer wants first. */
const COLLAPSED_BY_DEFAULT = new Set(['write-ups', 'subsystems', 'formats', 'tooling', 'reference']);
/* NAV sections rendered in the docs sidebar (order preserved). The 'explore'
   section is deliberately absent - those pages live in the rail + home grid. */
const DOCS_SECTIONS = new Set(['overview', 'guides', 'write-ups', 'subsystems', 'formats', 'tooling', 'reference']);

function zoneForKey(key) {
  return EXPLORE_KEYS.has(key || 'home') ? 'explore' : 'docs';
}

/* Explore sidebar: every interactive page, grouped like the home launcher,
   so each one is reachable from the left nav on any explore page. */
const EXPLORE_GROUPS = [
  { label: 'play',           title: 'Play', keys: ['play', 'minigames'] },
  { label: 'modding',        title: 'Mod your disc', keys: ['tooling/rom-patcher', 'tooling/translation-workbench'] },
  { label: 'browse the disc', title: 'Explore the game data', keys: [
    '#Viewers', 'viewer', 'media', 'world-overview', 'disc-map',
    '#Game guide', 'world', 'characters', 'npcs', 'monsters', 'arts', 'magic', 'shops',
  ] },
];

function exploreNavSections() {
  const byKey = new Map();
  for (const section of NAV) for (const item of section.items) if (item.key) byKey.set(item.key, item);
  return EXPLORE_GROUPS.map(g => ({
    label: g.label,
    title: g.title,
    /* A '#Name' key is a subhead row, rendered like a docs group heading. */
    items: g.keys.map(k => (k.startsWith('#') ? { heading: k.slice(1) } : byKey.get(k))).filter(Boolean),
  }));
}

/* Icon rail: one entry per zone shortcut. `match` marks the entry active. */
const RAIL = [
  { label: 'Home',   href: 'index.html',              match: k => k === 'home',
    icon: '<path d="M4 11.5 12 5l8 6.5"/><path d="M6.5 10.5V19h11v-8.5"/>' },
  { label: 'Play',   href: 'play.html',               match: k => k === 'play' || k === 'minigames',
    icon: '<rect x="3" y="8" width="18" height="9" rx="4.5"/><path d="M8 11v3M6.5 12.5h3"/><circle cx="15.5" cy="11.5" r="0.9"/><circle cx="17.8" cy="13.4" r="0.9"/>' },
  { label: 'Mods',   href: 'tooling/rom-patcher.html', match: k => k === 'tooling/rom-patcher' || k === 'tooling/translation-workbench',
    icon: '<path d="M4 8h4c3.5 0 4.5 8 8 8h4M4 16h4c1.4 0 2.4-1.1 3.2-2.3M12.8 10.2C13.8 9 14.8 8 16 8h4"/><path d="M18 6l2.5 2L18 10M18 14l2.5 2-2.5 2"/>' },
  { label: 'Browse', href: 'viewer.html',             match: k => ['viewer', 'media', 'world', 'world-overview', 'characters', 'npcs', 'monsters', 'shops', 'arts', 'magic', 'disc-map'].includes(k),
    icon: '<path d="M7 9 12 6l5 3v6l-5 3-5-3z"/><path d="M7 9l5 3 5-3M12 12v6"/>' },
  { label: 'Docs',   href: 'architecture.html',       match: (k, zone) => zone === 'docs',
    icon: '<path d="M4 6.5C5.5 5.5 7.5 5 9 5s3 .5 3 .5V19s-1.5-.5-3-.5-3.5.5-5 1.5zM20 6.5C18.5 5.5 16.5 5 15 5s-3 .5-3 .5V19s1.5-.5 3-.5 3.5.5 5 1.5z"/>' },
];

/* ---------- Helpers ---------- */
function resolveHref(href, depth) {
  if (depth === 0) return href;
  if (/^https?:/.test(href)) return href;
  return '../'.repeat(depth) + href;
}

function depthFromKey(key) {
  if (!key || key === 'home') return 0;
  return key.split('/').length - 1;
}

function flattenNav() {
  const out = [];
  for (const section of NAV) for (const item of section.items) if (item.key) out.push(item);
  return out;
}

function findSiblings(activeKey) {
  /* Prev/next stays within the docs zone: walking off the end of `formats`
     into an interactive page (or vice versa) made no sense as reading order. */
  const flat = [];
  for (const section of NAV) {
    if (!DOCS_SECTIONS.has(section.label)) continue;
    for (const item of section.items) if (item.key && item.key !== 'home') flat.push(item);
  }
  const idx = flat.findIndex(x => x.key === activeKey);
  if (idx < 0) return { prev: null, next: null };
  return {
    prev: idx > 0 ? flat[idx - 1] : null,
    next: idx < flat.length - 1 ? flat[idx + 1] : null,
  };
}

function slugify(s) {
  return s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '');
}

/* ---------- Zone sidebar (docs tree, or grouped explore pages) ---------- */
function buildSidebar(active, depth, zone) {
  const sidebar = document.createElement('aside');
  sidebar.className = 'sidebar';
  sidebar.id = 'sidebar';

  /* Zone shortcuts, mirroring the icon rail. The rail is a different
     destination set from the page list below it, and the rail is hidden at
     the drawer breakpoint - so without this strip a phone loses every
     top-level destination. Hidden by CSS while the rail is on screen. */
  sidebar.appendChild(buildZoneShortcuts(active, zone, depth));

  const head = document.createElement('div');
  head.className = 'sidebar-head';
  head.textContent = zone === 'explore' ? 'Explore' : 'Documentation';
  sidebar.appendChild(head);

  const sections = zone === 'explore'
    ? exploreNavSections()
    : NAV.filter(s => DOCS_SECTIONS.has(s.label));
  for (const section of sections) {
    const sec = document.createElement('div');
    sec.className = 'sidebar-section';
    sec.dataset.section = section.label;

    const hasActive = section.items.some(item => item.key === active);
    if (hasActive) sec.classList.add('has-active');

    /* Section header (toggle) */
    const tog = document.createElement('button');
    tog.type = 'button';
    tog.className = 'sidebar-section-toggle';
    tog.innerHTML = '<span class="arrow">▾</span>' + (section.title || section.label);
    tog.setAttribute('aria-expanded', 'true');
    tog.addEventListener('click', () => {
      sec.classList.toggle('collapsed');
      tog.setAttribute('aria-expanded', String(!sec.classList.contains('collapsed')));
      try {
        const persisted = JSON.parse(localStorage.getItem('sidebar-collapsed') || '{}');
        persisted[section.label] = sec.classList.contains('collapsed');
        localStorage.setItem('sidebar-collapsed', JSON.stringify(persisted));
      } catch (e) {}
    });
    sec.appendChild(tog);

    /* Item list */
    const nav = document.createElement('nav');
    nav.className = 'sidebar-nav';
    nav.setAttribute('aria-label', section.title || section.label);
    /* A `{ heading }` row opens a group: the subhead plus every item up to
       the next heading sit in one .sidebar-group, so CSS can draw the group
       as a tree branch (guide rail down the left of its items). */
    let parent = nav;
    for (const item of section.items) {
      if (item.heading) {
        /* A group subhead inside a long section - not a link, not a page. */
        const group = document.createElement('div');
        group.className = 'sidebar-group';
        const h = document.createElement('div');
        h.className = 'sidebar-subhead';
        h.textContent = item.heading;
        group.appendChild(h);
        parent = document.createElement('div');
        parent.className = 'sidebar-group-items';
        group.appendChild(parent);
        nav.appendChild(group);
        continue;
      }
      if (item.key === 'home') continue; /* Home lives in the rail, not the docs tree */
      const a = document.createElement('a');
      a.href = resolveHref(item.href, depth);
      a.textContent = item.text;
      a.dataset.key = item.key;
      if (item.key === active) { a.classList.add('active'); a.setAttribute('aria-current', 'page'); }
      if (item.indent) a.classList.add('nav-child');
      parent.appendChild(a);
      if (item.key === active && parent !== nav) parent.parentNode.classList.add('has-active');
    }
    sec.appendChild(nav);

    /* Restore collapsed state from localStorage (don't collapse the active section) */
    let collapsed = COLLAPSED_BY_DEFAULT.has(section.label);
    try {
      const persisted = JSON.parse(localStorage.getItem('sidebar-collapsed') || '{}');
      if (section.label in persisted) collapsed = !!persisted[section.label];
    } catch (e) {}
    if (collapsed && !hasActive) {
      sec.classList.add('collapsed');
      tog.setAttribute('aria-expanded', 'false');
    }

    sidebar.appendChild(sec);
  }

  const foot = document.createElement('div');
  foot.className = 'sidebar-foot';
  foot.innerHTML =
    '<a href="https://github.com/AndrewAltimit/legend-of-legaia-re" target="_blank" rel="noopener">GitHub →</a><br>' +
    'Tooling: MIT or Unlicense.<br>' +
    'No Sony bytes shipped.';
  sidebar.appendChild(foot);

  return sidebar;
}

/* ---------- Breadcrumb ----------
 * Every docs page's header carries a hand-written `.breadcrumb`, the no-JS
 * fallback. Written by hand, the trails drifted: some linked their section and
 * some did not, and the leaf text was whatever the page's first draft called
 * itself. Rebuilt here from NAV, every trail reads the same way - Home, the
 * section (linked to its index page when it has one), any index page the page
 * sits under (a write-up series, an indented companion's parent), then the
 * page's own sidebar label. */
function syncBreadcrumb(active, depth) {
  const crumb = document.querySelector('.page-header .breadcrumb');
  if (!crumb) return;
  const section = NAV.find(s => DOCS_SECTIONS.has(s.label) && s.items.some(i => i.key === active));
  if (!section) return;
  const pages = section.items.filter(i => i.key);
  const byKey = new Map(pages.map(i => [i.key, i]));
  const sectionIndex = pages.find(i => /^[^/]+\/index$/.test(i.key));
  const trail = [{ text: 'Home', href: 'index.html' }];
  if (sectionIndex && sectionIndex.key === active) {
    trail.push({ text: section.title });
  } else {
    trail.push({ text: section.title, href: sectionIndex && sectionIndex.href });
    /* A nested series index: writeups/glitches/x sits under writeups/glitches/index. */
    const parts = active.split('/');
    for (let n = 2; n < parts.length; n++) {
      const parentKey = parts.slice(0, n).join('/') + '/index';
      const parent = byKey.get(parentKey);
      if (parent && parentKey !== active) trail.push({ text: parent.text, href: parent.href });
    }
    /* An indented companion page: its parent is the nearest unindented row above it. */
    const at = section.items.findIndex(i => i.key === active);
    const self = section.items[at];
    if (self.indent) {
      for (let j = at - 1; j >= 0; j--) {
        const up = section.items[j];
        if (!up.key || up.indent) continue;
        if (!trail.some(t => t.href === up.href)) trail.push({ text: up.text, href: up.href });
        break;
      }
    }
    trail.push({ text: self.text });
  }
  crumb.textContent = '';
  trail.forEach((step, k) => {
    if (k) crumb.appendChild(document.createTextNode(' · '));
    if (step.href) {
      const a = document.createElement('a');
      a.href = resolveHref(step.href, depth);
      a.textContent = step.text;
      crumb.appendChild(a);
    } else {
      crumb.appendChild(document.createTextNode(step.text));
    }
  });
}

/* ---------- Wide tables ----------
 * A bare <table> is `display: table`, and overflow does not create a scroll
 * container on a table box - so a wide reference table pushes the whole page
 * sideways on a phone. `.table-wrap` is the scrolling container the generated
 * pages already use; give every prose table the same one. Runs at layout time,
 * before any page script builds its own tables, so it only ever touches the
 * static markup. */
function wrapWideTables() {
  const content = document.querySelector('.content');
  if (!content) return;
  content.querySelectorAll('table').forEach(table => {
    const parent = table.parentElement;
    if (!parent || parent.classList.contains('table-wrap')) return;
    const wrap = document.createElement('div');
    wrap.className = 'table-wrap';
    parent.insertBefore(wrap, table);
    wrap.appendChild(table);
  });
}

/* ---------- Sticky table headers ----------
 * A header can stick to the page only if no ancestor of the table is a scroll
 * container - and `.table-wrap` is one, by design. So a wrapper is told when
 * its table FITS (`.fits` drops the scroller and turns on the sticky header)
 * and keeps the horizontal scroller only when the table is really too wide.
 * Docs zone only: the interactive pages build and size their own tables.
 * Measured against the wrapper's own box, so it stays right across the
 * breakpoints; re-run on resize and once fonts have settled. */
function fitTables() {
  const wraps = document.querySelectorAll('.zone-docs .content .table-wrap');
  wraps.forEach(wrap => {
    const table = wrap.querySelector(':scope > table');
    if (!table) return;
    wrap.classList.toggle('fits', table.scrollWidth <= wrap.clientWidth + 1);
  });
}

/* ---------- Theme toggle ----------
 * The page template's head script has already applied a stored choice before
 * first paint; this only wires the button. Dark is the default. */
function buildThemeToggle() {
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'tb-theme';
  const sun = '<circle cx="12" cy="12" r="4"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M18.4 5.6 17 7M7 17l-1.4 1.4"/>';
  const moon = '<path d="M20 14.5A8 8 0 0 1 9.5 4a7 7 0 1 0 10.5 10.5z"/>';
  function current() {
    return document.documentElement.getAttribute('data-theme') === 'light' ? 'light' : 'dark';
  }
  function render() {
    const light = current() === 'light';
    btn.innerHTML = railSvg(light ? moon : sun);
    btn.setAttribute('aria-label', light ? 'Switch to dark theme' : 'Switch to light theme');
    btn.title = light ? 'Dark theme' : 'Light theme';
  }
  btn.addEventListener('click', () => {
    const next = current() === 'light' ? 'dark' : 'light';
    if (next === 'light') document.documentElement.setAttribute('data-theme', 'light');
    else document.documentElement.removeAttribute('data-theme');
    try { localStorage.setItem('site-theme', next); } catch (e) {}
    render();
  });
  render();
  return btn;
}

/* ---------- Heading ID assignment (before anchors / TOC) ---------- */
function assignHeadingIds() {
  const content = document.querySelector('.content');
  if (!content) return;
  content.querySelectorAll('section.doc-section h2, section.doc-section h3, section.doc-section h4').forEach(h => {
    if (h.id) return;
    const sec = h.closest('section.doc-section');
    if (h.tagName === 'H2' && sec && sec.id) {
      h.id = sec.id;
    } else {
      h.id = slugify(h.textContent || '') || ('h-' + Math.random().toString(36).slice(2, 8));
    }
  });
}

/* ---------- TOC rail ---------- */
function buildTocRail() {
  const content = document.querySelector('.content');
  if (!content) return null;

  /* Only consider h2 and h3 inside doc-section (not the page-header h1). */
  const headings = content.querySelectorAll('section.doc-section h2, section.doc-section h3');
  if (headings.length < 2) return null;

  const rail = document.createElement('aside');
  rail.className = 'toc-rail';
  rail.setAttribute('aria-label', 'On this page');

  const title = document.createElement('div');
  title.className = 'toc-title';
  title.textContent = 'On this page';
  rail.appendChild(title);

  const list = document.createElement('ul');
  list.className = 'toc-list';

  headings.forEach(h => {
    const li = document.createElement('li');
    const a = document.createElement('a');
    a.href = '#' + h.id;
    a.textContent = (h.textContent || '').trim();
    a.dataset.target = h.id;
    if (h.tagName === 'H3') a.classList.add('h3');
    li.appendChild(a);
    list.appendChild(li);
  });

  rail.appendChild(list);
  return rail;
}

/* ---------- Heading anchor links (clickable § on h2/h3/h4) ---------- */
/* Call AFTER assignHeadingIds() and AFTER buildTocRail() so the § isn't
   captured into TOC link text. */
function injectHeadingAnchors() {
  const content = document.querySelector('.content');
  if (!content) return;
  content.querySelectorAll('section.doc-section h2, section.doc-section h3, section.doc-section h4').forEach(h => {
    if (h.querySelector('.h-anchor') || !h.id) return;
    const a = document.createElement('a');
    a.className = 'h-anchor';
    a.href = '#' + h.id;
    a.setAttribute('aria-label', 'Anchor link');
    a.textContent = '§';
    h.appendChild(a);
  });
}

/* ---------- Prev/next footer ---------- */
function buildPageNav(active, depth) {
  const { prev, next } = findSiblings(active);
  if (!prev && !next) return null;

  const nav = document.createElement('nav');
  nav.className = 'page-nav';
  nav.setAttribute('aria-label', 'Previous and next page');

  if (prev) {
    const a = document.createElement('a');
    a.href = resolveHref(prev.href, depth);
    a.className = 'pn-prev';
    a.innerHTML =
      '<div class="pn-label">Previous</div>' +
      '<div class="pn-title">' + prev.text + '</div>';
    nav.appendChild(a);
  }
  if (next) {
    const a = document.createElement('a');
    a.href = resolveHref(next.href, depth);
    a.className = 'pn-next';
    a.innerHTML =
      '<div class="pn-label">Next</div>' +
      '<div class="pn-title">' + next.text + '</div>';
    nav.appendChild(a);
  }
  return nav;
}

/* ---------- Mobile toggle button ----------
 * Lives inside the top bar rather than floating over the page, so the content
 * column needs no reserved strip and the button can never sit on top of a
 * paragraph. */
function buildMobileToggle() {
  const toggle = document.createElement('button');
  toggle.type = 'button';
  toggle.className = 'sidebar-toggle';
  toggle.setAttribute('aria-label', 'Toggle navigation');
  toggle.setAttribute('aria-expanded', 'false');
  toggle.setAttribute('aria-controls', 'sidebar');
  toggle.innerHTML = '&#9776;';
  return toggle;
}

/* ---------- Top bar ---------- */
function railSvg(paths) {
  return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" ' +
         'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' + paths + '</svg>';
}

function buildTopbar(active, zone, depth) {
  const bar = document.createElement('header');
  bar.className = 'topbar';
  bar.innerHTML =
    '<a class="tb-brand" href="' + resolveHref('index.html', depth) + '">' +
      '<span class="tb-mark">LR</span>' +
      '<span class="tb-name">Legend of Legaia <em>RE</em></span>' +
    '</a>' +
    '<nav class="tb-zones" aria-label="Site zones">' +
      '<a class="tb-zone' + (zone === 'explore' ? ' active' : '') + '" href="' + resolveHref('index.html', depth) + '">Explore</a>' +
      '<a class="tb-zone' + (zone === 'docs' ? ' active' : '') + '" href="' + resolveHref('architecture.html', depth) + '">Docs</a>' +
    '</nav>' +
    '<div class="tb-spacer"></div>' +
    '<button type="button" class="tb-search" id="open-search" aria-label="Open search">' +
      '<span class="icon">⌕</span><span class="label">Search</span><span class="kbd">/</span>' +
    '</button>' +
    '<button type="button" class="disc-chip" id="disc-chip" title="The disc is read locally and cached in this browser - nothing is uploaded.">' +
      '<span class="dot"></span><span class="txt">Checking disc…</span>' +
    '</button>' +
    '<a class="tb-gh" href="https://github.com/AndrewAltimit/legend-of-legaia-re" target="_blank" rel="noopener" aria-label="GitHub">' +
      railSvg('<path d="M12 3a9 9 0 0 0-2.85 17.55c.45.08.62-.2.62-.44v-1.7c-2.5.55-3.03-1.06-3.03-1.06-.41-1.04-1-1.32-1-1.32-.82-.56.06-.55.06-.55.9.06 1.38.93 1.38.93.8 1.38 2.11.98 2.63.75.08-.58.31-.98.57-1.2-2-.23-4.1-1-4.1-4.45 0-.98.35-1.78.93-2.41-.1-.23-.4-1.15.08-2.4 0 0 .76-.24 2.48.92a8.6 8.6 0 0 1 4.51 0c1.72-1.16 2.47-.92 2.47-.92.49 1.25.19 2.17.1 2.4.58.63.92 1.43.92 2.41 0 3.47-2.1 4.22-4.11 4.44.32.28.61.83.61 1.67v2.47c0 .24.16.53.62.44A9 9 0 0 0 12 3z"/>') +
    '</a>';
  return bar;
}

/* ---------- Disc chip: global cached-disc state, insertable from any page ----------
 * The chip peeks at the shared IndexedDB cache (via RomCache when the page
 * loads it, else a self-contained read of the same store) and doubles as a
 * global "insert disc" control: clicking it picks a .bin and stores it, so
 * every disc-driven page auto-loads from then on. */
function peekDiscMeta() {
  if (window.RomCache && window.RomCache.meta) return window.RomCache.meta();
  return new Promise(function (resolve) {
    if (typeof indexedDB === 'undefined') return resolve(null);
    let req;
    try { req = indexedDB.open('legaia-rom-cache', 1); } catch (e) { return resolve(null); }
    req.onupgradeneeded = function () {
      const db = req.result;
      if (!db.objectStoreNames.contains('disc')) db.createObjectStore('disc');
    };
    req.onerror = function () { resolve(null); };
    req.onsuccess = function () {
      const db = req.result;
      try {
        const get = db.transaction('disc', 'readonly').objectStore('disc').get('current');
        get.onsuccess = function () {
          const rec = get.result;
          db.close();
          resolve(rec ? { name: rec.name, size: rec.size } : null);
        };
        get.onerror = function () { db.close(); resolve(null); };
      } catch (e) { db.close(); resolve(null); }
    };
  });
}

function storeDisc(file) {
  if (window.RomCache && window.RomCache.put) return window.RomCache.put(file);
  return new Promise(function (resolve, reject) {
    let req;
    try { req = indexedDB.open('legaia-rom-cache', 1); } catch (e) { return reject(e); }
    req.onupgradeneeded = function () {
      const db = req.result;
      if (!db.objectStoreNames.contains('disc')) db.createObjectStore('disc');
    };
    req.onerror = function () { reject(req.error); };
    req.onsuccess = function () {
      const db = req.result;
      const t = db.transaction('disc', 'readwrite');
      t.objectStore('disc').put({
        name: file.name || 'disc.bin', size: file.size,
        type: file.type || '', savedAt: Date.now(), blob: file,
      }, 'current');
      t.oncomplete = function () { db.close(); resolve(); };
      t.onerror = function () { db.close(); reject(t.error); };
    };
  });
}

function wireDiscChip() {
  const chip = document.getElementById('disc-chip');
  if (!chip) return;
  const dot = chip.querySelector('.dot');
  const txt = chip.querySelector('.txt');

  function render(meta) {
    if (meta && meta.name) {
      chip.classList.add('loaded');
      chip.classList.remove('empty');
      txt.textContent = meta.name;
      chip.title = meta.name + ' is cached in this browser and feeds every page. Click to swap discs. Nothing is uploaded.';
      /* The label text is hidden on a small phone, leaving only the status
         dot - so the accessible name has to be stated, not inferred. */
      chip.setAttribute('aria-label', 'Disc loaded: ' + meta.name + '. Click to swap discs.');
    } else {
      chip.classList.add('empty');
      chip.classList.remove('loaded');
      txt.textContent = 'Insert disc image (.bin)';
      chip.title = 'Pick your Legend of Legaia .bin once - it is cached locally and every page reads from it. Nothing is uploaded.';
      chip.setAttribute('aria-label', 'No disc loaded. Click to insert a disc image (.bin).');
    }
    /* The home page's prominent disc slot mirrors the same state. */
    const slot = document.getElementById('disc-slot');
    if (slot) {
      slot.classList.toggle('loaded', !!(meta && meta.name));
      const line = slot.querySelector('.slot-line');
      const browse = slot.querySelector('.browse');
      if (meta && meta.name) {
        if (line) line.innerHTML = '<b>' + meta.name + '</b> is in the drive.' +
          '<span class="sub">Cached in this browser - every page below reads from it. Nothing is uploaded.</span>';
        if (browse) browse.textContent = 'Swap disc';
        /* Identity card (js/disc-info.js, home page only): what the cached
           image actually is - serial, region, build, PROT layout. */
        if (window.DiscInfo) window.DiscInfo.intoCached(slot);
      }
    }
  }

  peekDiscMeta().then(render).catch(function () { render(null); });

  const input = document.createElement('input');
  input.type = 'file';
  input.accept = '.bin,.img,.iso,.dat';
  input.style.display = 'none';
  document.body.appendChild(input);
  chip.addEventListener('click', function () { input.click(); });
  const slotBrowse = document.querySelector('#disc-slot .browse');
  if (slotBrowse) slotBrowse.addEventListener('click', function () { input.click(); });
  input.addEventListener('change', function () {
    const f = input.files && input.files[0];
    if (!f) return;
    txt.textContent = 'Caching…';
    storeDisc(f).then(function () {
      /* Reload so the page's own RomCache.attach auto-load path picks it up. */
      window.location.reload();
    }).catch(function (e) {
      console.warn('disc chip: cache failed -', e);
      render(null);
    });
  });
}

/* ---------- Icon rail ---------- */
function buildRail(active, zone, depth) {
  const rail = document.createElement('nav');
  rail.className = 'rail';
  rail.setAttribute('aria-label', 'Site areas');
  for (const item of RAIL) {
    const a = document.createElement('a');
    a.className = 'rail-item';
    a.href = resolveHref(item.href, depth);
    if (item.match(active, zone)) a.classList.add('active');
    a.innerHTML = railSvg(item.icon) + '<span>' + item.label + '</span>';
    rail.appendChild(a);
  }
  return rail;
}

/* The same destinations as the rail, rendered as a strip at the top of the
   drawer. Built on every page; CSS shows it only where the rail is hidden. */
function buildZoneShortcuts(active, zone, depth) {
  const nav = document.createElement('nav');
  nav.className = 'sidebar-zones';
  nav.setAttribute('aria-label', 'Site areas');
  for (const item of RAIL) {
    const a = document.createElement('a');
    a.className = 'sidebar-zone';
    a.href = resolveHref(item.href, depth);
    if (item.match(active, zone)) a.classList.add('active');
    a.innerHTML = railSvg(item.icon) + '<span>' + item.label + '</span>';
    nav.appendChild(a);
  }
  return nav;
}

/* ---------- Search overlay ---------- */
function buildSearchOverlay(depth) {
  const overlay = document.createElement('div');
  overlay.className = 'search-overlay';
  overlay.id = 'search-overlay';
  overlay.innerHTML = `
    <div class="search-box" role="dialog" aria-label="Search">
      <div class="search-input-wrap">
        <span class="icon">⌕</span>
        <input type="text" class="search-input" id="search-input" placeholder="Search pages, sections, formats, functions..." aria-label="Search query">
        <button type="button" class="search-close" aria-label="Close">esc</button>
      </div>
      <ul class="search-results" id="search-results" role="listbox"></ul>
      <div class="search-foot">
        <span><kbd>↑</kbd><kbd>↓</kbd> navigate</span>
        <span><kbd>↵</kbd> open</span>
        <span><kbd>esc</kbd> close</span>
      </div>
    </div>
  `;
  overlay.dataset.depth = String(depth);
  return overlay;
}

/* ---------- Sidebar scroll persistence ---------- */
/* The site is multi-page: every nav click loads a fresh document and rebuilds
   the sidebar, which would otherwise snap back to the top. We stash the
   sidebar's scrollTop in sessionStorage (per-tab, cleared on tab close) and
   restore it before the first paint so the nav stays where the reader left it
   while only the middle/right panels change. */
const SIDEBAR_SCROLL_KEY = 'sidebar-scroll';

function restoreSidebarScroll(sidebar) {
  try {
    const saved = parseInt(sessionStorage.getItem(SIDEBAR_SCROLL_KEY) || '', 10);
    if (!isNaN(saved)) sidebar.scrollTop = saved;
  } catch (e) {}

  let raf = 0;
  sidebar.addEventListener('scroll', () => {
    if (raf) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      try {
        sessionStorage.setItem(SIDEBAR_SCROLL_KEY, String(sidebar.scrollTop));
      } catch (e) {}
    });
  }, { passive: true });
}

/* The current page's row, scrolled into the sidebar's view when the restored
   position (or the top, on a first visit) leaves it off screen. On a phone the
   drawer opens with the two always-expanded sections first, so without this
   the reader's own page sat below the fold of the drawer that is meant to show
   where they are. */
function revealActiveRow(sidebar) {
  /* Scoped to the page list: the zone strip above it marks its own entry
     active too, and that one is always at the top. */
  const row = sidebar.querySelector('.sidebar-nav a.active');
  if (!row) return;
  const box = sidebar.getBoundingClientRect();
  const r = row.getBoundingClientRect();
  if (r.top < box.top || r.bottom > box.bottom) {
    sidebar.scrollTop += (r.top - box.top) - box.height / 3;
  }
}

/* ---------- Main ---------- */
function injectLayout(opts) {
  const { active } = opts || {};
  const depth = depthFromKey(active);
  const zone = zoneForKey(active);

  document.body.classList.add(zone === 'docs' ? 'zone-docs' : 'zone-explore');
  if (active === 'home') document.body.classList.add('page-home');

  /* Shell chrome on every page */
  const topbar = buildTopbar(active, zone, depth);
  const rail = buildRail(active, zone, depth);
  const overlay = buildSearchOverlay(depth);
  document.body.insertBefore(rail, document.body.firstChild);
  document.body.insertBefore(topbar, document.body.firstChild);
  document.body.appendChild(overlay);

  const app = document.querySelector('.app');

  /* Both zones get a left sidebar: the docs tree, or the grouped explore
     page list - so every page is reachable from the left nav. */
  const sidebar = buildSidebar(active, depth, zone);
  const toggle = buildMobileToggle();
  const scrim = document.createElement('div');
  scrim.className = 'sidebar-overlay';
  scrim.id = 'sidebar-scrim';

  function setDrawer(open) {
    sidebar.classList.toggle('open', open);
    toggle.setAttribute('aria-expanded', String(open));
    scrim.classList.toggle('show', open);
    /* Lock the page behind the drawer so a swipe scrolls the nav, not the
       article underneath it. */
    document.body.classList.toggle('nav-open', open);
    if (open) revealActiveRow(sidebar);
  }

  toggle.addEventListener('click', () => setDrawer(!sidebar.classList.contains('open')));
  scrim.addEventListener('click', () => setDrawer(false));
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && sidebar.classList.contains('open')) setDrawer(false);
  });
  /* Growing past the drawer breakpoint brings the rail back; drop the lock so
     the page is not left unscrollable. The breakpoint is read off the toggle's
     own computed style rather than restated here - one width literal, and it
     lives in the stylesheet. */
  window.addEventListener('resize', () => {
    if (!sidebar.classList.contains('open')) return;
    if (getComputedStyle(toggle).display === 'none') setDrawer(false);
  });

  if (app) app.insertBefore(sidebar, app.firstChild);
  else document.body.insertBefore(sidebar, document.body.firstChild);
  topbar.insertBefore(toggle, topbar.firstChild);
  topbar.insertBefore(buildThemeToggle(), topbar.querySelector('.tb-gh'));
  document.body.appendChild(scrim);
  restoreSidebarScroll(sidebar);
  revealActiveRow(sidebar);

  if (zone === 'docs') syncBreadcrumb(active, depth);
  wrapWideTables();

  /* Order matters: assign IDs first → build TOC (clean text) → add § anchors */
  assignHeadingIds();
  if (zone === 'docs') {
    const toc = buildTocRail();
    if (toc && app) app.appendChild(toc);
    else if (app) app.classList.add('no-toc');
  } else if (app) {
    app.classList.add('no-toc');
  }
  injectHeadingAnchors();

  if (zone === 'docs') {
    const content = document.querySelector('.content');
    if (content) {
      const pn = buildPageNav(active, depth);
      if (pn) content.appendChild(pn);
    }
  }

  wireDiscChip();

  fitTables();
  window.addEventListener('resize', fitTables);
  window.addEventListener('load', fitTables);
  if (document.fonts && document.fonts.ready) document.fonts.ready.then(fitTables);

  /* Wire search trigger */
  const openSearch = document.getElementById('open-search');
  if (openSearch) openSearch.addEventListener('click', () => window.openSearch && window.openSearch());

  /* Global keyboard shortcut for search */
  document.addEventListener('keydown', (e) => {
    if (e.target && (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA' || e.target.isContentEditable)) return;
    if (e.key === '/') {
      e.preventDefault();
      window.openSearch && window.openSearch();
    }
  });
}

window.injectLayout = injectLayout;
window.SITE_NAV = NAV;
