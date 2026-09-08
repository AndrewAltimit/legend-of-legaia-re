# VRChat world kit - Legend of Legaia scenes in Unity

Tooling + guide for turning `legaia-engine export-glb` output (a scene's
textured world, its NPCs, its animated props) into a **private** VRChat
world. Companion to the battle diorama transport in
[`../vrc-diorama/`](../vrc-diorama/README.md); full exporter reference:
[`docs/tooling/vrchat-world-export.md`](../../docs/tooling/vrchat-world-export.md).

**Legal first.** The exported `.glb` files contain Sony-derived geometry and
textures decoded from *your* disc. They are for your own local use: never
commit them, never redistribute them, and keep any VRChat upload **private**
(the default for a fresh world). This directory ships only from-scratch
tooling - scripts and a guide - no game data.

```
your disc ──legaia-extract──▶ extracted/ ──legaia-engine export-glb──▶ glb-export/town01/
                                                                          │ town01.glb  (world)
                                                                          │ npcs/*.glb  (animated)
                                                                          │ props/*.glb (windmill…)
                                                                          │ manifest.json
                                          Unity + glTFast + this kit ◀────┘
```

## Files

| File | Role |
|---|---|
| `world-project/Assets/LegaiaWorld/Editor/LegaiaWorldBuilder.cs` | Editor menu `Legaia > Build Scene From Manifest...`: instantiates the world, adds colliders, places NPCs + animated props, builds doorway-teleport triggers, wires proximity doors + the shoreline morph clip + the BGM loop, drops a spawn marker; also the **Equipment props** rack (the `--items` export placed as grabbable pickups near the spawn) and the camp props (below). |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaCampProps.cs` | The **Camp props** pass: a carry-able settings panel (world-space buttons - local music mute, synced day/night jumps; grab collider confined to a bottom handle so it can't shadow the UI) plus two carry-able torches and two campfires near spawn, all primitives + generated materials, in a top-level container outside the mirrored root (mirrored UI text would render backwards). |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaAudioGen.cs` | Synthesized ambience: the long day / night / base / wind-gust beds, the shore-wave, tree-bird, night-wildlife and windmill emitter clips, and the camp fire crackle - seeded Poisson events under multi-octave value-noise envelopes, seam-crossfaded, no disc audio. Also the `VRC_SpatialAudioSource` compliance helper (the SDK deprecates bare AudioSources; 2D beds get the disabled component its Auto Fix adds, spatial sources a configured one). |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaRealism.cs` | The builder's "Realism enhancements" foldout: lit materials + generated normals + sun, day/night wiring + night doorway lamps, sky + fog, procedural grass, interior room shells, texture smoothing, synthesized ambience, wander wiring. Every pass defaults on; untick for the faithful look. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaSceneSettings.cs` | Per-scene refinements from `Settings/<scene>.settings.json` (see "Per-scene settings" below): delete named objects after the build, keep listed NPCs static (idle clip, no wandering), drop listed NPCs entirely, override the spawn point in Unity world space, and point the VRC Scene Descriptor's `Spawns[0]` at LegaiaSpawn automatically. |
| `world-project/Assets/LegaiaWorld/Settings/town01.settings.json` | The town01 refinements (kit-authored tuning data, no game content). |
| `world-project/Assets/LegaiaWorld/Editor/MiniJson.cs` | Dependency-free JSON reader for `manifest.json` (so the builder compiles in any project). |
| `world-project/Assets/LegaiaWorld/Shaders/LegaiaLitVertexColor.shader` | Lit cutout stand-in for the exports' unlit materials: `COLOR_0` keeps modulating the texture, and lighting is the sign-independent two-sided Lambert `\|N.L\|` (the only stable answer over the mixed PSX winding - the header keeps the failed-flip history). |
| `world-project/Assets/LegaiaWorld/Shaders/LegaiaLitVertexColorTransparent.shader` | The BLEND (water / light pool) sibling of the lit shader - alpha-blended, depth-write off. |
| `world-project/Assets/LegaiaWorld/Shaders/LegaiaVertexColorAdditive.shader` | Unlit additive for the exporter's `legaia_semi_abr1/2/3` materials - PSX additive prims (window light shafts, glows) read grey under plain alpha blend. Two passes approximate the PSX's display-space add in Unity's Linear pipeline: a background multiplier that keeps the scene readable through the prim, plus an additive floor exact over black (the shader header derives the fit). |
| `world-project/Assets/LegaiaWorld/Shaders/LegaiaGrassWind.shader` | Vertex-coloured wind sway for the procedural grass blades (sway weight in vertex alpha, world-position phase). |
| `world-project/Assets/LegaiaWorld/Shaders/LegaiaInteriorShell.shader` | Unlit black, front faces only: the interior-room dome, wound inward so it reads as black space from inside and is invisible (backface-culled) from outside. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaDoorway.cs` | UdonSharp doorway teleport: walking into the trigger repositions the local player at the landing marker with the authored arrival facing - the retail intra-scene door mechanism. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaDoor.cs` | UdonSharp proximity door: first approach by a player plays the door's swing clip once and holds it open; `NpcOpen` / `NpcClose` (the `OnNpcArrive` / `OnNpcLeave` station events) let a villager open a cupboard while it stands there and close it again on leaving, refcounted so two villagers at one cupboard close it once. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaNpcWander.cs` | The NPC locomotion controller (plus the keep-out zones the stroll never aims into, and `Nod` - a short LateUpdate pitch of the torso over whatever the Animator posed, for a villager taking its turn): the autonomous small-radius stroll (collision-aware, floor-following, forward-only) plus the command API the living-town brain drives (`GoTo` / `GoToWithin` / `SlideTo` / `FaceToward` / `Arrived` / `Blocked` / `Stop` / `Teleport` / `Facing`). A commanded walk follows a route over the baked navmesh corner by corner, hopping the bake's ledge links where no walk connects; the probe-ray steering fan only detours around another villager while on a route, and two watchdogs (no displacement, no progress toward the current corner) end a walk that cannot get through. Facing is measured off the rendered torso, never derived. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaNavMeshLoader.cs` | Registers the baked villager navmesh (`NavMeshData` asset) with the runtime `NavMesh` at load - the one-liner that stands in for the AI Navigation package's surface component. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaNpcStation.cs` | The station contract of the living town: a place an NPC can go and do something (`kind` 0 use-prop / 1 fishing / 2 seat / 3 chat / 4 viewpoint / 5 carry endpoint / 6 visit), a stand point + facing, an optional handler that receives `OnNpcArrive` / `OnNpcLeave`, visitor-side fields (`arriveIcon`, `glance`, `carryFlow`) and `Claim` / `Release` bookkeeping. Any pass can plant one; the director finds them all. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaTownDirector.cs` | The town's scheduler (one per scene; also turns a group to face the current speaker, has a listener answer the topic, and runs the night host's shift): builds multi-stop errand itineraries, matchmakes conversations onto chat rings *and* on the spot where villagers meet, runs the bubble turn-taking, pairs passing villagers for a greeting or an errand walked side by side, and sends the town indoors at night (and back out at dawn). Asks the navmesh before sending anyone anywhere. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaNpcBrain.cs` | One villager's state machine (stroll / go to station / at station / go chat / chat / go home / door opening / onto the threshold / come out / stand out for the night, plus the daytime states: errand leg, errand stop, greeting, walking beside a companion, ad-hoc meeting, and the night host's shift), the conversation attention / reaction vocabulary and the glance at a player who walks up, executing the director's decisions through the locomotion controller and the station contract; a build-time personality seed keeps every client's choices aligned. The door trip is a reusable action (`DoorTrip` / `LeaveThrough`) any layer can aim at any doorway pair. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaSpeechBubble.cs` | The billboarded pictogram bubble over a talking villager: six reactions ("...", "!", "?", heart, music note, laugh), the wave and the work drops, and six conversation topics (fish, house, sun, sleep, food, storm). Pictures only, never dialog text; icons swap by enabling one of fourteen child quads, never by material writes. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaNpcCarry.cs` | What a villager is holding: the per-villager item rig at a measured hand point, one child per item, `Show` / `Hide` by `SetActive`, a work sway, and a hard drop deadline so an errand cut short never leaves a bucket welded to somebody's arm. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaNpcHandItem.cs` | The handler behind a kind-5 carry station: hands the arriving villager an item, or takes back the one it is carrying. `keepOnLeave` is what makes a fetch read as one errand instead of two visits. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaVisitSpot.cs` | The handler behind a kind-6 visit station: the fixed resident being called on answers with its own speech bubble, on a beat after the caller's hello, and keeps answering while the caller stands there. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaWeather.cs` | Clock-synced weather schedule (clear / overcast / windy spells): ambient + fog greying multiplied over the day/night cycle, grass gust strength, and the `windLevel` feed into the ambience mixer. `JumpToClear` is the settings panel's button. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaFishingSpot.cs` | Fishing-station handler: while a villager stands on a shoreline station it holds a generated rod over the water, a line to a bobbing float with ripples, and an occasional catch; steps aside when a player stands on the spot. Players fish here too (Interact on the stake): bite, window, catch, coins into the purse. See "Coins" below. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaCardTableHost.cs` | Card-table seat handler: the four stools' single NPC-station handler - availability, the seated pose and facing (toward the felt), and the arrival gate that keeps a villager off a stool it has not walked to. A seated player is an invitation, not a shoo; the synced *NPCs: sit / shoo* button is the override. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaCardGame.cs` | The card table's dealer: community-card hold'em by day, Cara's five-card night game after dusk, and blackjack, with the 52 real card pickups, villagers summoned to the free stools and held through a hand, AI opponents, betting from the coin purse, a seat panel with a speech line per seat. The object's owner is the master; presses travel as network events. See "Common prefabs" below. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaTableTalk.cs` | What the villagers say at the card table: new lines written for the world (the Mist, the wall, the Genesis Tree, hunters), the flop / turn / river beats and the bad beats, a stage-direction voice for Vahn (who never speaks in retail), a wolf-raised voice for Noa and a hunter's voice for Cara (whose poker night it is), plus what the villagers say ABOUT the three when they sit down. Composed by the master, synced per seat. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaWallet.cs` | The per-player coin purse: the local player's balance persisted through VRChat PlayerData (`legaia.coins`), read for any player through `CoinsOf`, written only by its own client. Every minigame pays into and out of it. See "Coins" below. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaNpcHitbox.cs` | The trigger capsule on each villager: a rack weapon held by the local player and swung fast enough strikes the villager down through the brain's `Slay`, which broadcasts the fall and the coin drop to every client. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaCoinDrops.cs` | The pooled coin drops under `living_town/coins`: the brain hands it a spot and a value on every client, it floor-snaps a free coin there and expires it after 90 s. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaCoinDrop.cs` | One coin on the ground: spins, bobs, Interact takes it - one server-ordered network event, so only the first taker is paid and every client agrees which. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaLivingTown.cs` | The **Living town** pass: bakes the villager navmesh, builds the keep-out zones every stand point is filtered against, wires the scene settings' night host, builds the director, per-villager brains + bubbles + carried-item rigs, use-prop stations in front of every one-shot prop (cupboards, drawer, shop door), chat rings indoors and out, path / shoreline / doorstep / visit stand spots, carry endpoints and the seeded home assignment from the manifest's doorway pairs - each home with its doorway tile, a stand spot in front of it and the door prop to swing. Idempotent. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaNavMesh.cs` | The navmesh bake: collects every non-trigger collider under the built root and the kit's prefab containers (NPC capsules and rigidbodies excluded), bakes a villager-sized `NavMeshData` with Unity's runtime builder, saves it under `LegaiaGenerated/<scene>/livingtown/` and wires the loader. Also finds the **ledge links** that let a villager hop between islands the bake leaves unconnected, and the editor-side route check (walks and hops) the batch test uses. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaBubbleArt.cs` | Procedural speech-bubble art: the fourteen pictogram textures drawn from scratch, one material each, and the bubble quad. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaCarryArt.cs` | The things villagers carry - bucket, broom, firewood bundle, basket - built from Unity primitives with generated flat materials, sized as fractions of each villager's own measured height. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaWeatherBuilder.cs` | The **Weather** pass: builds `<root>/weather` and wires `LegaiaWeather` to the day/night cycle, the grass material, the ambience mixer and the settings panel. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaLivingProps.cs` | The **Living props** pass: finds standable shoreline points from the world mesh alone (water sheet vs land cells, floor ray, clear standing capsule) and plants the fishing stations in `<root>/living_props`. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaDayNight.cs` | Optional UdonSharp day/night cycle: sweeps the realism sun on a fixed cycle, synced across players via server time; night dims the trilight ambient + fog to a moonlit fraction, enables the night-lamp container, and crossfades the day/night ambience beds. `JumpToDay`/`JumpToNight` apply a synced offset (the settings panel's buttons). |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaAmbienceMixer.cs` | Owns every ambient volume: crossfades the day and night beds on the cycle `LegaiaDayNight` publishes, fades the day-only and night-only spatial emitter groups, and takes `windLevel` from the weather layer to bring the gust bed in. See "Ambient audio". |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaWorldMenu.cs` | The settings panel's behaviour: `ToggleMusic` (mutes the BGM locally - a personal preference), `SetDay`/`SetNight` (jump the shared cycle for everyone). |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaTorch.cs` | Torch/campfire pickup: hold + Use toggles the flame container (fire + smoke particles, a Perlin-flickered point light - no glow orb) and a spatial crackle loop; `lit` is synced so a fire someone lights burns for everyone. Spawn-kinematic like the rack pickups. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaFlicker.cs` | Firelight flicker for the always-burning night torches: no sync, no interaction - just the two-octave Perlin intensity wobble on the flame's point light. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaPickupProp.cs` | UdonSharp equipment-rack pickup: the prop spawns kinematic (frozen on the rack) and only becomes a free physics object the first time a player drops it - so a rack of dozens of bodies can't tunnel through the thin ground during world-load hitches. Weapon rows carry `weapon` and measure their own swing speed while held. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaCommonPrefabs.cs` | The **Common prefabs** foldout: builds the mirror, the TV and the card table from primitives + generated textures + the SDK's own components, spawns the SDK's sample pen system, and drops any prefab assets you list (QvPen, ProTV, a community deck...) near spawn. See "Common prefabs" below. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaSettingsSnapshot.cs` | Menu `Legaia > Snapshot placements to scene settings`: writes the hand-placed Inspector transforms of the common prefabs, the camp settings panel and LegaiaSpawn into `Settings/<scene>.settings.json` (`prefab_transforms` + `spawn_position`), preserving the file's other keys. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaSoak.cs` | The PLAY-MODE soak (`LegaiaBatchChecks.Soak`): enters play mode with ClientSim, pins the day/night clock to night or day, samples every brain at 2 Hz into a per-villager timeline, and asserts the night routine ran (in through the door, door swung, out again at dawn) or reports a day's station visits and conversations. The only check here that actually runs a villager. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaBatchChecks.cs` | Headless self-tests (`Unity -batchmode -executeMethod LegaiaWorld.LegaiaBatchChecks.CommonPrefabs` builds the common prefabs against the scene's built root and asserts every SDK component and every UdonSharp field wiring reached the backing behaviour; `.LivingTown`, `.Weather`, `.Ambience` likewise; `.RigPose` measures every imported NPC rig and checks the sitting pose's hip turn against the rig's walk clip). Never saves the scene. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaEventButton.cs` | A collider button: Interact sends one named event into a target behaviour. Every common-prefab button uses it (no world-space UI, so no pickup-collider-steals-the-click trap). |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaMirror.cs` | Mirror controller: Off / full / players-only surfaces, local choice, auto-off when the player walks away. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaVideoTv.cs` | Synced video player over the SDK's AVPro (PC) + Unity (Android) players: owner-synced URL + playhead origin, late-joiner seek, 5-second load rate limit honoured, error retry. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaSeat.cs` | Interact sits the local player in this object's VRC station (the SDK chair wire); mirrors who sits there for the card game. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaCard.cs` | One playing card: pickup, hold + Use flips it (synced), spawn-kinematic until first drop, re-parked (dealt, revealed) by the deck and the game. The face flip and the dealt-card slide animate the `visual` child only - the root is always at the pose the caller passed, on that frame. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaCardDeck.cs` | Deck controller: Shuffle (seeded Fisher-Yates) / Gather restack every card face-down at the anchor by taking ownership of each card. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaSlotMachineBuilder.cs` | Editor menu `Legaia > Build Slot Machine...`: assembles the playable casino slot minigame directly onto a cabinet mesh's screen face from the `asset slot-art` export - retail reel drums, glass furniture, dot-matrix marquee, HUD, buttons. See "The casino slot machine" below. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaSlotMachine.cs` | UdonSharp port of the engine's slot-machine rules kernel (`engine-core::slot_machine`): the retail LCG + reel strips, feature rolls, stop plans, five-payline evaluation, bonus rounds, and the retail per-frame reel-drum face derivation. Owner-synced with synced RNG streams; reels animate locally on every client from the same deterministic strips. Spins and payouts move the seated player's purse. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaCardGameBuilder.cs` | Builds the card table's `game` child, the per-stool hand anchors, the five community-card spots across the felt centre, and the seat panel (world-space canvas on a post beside the table). |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaPosters.cs` | Hangs the scene's `posters` on the world's own walls: a hand placement first, else a 24-ray wall search from the object each poster names. Owns the `LegaiaWorld.LegaiaPosters.Check` headless gate. See "Wall posters". |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaPortraits.cs` | Villager portraits for the seat panel: one orthographic head render per villager from its measured visible front, saved under `LegaiaGenerated/<scene>/portraits/`; a drawn silhouette when there is no graphics device. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaBounty.cs` | The bounty layer of the living town: the weapon hitbox on every villager, the coin-drop pool, the brain links; per-scene `bounty` / `respawn_seconds` / `coin_drop` keys. |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaCardGameChecks.cs` | Headless `LegaiaCardGameChecks.Run` (wiring, panel facing, portraits, the poker evaluator and blackjack settlement against fixed cases) and `.Soak` (play mode: villagers play hands among themselves, pot conservation asserted). |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaEconomyChecks.cs` | Headless `LegaiaEconomyChecks.Run` (purse, slot-machine link, fishing stakes, hitboxes, coin pool, weapon flags), `.Soak` (play mode: strike a villager, see the coin, see it respawn) and `.SlotRules` (the rules fixture replayed against the class). |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaSlotButton.cs` | One cabinet face button: forwards Interact to the machine with its reel index (idle = spin, reels running = stop reel N, payout = collect). |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaSlotTools.cs` | Menus `Legaia > Verify Slot Rules` (replays the committed rules-parity fixture against the actual UdonSharp class, in the editor, no play mode) and `Legaia > Slot Screen Snapshot` (films the on-cabinet screen composition head-on to a PNG for visual comparison against the site's minigames page). |
| `world-project/Assets/LegaiaWorld/Editor/slot-verify.json` | The rules-parity fixture: scripted spin/stop traces with every outcome field, generated by the repo's `engine-core` test `slot_verify_fixture.rs`. Pure arithmetic from committed seeds + a synthetic payout table - no disc data. |
| `world-project/Assets/LegaiaWorld/Shaders/SlotReelFace.shader` | Reel-face cutout + retail's depth-cued shade per pixel (`clamp(0xB4 - (z+0x200)*0x21C>>9)`, blend `texel*shade/128`): bright at the payline, black past ~48 degrees - the fade that caps the reel window. `Cull Off` (the composition is x-mirrored). |
| `world-project/Assets/LegaiaWorld/Shaders/SlotCutout.shader` | Unlit cutout for everything else on the slot screen: `Cull Off` for the mirrored composition (the legacy back-culled cutout would render nothing) plus a `_Color` tint for the black reel backdrop. |
| `sync-to-project.ps1` | Copies the kit's `Udon/` + `Editor/` + `Shaders/` + `Settings/` into a Unity project, refusing when a project-side file is newer (an in-Unity edit that must be ported back first). The kit is the source of truth; never edit the project copies. |
| `headless-check.ps1` | Runs one of the kit's headless checks against a **project copy** (`-Project C:\lgw-qa -Method LegaiaWorld.LegaiaBatchChecks.CommonPrefabs [-Sync] [-PlayMode] [-Extra @(...)]`): optional kit sync + reimport touch, the editor launched detached with the log inside the copy, the orphaned licensing client killed, `[UdonSharp]` / `error CS` lines surfaced first. Its header carries the robocopy recipe and why `Bee` / `ilpp.pid` must be excluded. |
| `fix-glb-uvsets.py` | Repairs a glb whose materials sample UV set 2/3 (Blender bakes against a late UV layer): glTFast only supports sets 0/1 and fails the whole import with `UVMulti` errors, while web viewers show the file fine. Losslessly rewires each primitive's sampled set onto `TEXCOORD_0`; refuses when a material mixes sets. |

## Step 1 - export a scene

```bash
cargo build --release -p legaia-engine-shell
./target/release/legaia-engine export-glb --scene town01 --out glb-export
# or every scene that assembles:
./target/release/legaia-engine export-glb --all-scenes --out glb-export
# every equipment item (grabbable weapons and the rest):
./target/release/legaia-engine export-glb --items --out glb-export
# Vahn, Noa and Gala as they walk the towns (field forms + idle/walk):
./target/release/legaia-engine export-glb --party --out glb-export
```

Reads `extracted/` by default (`--disc "Legend of Legaia (USA).bin"` works
too). Per scene you get the world `.glb` (with the shoreline's baked
blendshape clip where the scene has one), `npcs/`, `props/`, `music/`
(the entry BGM as a seamless-loop WAV) and `manifest.json` (which now also
carries the doorway-teleport and door-tag data the builder wires below).
Default `--scale` is 1/128: one 128-unit walk tile becomes 1 m. (The
earlier 1/64 default read oversized in-headset - retail's field
proportions are generous, so "2 m per tile" made buildings loom over a
real-scale player.) `glb-export/` is gitignored -
it is Sony-derived output.

## Step 2 - a VRChat worlds project

1. Install the [VRChat Creator Companion](https://vcc.docs.vrchat.com/)
   (Windows; on Linux the community `vrc-get`/ALCOM tools do the same job)
   and create a **Worlds** project (Unity 2022.3).
2. Add **glTFast** for `.glb` import: `Window > Package Manager >
   + > Add package by name...` → `com.unity.cloud.gltfast` (a Unity-registry
   package - it never appears in the Creator Companion, which only manages
   VRChat's own packages; likewise the VRChat packages live in
   `Packages/vpm-manifest.json`, so don't be alarmed when
   `Packages/manifest.json` shows no `com.vrchat.*` entries). On the
   VRChat-mandated 2022.3.22f1 editor, pin version `6.14.1`: 6.15+ needs a
   newer 2022.3 patch (2022.3.67f2) and 6.19+ needs Unity 6, so the
   Package Manager refuses them. Equivalent to the UI route, one line in
   `Packages/manifest.json` under `"dependencies"` does the same job:

   ```json
   "com.unity.cloud.gltfast": "6.14.1",
   ```
3. Sync `world-project/Assets/LegaiaWorld/` into the project's `Assets/`:
   `./sync-to-project.ps1 -Project "<unity project>"`. Re-run it after any
   kit change; it refuses to clobber files edited inside Unity (port those
   back into the kit first - the repo copy is the source of truth).

The sibling diorama kit's
[`world-project/README.md`](../vrc-diorama/world-project/README.md) walks
the VCC setup in more detail if this is your first worlds project.

## Step 3 - import + build the scene

1. Copy an exported scene folder into the project, e.g.
   `Assets/LegaiaImports/town01/` (keep `npcs/`, `props/`,
   `manifest.json` beside the world glb). Let Unity import - glTFast picks
   up every `.glb`, and the baked NEAREST samplers keep the PSX
   point-sampled look without any material fixup.
2. Menu **Legaia > Build Scene From Manifest...**, browse to the copied
   `manifest.json`, and **Build scene**. Two default-on options worth
   knowing: **Match explorer orientation** mirrors the finished root on X
   so the scene reads the way the site's field-scene viewer presents it
   (the raw import genuinely is mirrored - see the troubleshooting entry
   for the landmark test that settled it), and **Merged + welded**
   builds one welded collision mesh for the whole world instead of a
   collider per mesh (closes the hairline seams between tile colliders a
   player capsule can slip through, and client builds stop needing
   Read/Write enabled on the glb). You get a `Legaia_town01` root:
   - `world` - the full map with its collider (ground included, so the
     retail walk surface is what you stand on). Semi-transparent submeshes
     are left OUT of collision - they represent light (window shafts,
     glow cones), and cooked solid a slanted shaft even acts as a
     walkable ramp - except ones shaped like a water sheet (large, flat,
     horizontal), so the sea keeps its floor. When the scene carries the
     baked shoreline morph clip (**Animate world morphs**) the sea edge
     washes in and out on a loop, and the exported BGM loops from an
     AudioSource on the root (**Scene music**);
   - `npcs/` - every catalogued villager at their MAN spawn tile, playing
     their retail spawn clip on a loop, capsule-collided;
   - `props/` - the animated placements (Rim Elm's windmills, doors, gate
     leaves) playing their bind clips. The world glb keeps a frame-0
     static twin under each one; the builder disables it by default
     (**Hide static prop twins**) so the pair doesn't z-fight. Instances
     the manifest tags as doors - plus every prop whose clip the manifest
     marks one-shot (`cyclic: false`: interior doors, cupboard doors,
     drawers - retail parks these held at spawn and only a script plays
     them, so looped they'd re-play their swing forever) - open **on
     approach** and stay open (**Doors open on approach**, via the
     `LegaiaDoor` behaviour). Only the props retail itself leaves
     free-running loop - the `cyclic` flag is the bind record's own
     spawn-pass verdict, which is what keeps the village windmill
     spinning (its clip ends 179° displaced, so any keyframe-shape test
     misreads it; the four-blade symmetry makes the loop seamless);
   - `teleports/` - one trigger volume per manifest doorway teleport
     (**Doorway teleports**): walk into a house door and you land in its
     interior with the authored facing, exactly the retail intra-scene
     door mechanism (`LegaiaDoorway`). Trigger boxes get an absolute
     player-sized floor (2.2 m across, player height) - the authored
     extents scale with the world, but the player doesn't, and a retail
     trigger hugging the door plane of a recessed entrance (Mei's house,
     the cave) is hard to clip at any smaller size; the wall absorbs the
     backward growth.
     A loop guard covers the retail landings that sit inside (or a
     capsule-width from) the paired door's trigger (town01's hilltop
     house lands inside its own exit band): before teleporting, the
     firing doorway suppresses every sibling doorway near the landing
     for a few seconds, so a landing can never chain-fire a ping-pong
     loop - which reads as "the door does nothing" when the hops cancel
     out. Stepping off a trigger re-arms it immediately (retail's own
     walk-away re-arm), so walking away and straight back always fires;
     Scene-exit portals also connect
     automatically when the target scene's built root (`Legaia_map01`,
     say) already exists in the same Unity scene;
   - `LegaiaSpawn` - the manifest's suggested spawn (the placed-object
     median, i.e. the village centre).

   A separate top-level `Legaia_camp_props` container (**Camp props**,
   default on) holds the settings panel and the torches/campfires - it
   sits outside the mirrored root on purpose (world-space UI text under
   the X-flip would render mirror-written). The panel is a board with a
   grab handle along its bottom edge: point at a button for the UI
   cursor, grab the handle to carry it. The handle is the panel's ONLY
   collider on purpose - VRChat's pointer takes the closest collider
   hit, so a grab collider covering the buttons turns every press into
   a pickup grab and the UI never receives the click (in desktop the
   click then reads as the held pickup's use/drop, with the button still
   animating - the panel looks alive but is dead). Click the buttons
   while the board stands; a held pickup's own UI is not clickable in
   desktop mode. **Music On/Off** mutes the BGM for you alone,
   **Daytime**/**Nighttime** jump the shared day/night cycle for
   everyone (they do nothing until the realism pass builds the cycle),
   and **Clear sky** jumps the shared weather schedule the same way
   (inert until the weather pass is built).
   The two torches and two campfires
   are pickups too - hold one and press **Use** (left click / trigger)
   to light or snuff it: fire particles, a faint rising smoke plume, a
   warm point light with a Perlin fire flicker (no glow-orb mesh), and a
   spatial synthesized crackle loop; the lit state is synced, so a
   campfire someone lights burns for the whole instance. Every
   AudioSource the kit creates also carries the `VRC_SpatialAudioSource`
   component the SDK now expects (disabled on the flat 2D music/ambience
   beds, exactly what the SDK's Auto Fix does - the "2D audio source
   with no VRC Spatial Audio component" warning is gone).
3. Add the VRChat scene descriptor (`VRCWorld` prefab from the SDK). The
   builder points its **Spawns** element 0 at `LegaiaSpawn` automatically
   when the descriptor already exists at build time (set
   `"set_descriptor_spawn": false` in the scene's settings file to keep
   your own); if you added `VRCWorld` after building, either rebuild or
   drag `Legaia_town01/LegaiaSpawn` into element 0 by hand (replacing the
   prefab's own `Spawn` child - moving the marker does nothing until the
   list references it; with an *empty* list the descriptor's own
   transform is the spawn). Players face the spawn transform's +Z, so
   rotate the marker toward the view you want. Then `VRChat SDK > Show Control
   Panel > Build & Test`. Publish with **Upload** when it feels right - a
   fresh world is private until you explicitly publish it to Community
   Labs; leave it private.
4. Sanity checks: the sample scene's `GridFloor` plane sits near the
   origin - delete it (or keep it far below as a fall catch) so it can't
   mask the real walk surface; and after a build, `world` should carry
   one MeshCollider referencing
   `Assets/LegaiaGenerated/<scene>/world_collider.asset`.

## Per-scene settings

The generic build is right almost everywhere, but each scene earns a few
hand-tuned corrections - and they must survive re-exports and rebuilds,
so they live in a small JSON file shipped with the kit (never in the
exported folder, which is regenerated):

```
Assets/LegaiaWorld/Settings/<scene>.settings.json
```

All keys optional (`town01.settings.json` is the worked example):

```json
{
  "scene": "town01",
  "delete_objects": ["room_6_shell", "room_6_light", "prop_53_anim8/object_1"],
  "static_npcs": [26, 27, 45, 46, 29, 12],
  "remove_npcs": [28, 10, 11],
  "freeze_npcs": [47],
  "spawn_position": [-24.85, 1.75, 12.22],
  "set_descriptor_spawn": true
}
```

- **`delete_objects`** - exact GameObject names removed after the build +
  realism passes (so generated objects like interior shells are already
  there to match). A name with `/` is a path *suffix*: the last segment
  names the object, each earlier segment must match the next parent up -
  so `prop_53_anim8/object_1` hits only that prop's `object_1`, while a
  bare `object_1` would hit every glb's. Searched under the built root
  and the kit's top-level containers. Generated objects are destroyed;
  glb prefab children are disabled instead (Unity forbids deleting
  prefab-instance children). A name that matches nothing logs a warning
  so typos are visible.
- **`static_npcs`** - these NPCs keep their looping idle clip but the
  wander pass never wires them, so they stay put. A number `N` matches
  the exported `npc_<NN>_...` stem exactly; a string matches any part of
  the file name. Re-applying the realism layer also *strips* a wander
  behaviour wired before the rule was added.
- **`remove_npcs`** - not placed at all (same matching rules).
- **`freeze_npcs`** - placed with **no animation clip**, holding the rest
  pose (and never wander-wired). For prop-kind actors - trees, signs -
  whose bundle slot carries a generic locomotion record: looping that
  record visibly walks the prop in place (town01's npc_47 tree paces
  ~10 cm side to side on `record_37`). Takes effect on the next full
  build, like `remove_npcs`.
- **`replace_npcs`** - `{"14": {"scene": "kor3", "model": 127}}` puts
  another scene's model on a placed villager: the key follows the
  matching rules above, the value names an export folder under
  `Assets/LegaiaImports/` and the `model_index` its manifest lists for a
  placement. Position, label and every rule keyed on the number stay with
  the original; mesh, rig and idle clips come from the source.
- **`add_npcs`** - `[{"scene": "bylon", "model": 103, "position": [x, y, z],
  "yaw": 0, "label": "Maya"}]` places a brand-new villager from another
  scene's export at an Inspector position under the built root (the
  `spawn_position` convention). It joins the manifest as `npc_1NN` so
  the other rules can name it, and the living town treats it as any talk
  villager. `label` is the villager's **name** (the card table prints it
  and its table talk keys on it). **The party** comes in the same way:
  `export-glb --party` writes `party/{manifest.json, npcs/}` (Vahn, Noa
  and Gala's field forms with their real idle and walk cycles); copy it
  to `Assets/LegaiaImports/party/` and add `{"scene": "party", "model":
  0}` (Vahn), `1` (Noa) or `2` (Gala) - the export names them, so
  `label` is optional. They walk, chat, go home and sit at the card
  table like anyone else, and the table knows who they are. town01's
  settings place all three: Vahn and Noa by the spawn, Gala beside
  Tetsu's (npc_07's) spot.
- **`living_town.names`** - `{"npc_07": "Tetsu"}` pins a villager's
  display name. Without a pin the living town names everyone itself:
  the speaker prefix of the actor's retail line when it has one ("Val:
  I'm sorry" is Val), "Crow" for the birds, else a short Legaia-shaped
  name from a pool (Bram, Tilo, Rena, ...) chosen by the scene seed.
  Never the retail line itself - the panel used to print "Tetsu: You
  were a child when the" and "I am a dummy." as names.
- **`mesh_npcs`** - `{"mesh_55": {"scene": "balden", "model": 151}}` turns
  a world-glb mesh (a villager the scene baked as scenery) into a live
  one: the mesh is hidden and the source model stands on its footprint.
  The source scene must be exported and copied in first (`legaia-engine
  export-glb --scene balden --out <dir> --no-props`, then `manifest.json`
  + `npcs/` into `Assets/LegaiaImports/balden/`). Scene names are the
  disc's: Biron Monastery is `bylon`.
- **`spawn_position`** - overrides the manifest's suggested spawn:
  **exactly what LegaiaSpawn's Inspector shows** (its local position
  under the built root). Drag the marker where you want it, copy its
  Inspector position here, and the rebuild reproduces it digit for
  digit. (Not world space: the root is X-mirrored, so a world value
  would come back with its X sign flipped in the Inspector.)
- **`prefab_transforms`** - hand placements for the common prefabs and
  the camp settings panel (`menu`), position + optional rotation in
  Inspector numbers; written by the snapshot menu. See "Common prefabs".
- **`slot_machine`** - the casino cabinet: model asset, `asset slot-art`
  folder, placement + scale. The common-prefabs pass places the cabinet
  and builds the minigame on it. See "Common prefabs".
- **`ambience`** - user-supplied ambience loops by role:
  `{"day": "Assets/Audio/day.wav", "night": ..., "base": ...,
  "waves": ..., "wind_gust": ..., "tree_birds": ..., "night_wildlife": ...,
  "windmill": ...}`. Each key optional, each value the asset path of any
  AudioClip in the project. A named role uses that clip instead of the
  synthesized one, which is then never generated. See "Ambient audio".
- **`living_town`** - the village layer's per-scene tuning:
  `home_cap`, `chat_spots`, `seed`, `daytime_indoors_share`, `walk_clip`,
  plus `home_doors` / `exclude_doors` (teleport indices to force or refuse
  as HOUSES - the pass otherwise decides from a door leaf on the tile or a
  single-tile way out, and logs every rejection with its index) and
  `nav_links` (`[[[x,y,z],[x,y,z]], ...]` in manifest coordinates: ledge
  links to add by hand where the bake's own search misses one), and the
  bounty layer's `bounty` / `respawn_seconds` / `coin_drop` (see
  "Coins").
- **`living_town.night_host`** - `{"npc": "npc_14", "station":
  "Legaia_common_prefabs/card_table/stool_0"}` pins one villager to one
  station for the whole night instead of sending it home; town01 seats
  Cara at the card table. See "Somebody keeps the card table at night".
- **`posters`** - a list of `{name, image, near, width}`. `name` names
  the object (`poster_<name>` under `Legaia_common_prefabs`) and its
  generated material; `image` is a project asset path (the image is
  never part of the kit - drop it under your project's `Assets/`);
  `near` is the object the wall search starts from, looked up first as
  a common-prefab child (`card_table`) and then anywhere in the scene;
  `width` is the print's width in metres, its height following the
  image's aspect. A `prefab_transforms["poster_<name>"]` entry overrides
  the search completely. A poster whose image is missing, whose `near`
  object is absent, or that finds no wall inside 12 m logs a warning and
  is skipped - never hung in mid-air. See "Wall posters".
- **`npc_homes`** - pin a villager to a house door by index into the
  homes the pass keeps (`living_town/homes/home_N`). Note the indices
  shift when a teleport is rejected as not-a-house.
- **`set_descriptor_spawn`** - point the VRC Scene Descriptor's
  `Spawns[0]` at `LegaiaSpawn` after the build (default true, also
  without a settings file; a no-op until the `VRCWorld` prefab is in the
  scene).

Deletions, the descriptor assignment and the three model-override rules
also re-run after "Apply enhancements to the already-built root" (an
overridden villager is swapped in place there), so tuning a rule doesn't
force a full rebuild - except `remove_npcs`/`freeze_npcs`/`spawn_position`,
which take effect on the next **Build scene**.

## Making it feel alive

- **Wandering villagers**: add `LegaiaNpcWander` (UdonSharp) to an NPC the
  builder placed; tune radius/speed. Movement is computed per player -
  fine for ambience, use synced variables if everyone must agree.
- **Grabbable weapons**: `export-glb --items` exports **every equipment
  item** (all four characters' weapons, Ra-Seru, armour, headgear,
  footwear) to `glb-export/items/` - per record an item-alone `.glb`
  (grip repaired) and an exact-cut `.glb` with its host limb, both
  animated, plus a manifest of names and cut-honesty tags. Copy the
  `items/` folder into `Assets/`, point the builder's **Equipment props**
  section at its `manifest.json`, and **Place equipment rack near spawn**
  lines them up grounded on the world collider - one row per character,
  each prop scaled from raw PSX units by the scene's export scale times
  the **Size multiplier** (default 0.5: these are battle-mode models the
  field never shows, and at the raw battle-vs-field ratio they read
  comically large in hand),
  wrapped in a convex mesh collider cooked from its baked rest pose (a
  tight hull, not a bounding box; near-flat pieces fall back to a padded
  box), and wired as a `VRC Pickup` + `VRC Object Sync` physics pickup
  (static display without the SDK). Props spawn **frozen** on the rack
  (`LegaiaPickupProp` flips the body physical on first drop), so world
  load never scatters them or drops them through the thin ground mesh.
  **Weapons only** is the
  default filter; untick it to also rack armour, headgear, footwear and
  Ra-Seru. (The site's equipment viewer offers the same files per
  download.)
- **More clips**: every NPC glb carries *all* the scene-bundle clips whose
  bone count matches (`record_N` takes) - retarget the Animator the
  builder generated at any of them.

### Living town

The realism foldout's **Living town** option (default on) layers a small
village routine over the stroll: villagers meet and talk in twos and
threes, walk to things and use them, and go indoors at night. It builds a
`living_town` child under the built root - a `LegaiaTownDirector`, one
`LegaiaNpcBrain` per villager, and the `LegaiaNpcStation` destinations
they visit - plus a `speech_bubble` on each villager.

- **Conversations.** A conversation spot is a *ring* of three stand points
  around one meeting place, so a group of three is an ordinary station
  claim rather than a special case; the director matchmakes two or three
  free villagers onto a free ring, waits for everyone to arrive, and then
  gives turns. The talker gets a bubble over their head, and the bubbles
  speak in **pictograms only**, the way The Sims does - no dialog text is
  ever printed. Each conversation opens on a topic (fish, house, sun,
  sleep, food, storm cloud) and the replies are reactions ("...", "!",
  "?", laugh, music note, heart); the topic comes back now and then, or
  drifts to a new one. Outside conversations the same vocabulary carries
  the rest of the day: a wave when two villagers cross, work drops at an
  errand stop, the house on the walk home, a yawn of Zs from a villager
  with nowhere to go, a fish or a sun at a shore or path viewpoint.
  Three-ways are not left to chance: after two consecutive pairs the next
  conversation *must* be a three, and a forced three-way now **waits**
  for a third free villager instead of settling for a pair - settling
  spent the force on a two-way and re-armed it against the same two
  people, which is why groups of three never appeared in a village of
  four.
- **Using props.** Every one-shot, non-door prop - Rim Elm's four
  cupboards, the drawer, the shop's upstairs door - gets a stand point in
  front of it whose station handler is the prop's own `LegaiaDoor`. A
  villager who walks up opens it and closes it again on the way out
  (`NpcOpen` / `NpcClose`, the swing clip played forward then at speed
  -1). A door a *player* opened stays open, as before, and two villagers
  at one cupboard close it once.
- **Night.** Homes come from the manifest's own doorway teleports: a
  trigger outside paired with a landing inside is a front door, and the
  interior-side teleport nearest that landing is the way back out. At
  nightfall (`LegaiaDayNight.isNight`) villagers go home the way a player
  does: the navmesh route takes them to a stand spot a step out from
  the doorway tile (probe rays pick the open side of the tile - the hut
  wall is on the other), the home's door prop swings open (`NpcOpen` on
  the same `LegaiaDoor` a player's approach fires), they step onto the
  tile - retail's own teleport trigger - and are teleported to the
  landing with its authored facing, and the door shuts behind them
  unless a player had opened it. Inside they potter about the room; at
  dawn they walk to the interior doorway, appear at the village-side
  landing with the door swinging, and go about their day. A share of
  them (**default 18%**) stay in during the day as well. Home assignment
  is a **seeded shuffle at build time**, capped per door
  (**Villagers per house**, default 4), so every client agrees on who
  lives where without a single synced variable. Two kinds of villager
  get no door: one retail placed *inside* a house (the detached interior
  rooms - it starts indoors, on its own island, and keeps to its room),
  and one whose spawn has no walkable route to any door at all (town01's
  villager on the beach below the village bank - the bank is a 1.2 m
  wall in the collider, so that villager stays out at night rather than
  clip up it; the pass logs each such case).
- **Stations are shared ground.** The director picks free stations
  generically by kind, indoors flag and `IsFree()`, so the shoreline
  fishing spots and card-table seats other passes build against the same
  `LegaiaNpcStation` contract are visited too.

#### What villagers do by day

Retail put every usable prop in town01 *inside* a house, so the outdoor
half of the day is furniture this pass builds itself, and the behaviour
that walks it is an itinerary rather than a single hand-out.

A villager who is free is usually given an **errand**: two to four
stations, picked one at a time from the last one's position so the round
crosses the village instead of starring out of one point. It claims each
stop only as it sets off for it - holding three stations at once would
starve a village of four - stands at each for the station's dwell,
glancing about, and rests when the itinerary runs out. A stop it cannot
reach is dropped and the errand goes on, rather than the villager
standing against a wall until the walk times out.

Some stops put something in its hands. **Carry endpoints** (kind 5,
handler `LegaiaNpcHandItem`) hand over a bucket, a broom, a bundle of
firewood or a basket - primitives generated by `LegaiaCarryArt`, parented
under the villager at a hand point *measured* off its own rig: among the
body's mesh nodes at arm height, the widest of the centreline and then
the lowest of those, which lands on a forearm on every town01 rig family
(the ten-node humanoid, the six-node robed one and the short skirted one
whose arms are different node indices). The item stays in hand between
stops, and `carryFlow` biases the next stop toward one that takes it
back, so water filled at the low ground is carried to a doorstep rather
than walked in a circle. Nothing is held per-frame: the items are
children of the villager and follow it for free.

The new outdoor furniture, all floor- and standing-room-checked like the
existing builders:

- **doorstep stands** a step to the side of each house door, facing the
  doorway - half of them sweep it with a broom, half call at it with a
  wave (they stand *beside* the night routine's own door spot, never on
  it, so a sweeper is not in the way of somebody trying to get home);
- **path and shoreline stands** sampled over the walkable area; a spot
  well below the spawn's floor is shoreline and faces the water;
- **visit spots** in front of every fixed resident - the ones the scene
  settings keep static - measured off the resident's own rendered front.
  The resident does not move, but it gets a speech bubble of its own and
  answers a beat after the caller's hello (`LegaiaVisitSpot`);
- **one guaranteed outdoor conversation ring**, because the NPC-cluster
  ring builder finds only rooms in a village where eleven of the fifteen
  talkers were authored indoors;
- indoors: a ring per room and three stand spots per landing, so the
  villagers retail parked in one room talk to each other as well as
  taking turns at the room's cupboard.

Villagers also react to **each other**, all of it decided on the
director's own 1 Hz tick out of one cached position array - there is no
per-frame proximity work anywhere in the town:

- **passing greetings**: two who walk within arm's reach stop, turn to
  face each other, one waves, and both *resume* the walk they were on
  (the interrupted errand leg is re-issued to the stop it had already
  claimed, not restarted). Rate-limited per pair;
- **walking together**: an errand is sometimes given a companion, who
  keeps to the leader's shoulder by re-aiming twice a second at a point
  offset from where the leader is heading - measured from where the
  leader *was*, since the locomotion controller exposes no facing - and
  talks to it when they arrive;
- **conversations struck up on the spot**: villagers standing near one
  another talk where they are, with no ring and a centre invented from
  their own positions. This is what makes three-way groups happen at all
  in a village that can rarely fill a three-point ring.

**Nobody is sent somewhere they cannot walk.** The editor pass rejects a
stand spot no villager and not the spawn can reach over the bake, and the
director asks `NavMesh.CalculatePath` before handing out a station or a
ring (budgeted per tick, and everything counts as reachable when no bake
is registered). "Somebody" deliberately includes the villagers
themselves: town01's beach is an island of navmesh the village never
reaches, and a spot only the two beach villagers can use is a good spot,
not a broken one.
- **Walking.** Every NPC glb carries the scene bundle's whole clip set,
  and what a rig can do on foot is **measured** at build time
  (`LegaiaLivingTown.MeasureRig`): its nodes are a flat set of rigid
  TMD objects under the glb's scene root, the mirrored pairs among them
  are sorted by height (lowest = feet, highest = upper arms), and every
  clip is sampled for the feet swinging fore and aft in anti-phase - the
  walk signature, which the idle and the gestures fail on amplitude and a
  body sway fails on phase - and for the sole (the foot mesh's lowest
  vertex) travelling toward the face while lifted, which a clip that
  steps backward fails (balden's humanoids score one such `record_60`
  above their walk). town01 has five families: the 11-node
  humanoid (arms, forearms, upper and lower legs) whose `record_36` is a
  genuine stride in place (legs anti-phase at exactly half a period, arms
  contralateral, head and torso amplitude zero, body centroid fixed); the
  7-node body-head-arms rig (Cara's family - a one-piece body, no leg
  pair); the 8-node crows (wings); a 5-node axial chain; and the 2-node
  birds. A rig with a walking clip gets a two-state idle/walk Animator
  (the pinned **Walk cycle clip** / entry `walk_clip` wins when it
  measures as a walk, else the best-scoring clip) and the controller
  crossfades as it starts and stops. The clip's own **stride** (the
  foot's peak-to-peak swing, 0.19 m for `record_36`) and **cadence**
  (steps over length: one cycle of two steps over 2.07 s) go with it,
  and the controller scales the Animator so the feet keep pace with the
  ground - at speed 1 the humanoids slid almost four to one under a
  0.7 m/s walk. `strideScale` (1.3) lets the feet cover a little more
  ground per step than the clip animates, a touch of slide traded for a
  longer, calmer stride: with the 0.5 m/s walk that is about two steps
  a second, where 0.7 m/s at the clip's own stride was a quick shuffle. Rigs with no leg pair used to glide on their looping
  spawn clip; they now get a **procedural gait** (`LegaiaNpcWander.gaitBob`
  / `gaitRoll` / `gaitStride` / `gaitArmSwing`): the glb root bobs once
  per step and rocks a few degrees side to side, and where the rig has an
  arm pair the upper arms swing contralaterally about the rig's lateral
  axis with the forearms riding round the shoulder, blended over the idle
  clip's pose by the gait weight (LateUpdate, after the Animator has
  written the frame) and eased in and out so a stop does not snap. The
  gait is paced by the same 0.19 m stride and the same `strideScale`, so
  every family steps at the same cadence at the same speed. It moves the glb root and the arm nodes
  under the instance, so the walk, the floor ray and the card table's
  seat pose are untouched.

`LegaiaNpcWander` is the locomotion controller under all of this: it keeps
its autonomous stroll and its measured-facing recipe unchanged, and adds a
command API (`GoTo` / `FaceToward` / `Arrived` / `Blocked` / `Stop` /
`Teleport` / `SetIdle`). A commanded walk is **routed over a navmesh**:
the pass bakes one from the world's physics colliders (the merged
double-sided collider, so the X-mirrored root's flipped winding cannot
turn every floor into a ceiling - a bake from render meshes would), sized
for a villager (radius 0.2 m, height 0.8 m, **Step height** and **Max
slope** in the panel), and `LegaiaNavMeshLoader` registers it at load.
`GoTo` asks `NavMesh.CalculatePath` for the route and the walk follows its
corners, so a villager climbs the hill by the path and rounds the huts
instead of aiming straight at its door and wading into the slope (the
floor ray then read the hillside above as "the floor", which is the
clipping-through-terrain walk this replaces). Under the route sits the
old reactive layer - a fan of probe rays either side of a blocked line for
another villager or a moved prop - and a stalled walk re-plans once before
reporting `Blocked`. Strolls are pulled onto the mesh too, so nobody
ambles off the quay. A scene with no bake, or a target no mesh reaches,
falls back to the straight-line walk. Brains decide at 10 Hz and only move
per frame while actually stepping.

### The night trip, and what makes it work

At nightfall a villager walks to a stand spot in front of its home's
doorway tile, faces the tile, the door leaf swings open, it steps onto the
tile, teleports to the interior landing and the door shuts behind it; at
dawn the reverse. Four things had to hold before that read as a routine
rather than as milling about outside:

- **A home is a HOUSE.** Retail's outside-to-inside teleports also cover
  caves and passages - town01's has its trigger 1.2 m below village level
  and a three-tile way out - and a villager sent "home" to one walks to
  the cave mouth and stands there. A home now needs a door LEAF prop on
  its tile or a single-tile way out; every rejection is logged with its
  teleport index, and `living_town/home_doors` / `exclude_doors` pin the
  verdict per scene.
- **The last stride is not steered.** A doorway is narrower than the
  probe fan can read as passable, so the steering picked a lane, committed
  to it, re-aimed, picked the other lane - and the villager circled a step
  short of its own front door for ever. Inside a stride of the target the
  lane picking is off; on a navmesh route it only ever detours around
  another VILLAGER, because the bake already used this villager's radius
  and height. The step from the stand spot onto the tile is a scripted
  slide, since the tile sits inside the frame with the (visually open,
  still solid) leaf beside it.
- **Giving up is bounded.** A failed trip is retried three times and then
  the villager stands at its door until dawn (brain state 9) instead of
  being sent back at an unreachable door every few seconds.
- **The shore is reachable.** See ledge links below.

#### Nobody stands in a doorway

A doorway tile is a teleport a player walks through, so it is the one
place in the village nobody may be parked - and the town had four ways
of arranging it: a "call on a neighbour" stand spot 0.7 m beside every
house door, viewpoints and path stands sampled with no regard for the
teleports, conversations struck up wherever people happened to be, and a
villager that gave up on its own front door standing in it until dawn.

The pass now lifts a world sphere from every one of the manifest's
doorway-teleport trigger boxes - both directions, since a cave mouth is
as much a thing a player walks into as a front door - and from every
home's door stand spot, doorway tile, landing, way out and village-side
landing. `MakeStation`, which every stand point in the town goes
through, pushes a spot out of a zone it landed in or drops it; the same
zones go onto the director (ad-hoc conversations) and onto every
locomotion controller (the autonomous stroll), so the rule holds at run
time as well as at build time. The doorstep stand sits 2.6 m out from
the tile, swept over a fan of bearings around the way in, and a villager
who gave up on the door steps a few metres back before waiting out the
night.

Indoors the radius is **capped**: retail's interior rooms put a landing
and its way out a metre apart, and at the village radius a room would
have nowhere to stand at all - so indoors the rule is "not on the tile",
not "1.5 m clear of it". The night door trip is exempt by construction;
it is a scripted sequence rather than a station, and stepping onto the
tile is the one legitimate visit to a doorway in the town.

The LivingTown check asserts no station stand point sits inside a zone
and reports how many were relocated and dropped; town01 builds 48 zones
(18 trigger boxes + 30 home markers), relocates 13 spots and drops 2.
The knobs are code defaults on `LegaiaLivingTownOptions`
(`doorstepStandDistance` 2.6, `keepOutMargin` 1.5, `keepOutHomeRadius`
1.2, `indoorKeepOut` 0.75, `keepOutPush` 1.6).

#### Villagers reading as people

Conversations got attention: every non-speaker turns to face whoever is
talking (with a small delay, so it reads as noticing rather than
snapping), one listener answers the topic with a reaction bubble a beat
later (a laugh after the fish story, "..." after the storm, a heart after
food - two answers per topic so the same topic twice does not draw the
same face), and the speaker nods - a short LateUpdate pitch of the torso
over whatever the Animator posed, the same composition the gait and the
sitting pose use, so no rig needs a clip for it. Two villagers walking an
errand together swap a bubble every eight seconds or so. And a villager
idling within two metres of the local player turns to look, and waves
once every forty seconds at most - local only, nothing synced. None of
this is a brain state; it rides on the states the town already had.

#### Somebody keeps the card table at night

`living_town.night_host` pins one villager to one station for the whole
night - town01 seats Cara at the card table's `stool_0`. She is an
ordinary villager by day; at nightfall the director claims the station
and sends her there instead of home, renews the hold every tick (the
station's own dwell timer would otherwise walk her off in the middle of
the night), parks her a couple of metres off facing the table while a
**player** has the seat, and lets her go at dawn. The seat is an
ordinary kind-2 station, so the card table's own handler sits her down
through the existing arrive event - the living town knows nothing about
card tables, and the link is a scene path the brain resolves by name at
Start, because the table belongs to another pass's container.

Every other picker leaves her alone for free: her brain reports itself
unavailable for the length of the shift. The night soak asserts she
reached the station, held it for most of the night, and never went home
through a door - and excludes her from the exodus count, which she would
fail by design. The card game's night rules are hers too; see "Common
prefabs".

### Ledge links (villagers jump)

A drop the agent cannot climb leaves the bake as two islands with no route
between them - town01's shore sits 1.2 m under the village, and the two
villagers standing on it had nowhere to go at night. The bake now finds
**ledge links**: pairs of points on its own boundary edges, close enough
together and short enough a drop to jump, with clear air over the gap and
no walk already connecting them. They are kept as a spanning structure
(cheapest first, one per join) so every island that can be reached is,
written as `living_town/navmesh/links/link_N` marker pairs, and
`living_town/nav_links` in the settings file pins one by hand. **Jump
height** and **Jump distance** are in the builder panel.

`LegaiaNpcWander` composes a route no walk completes as a CHAIN of walks
and hops over those links (up to three - town01's shore needs two), and
the hop itself is a scripted parabola with the floor ray off while
airborne, so the villager clears the bank instead of being dragged back
down it. `NavMesh.AddLink` IS exposed to Udon, but `NavMeshLinkData` has
no constructor extern, so a link cannot be built inside Udon at all -
composing the route keeps the hop under the kit's control anyway.

Per-scene tuning lives in the settings file (`living_town` for the cap,
the number of conversation spots, the seed, the daytime-indoors share, the
walk clip name, the house overrides and the hand-pinned ledge links;
`npc_homes` pins named villagers to a particular door - see "Per-scene
settings"). Headless check:
`Unity -batchmode -executeMethod LegaiaWorld.LegaiaBatchChecks.LivingTown` -
which also registers the bake and asserts a *complete* navmesh route from
every homed villager's spawn to its door stand spot, from there onto the
doorway tile, and from the landing to the way out, so a door nobody can
walk to fails the build instead of clipping in-world. The pass itself is
stricter about only one of those legs: a house whose door stand spot has
no navmesh path onto its doorway tile is **not handed out** (the villager
would stand at the door all night - town01's sixth house), while a
landing with no path to the way out is only logged, because the dawn
routine steps out through the door on its own when that leg is blocked.
Add `-legaiaViaRealism` to run the whole enhancement pass first
(`LegaiaRealism.Apply` with default options, then the per-scene
deletions) - the editor button's flow - instead of the living town alone;
the two have disagreed, and only that flow shows why. It asserts the
daytime layer the same way: every stand spot on real floor with room for
a body, every outdoor one reachable from the spawn or from some villager,
every carry endpoint carrying a `LegaiaNpcHandItem` whose wiring reached
its backing behaviour, every visit spot able to answer, at least one
conversation ring out of doors, and every villager's items parented under
it, collider-free and hidden at build.

### The play-mode soak

The checks above are edit-mode geometry: they never run a villager. The
**soak** does - it enters play mode with ClientSim, pins the day/night
clock, samples every brain at 2 Hz into a timeline file and asserts the
night routine actually happened:

```
Unity.exe -batchmode -nographics -projectPath <copy>
    -executeMethod LegaiaWorld.LegaiaBatchChecks.Soak
    -legaiaSoakMode night -legaiaSoakSeconds 300
    [-legaiaSoakScale 2] [-legaiaSoakLog <copy>\Logs\soak.timeline.log]
    -logFile <copy>\Logs\soak.log
```

No `-quit`: the run drives itself from `EditorApplication.update` and
exits the editor itself (0 pass, 1 an assertion failed, 3 play mode never
started, 4 the wall-clock watchdog). `headless-check.ps1 -PlayMode` gets
the `-quit` / no-`-quit` distinction right for you and puts the log where
the editor can actually open it. `night` forces night for the first
65% of the budget then dawn, and fails on a villager that never got
inside, never came back out, whose door never swung, or that gave up on
the trip. `day` forces day for the whole budget and REPORTS instead -
station visits, conversations, ledge hops, blocked walks and metres
walked per villager - which is the mode to run over a merged kit when a
new daytime layer lands. Udon's delayed events follow `Time.timeScale`
(the summary prints brain ticks per simulated second, ~10 either way), so
`-legaiaSoakScale 2` really does buy twice the simulated time.


## Common prefabs

The builder's **Common prefabs** foldout spawns the furniture most VRChat
worlds end up with, near the spawn, in its own top-level
`Legaia_common_prefabs` container (outside the mirrored root, like the
camp props - so an object's Inspector position is its world position).
Three are built by the kit itself, from primitives, generated textures
and the SDK's own components - no third-party package, no game data:

- **Mirror** (default on) - a VRC mirror with three buttons on a side
  post: *Mirror off* / *Mirror on* / *Mirror on (players only)*. The
  frame and glass only exist while a surface is on - off, the mirror is
  just its button post, so it never reads as a wall.
  Two surfaces stand behind the frame, one reflecting every layer and
  one reflecting only the `Player` / `PlayerLocal` / `MirrorReflection`
  layers against the skybox (the usual "low quality" mirror). It starts
  **off**, the choice is local (your mirror never costs another player
  a frame), and it switches itself off when you walk more than ~9 m
  away - a mirror renders the whole scene a second time, which is why
  every VRChat performance guide says "off by default, toggle nearby".
  The surface material is the SDK's own `MirrorReflection.mat`.
- **TV (video player)** (default on) - a cabinet with a 16:9 screen,
  a URL field + status line on the front, and *Play / Pause*, *Stop*,
  *Resync* buttons on top. Both SDK players sit on it: **AVPro** is used
  on PC (the only one that plays YouTube's current formats after
  VRChat's yt-dlp resolve), the **Unity** player on Android / Quest, and
  the Unity player everywhere when `preferUnityPlayer` is ticked on the
  behaviour - the Unity player is also the only one that runs inside
  the editor, and only with a direct video-file link. Type a YouTube /
  Twitch / direct link and press enter: the URL, a load serial, the
  playing flag and a server-time playhead origin sync from whoever
  pressed it, every client loads the video itself and seeks to
  `server time - origin`, so a late joiner lands where everyone else
  is. VRChat's one-load-per-5-seconds rate limit is honoured (a load
  inside the window is queued and the status line counts down) and
  `RateLimited` / player errors retry three times. Hosts outside
  VRChat's allow-list need each viewer's **Allow Untrusted URLs**
  setting on - the status line says *access denied* when that is the
  cause. Audio is one spatial speaker under the screen.
- **Card table** (default on) - a round table with felt, N stools
  (**Stools**, default 4) that are VRC stations (look at one, *Sit*),
  and a 52-card deck stacked face-down in the middle. Cards are
  pickups: hold one and press **Use** to flip it (the flip is synced,
  so a card you turn shows the same side to everyone), drop it on the
  felt to play it. Faces are generated - rank glyphs in the corners, a
  suit pip in the middle, framed letters for the court cards, a teal
  lattice back - into one atlas, one 63 x 88 mm box mesh per card.
  *Shuffle* restacks every card face-down in a seeded random order,
  *Gather* in new-deck order; both take ownership of each card, so the
  cards' own Object Sync carries the result. Like the equipment rack,
  a card starts kinematic and only goes physical the first time it is
  dropped (52 bodies waking during a world-load hitch is the
  tunnel-through-the-floor hazard). The card behaviour syncs
  *Continuous*, not *Manual*: the SDK's world validator refuses an
  Object Sync on the same object as a manually synchronized behaviour,
  and the headless self-test asserts the pairing so a build never
  trips on it again.
  Each stool is also a `LegaiaNpcStation` (kind 2) on one
  `LegaiaCardTableHost`, so villagers sit down and hold a fanned pair
  of card backs. Sitting down yourself is an **invitation**: while
  anyone is at the table the free stools stay open and the game calls
  the nearest idle villagers over to play (`LegaiaTownDirector.Summon`),
  and holds them there through a hand. The table's third button
  (*NPCs: sit / shoo*) is the override - shoo, and the stools are
  withdrawn and nobody is called. The seated pose is an
  **built pose** - the exported rigs carry no sit clip, so the host
  nudges the NPC onto the stool centre with its hips on the seat and
  the locomotion controller poses it sitting (`LegaiaNpcWander.SetSeated`)
  from the leg and arm pairs the living-town pass measured: thighs
  turned forward at the hip, shins hanging from the knee, upper arms
  brought forward so the hands reach the table, blended in over a
  third of a second on top of the idle clip. The hip is the measured
  thigh pivot where the rig has legs (so the thighs lie on the seat),
  else 45% of the rig's height (the one-piece bodies only bring their
  arms forward; both heights fold the renderer bounds by hand - see
  the struct-mutator trap under Troubleshooting). The hip turn is not a
  fixed angle from rest: the
  controller sweeps the turn about the rig's lateral axis and keeps the
  one that lands the knee level and ahead ON SCREEN (`sitKnee` is the
  knee's angle below level, `sitArm` the hand's angle forward of
  straight down), read through the rendered transform chain against
  the face as `VisualForward` reads it - so a villager who rests in a
  crouch (town01's Val) sits like one who rests upright. The builder's
  handedness mirror on the instance scale makes `transform.forward` and
  `TransformDirection` disagree with what is on screen, and the first
  cut, which compared those two, sat every rig with its knees behind
  it. (The cut before that sank the whole rig a third of its height
  into the ground.) `LegaiaBatchChecks.RigPose` measures every imported
  rig and holds the premise against the rig's own walk clip - the
  lifted foot travels toward the face, the only ground truth for
  "forward" a rig carries - and fails on a rig that was handed legs no
  hip turn can seat (the pass itself withholds the leg pair when the
  knee cannot reach level-and-ahead - a bather's folded legs, a
  mis-paired rig - and such a villager is seated by hip fraction like a
  one-piece body). Undone when it leaves. Villagers **walk** to the stool - the host sits a villager
  only once its brain reports it has arrived; the station names its
  villager at claim time, before the walk, and sitting on that alone
  snapped them onto the stools from across the square. The table calls
  villagers from within 28 m, which is what a 45 s walk covers. A
  villager that **gives up** on the way (blocked, or out of time) is not
  sent to that same stool again for two minutes
  (`LegaiaNpcBrain.failRetrySeconds`): the director summons nearest-first,
  so without that memory the villager standing beside the table was
  re-summoned onto the same failing approach every few seconds - a walk
  loop, with the brain's reason in its `lastFailure` field and the
  controller's in `blockReason` (what the probe hit, or which watchdog
  fired). The card soak logs every give-up with that reason and fails on
  the loop shape (one villager giving up on one stool three times).

  **The game** (`LegaiaCardGame`, on the table's `game` child) plays
  **poker** and **blackjack** with those 52 pickups, for the coins in
  your purse (see "Coins" below). A panel on a post beside the table
  shows the mode, each seat (a villager's rendered **face**, name, chips
  and *what that villager just said*; a player's silhouette, VRChat name
  and real coins), the board, the pot and a message line, and carries
  seven buttons: Deal, Mode, Check/Call, Bet/Raise, Fold, Hit, Stand.
  - **Deal** starts a hand, **Mode** switches poker and blackjack
    between hands; both need you seated.
  - **Which poker is the clock's call, not a button.** By day it is
    **hold'em**: ante 2, two hole cards a seat, then five community
    cards face down across the middle of the felt, turned over in
    stages - three for the flop, one for the turn, one for the river,
    about 0.7 s apart - with a betting round (Check/Call, Bet/Raise +2,
    at most three raises, Fold) before each and after the river. Best
    five of your seven takes the pot; ties split it.
  - **After dusk it is Cara's poker night** - the game on the rule
    poster by the table: five cards each, one betting round, best hand
    wins. No board, no draw. The switch only lands between hands, so a
    hand dealt in daylight finishes in daylight's rules, and the panel's
    mode line says which game is on.
  - **Showdown is paced.** The losing hands turn over first and the
    winner last, about 0.6 s apart, and only then is the pot named. Fold
    and *both* your cards leave the felt at once. When everyone but one
    seat folds, that seat takes the pot **without showing** - "wins
    uncontested".
  - Blackjack: stake 2, the table deals itself two cards (one up) and
    draws to 17 standing on soft 17; **Hit** / **Stand**; 1:1, a natural
    pays 3:2 rounded down, a push returns the stake.
  - Buttons that do not apply are greyed. A seat that does nothing for
    25 s is checked (or folded, if the call costs) so one AFK player
    never stalls the table. A seat that cannot cover the ante sits the
    hand out.
  - Nothing is taken until the hand settles: the whole hand is one net
    change to your purse at the showdown, applied by your own client;
    the dealer never writes anyone's coins. Stand up mid-hand and the
    hand is voided, nobody charged. Villagers play with virtual chips
    (60 when they sit) and never touch a player's coins.
  - Villagers play by hand strength with a nerve of their own (from
    their personality seed): pre-flop they read their two cards the way
    any starting-hand chart does, and after the flop they read the made
    hand on the visible board - discounted when the board alone makes
    it, because playing the board beats nobody.
  - With nobody at the table the villagers keep playing among
    themselves, a hand every few seconds, so the table looks alive from
    across the square (`npcSelfPlay` on the game turns it off).

  Say these out loud to your players: **your cards are dealt face up**
  (the deck is 52 real pickups anyone can walk round and pick up, so a
  hidden hand is not possible and the game does not pretend otherwise;
  villagers' cards are dealt face down and flip at the showdown). You
  can still carry a card off - the game tracks cards by identity, not
  position, and re-stacks them at the end of the hand. Which villager is
  in a seat is decided per client (every client walks its own copy of
  the town), while the seat, cards, pot and money are the same for
  everyone. The panel faces outward from its side of the table and
  turns with it when you move the table; to put it somewhere else, drag
  the table's `panel` child where you want it and pin its Inspector
  transform in the scene settings as `prefab_transforms.card_table_panel`
  (`{"position": [x, y, z], "rotation": [0, yaw, 0]}`, the panel root's
  LOCAL values under the table). Which way it then reads is yours: the
  board's readable face is the panel root's -Z side, so forward into
  the table reads from outside (the default) and forward away from it
  reads from the stools.

  **Table talk.** Each seat's row on the panel prints what that
  villager just said, under the name, fading after a while - at sitting
  down, the deal, each street of the board, a raise, a fold, a bad beat,
  the pot going one way, a real player joining, and now and then
  between hands.
  The lines are new, written for the world rather than lifted from it:
  retail Rim Elm has no card table, and each actor's manifest line is a
  cutscene fragment. So the talk is light and generic with the town's
  references (the Mist and the wall, the Genesis Tree, Hunter's Spring,
  Biron up the road, Val's leg, Mei's sewing). Three voices are their
  own: **Vahn** never speaks a word in retail, so his lines are stage
  directions (*Vahn nods to the table and sits.*); **Noa** was raised
  by a wolf in Snowdrift Cave and talks like it; **Cara** is Rim Elm's
  hunter girl, dry and sure of herself, and opens the night game with
  her four rules. When any of them sits down the other villagers talk
  *about* them (Val's boy, the wolf girl) every other line. Which pool applies is decided by the seat's name - so an
  `add_npcs` entry labelled `Vahn` or `Noa` (the party export names
  them) gets the voice, and a joke villager named Noa would too. Text
  is composed by the table's master and synced, so everyone reads the
  same words (`LegaiaTableTalk`).

- **Wall posters** (`LegaiaPosters`, from the settings file's `posters`
  list) - an image hung on a wall of the imported town. The kit has no
  authored wall list, so the pass finds one: 24 horizontal rays at eye
  height from the object the poster names (`near`), keeping surfaces
  that are vertical, face back toward that object, and continue upward,
  then taking the nearest whose plane covers the whole print at its
  hanging height - 1.55 m above the floor under *that* wall, which is
  not the floor the anchor stands on. The print is 1 cm off the wall in
  a dark frame, on the kit's lit shader with the light wrap raised so a
  photograph does not get a shading terminator across it. The search is
  a fallback: drag the poster where you want it and run **Legaia >
  Snapshot placements** - it is an ordinary named child of
  `Legaia_common_prefabs`, so it lands in `prefab_transforms` as
  `poster_<name>` and the next build reproduces the placement instead
  of searching. town01 is the case that shows why you might: its card
  table stands in an open yard and the nearest wall that fits the print
  is 8.7 m away, which the build says in the log.

The rest are spawned **from prefabs already in your project**:

- **SDK pen system** (default off; greyed out when the SDK sample isn't
  installed) - instantiates the worlds SDK's own `SimplePenSystem`
  sample prefab (`Packages/com.vrchat.worlds/Samples/UdonExampleScene/Prefabs/`).
- **Slot machine (cabinet + minigame)** (default on) - instantiates the
  casino cabinet model (**Cabinet asset**, default
  `Assets/Prefabs/legaia slot machine.glb`; a moved file is found again
  by name) at the offset / scale, then runs the slot-machine builder on
  it from **Slot art folder** (the `asset slot-art` export - see "The
  casino slot machine"). The scene settings' `slot_machine` block
  overrides asset, art folder, position, rotation and scale, so a
  hand-placed cabinet comes back on every rebuild; a cabinet of the
  same asset left at the scene root by the old drag-it-in workflow is
  retired (logged) so there is never a second machine. The kit ships no
  cabinet model (ours carries disc-derived art).
- **Extra prefabs** - *+ Add prefab slot*, drag any prefab asset from
  the Project window into it, set its offset from spawn and a yaw. The
  builder instantiates it as a prefab instance (so it keeps its link
  to the package), grounds it, and turns it to face the spawn plus the
  yaw. This is the hook for the community prefabs below.

Every item takes an **Offset from spawn** (grounded on the world
collider) and turns to face the spawn. To pin the placement you settle
on by hand, drag things where you want them, then **Snapshot placements
to scene settings** (the button under the foldout, or the
`Legaia > Snapshot placements to scene settings` menu). It writes the
current Inspector position + rotation of every common prefab, the camp
settings panel and LegaiaSpawn into the scene's settings file:

```json
"prefab_transforms": {
  "tv":   { "position": [40.68, -0.003, 15.14], "rotation": [0, 89.265, 0] },
  "menu": { "position": [34.978, 0.801, 19.277], "rotation": [0, -0.422, 0] }
}
```

(keys `mirror`, `tv`, `card_table`, `pens`, `menu`, and an extra slot's
prefab name; values are exactly the Inspector numbers, world == local
under the origin containers; `rotation` is optional). The slot cabinet
gets its own block, found through its `LegaiaSlotGame` rig wherever it
sits, with the asset path, art folder and uniform scale alongside:

```json
"slot_machine": { "cabinet": "Assets/Prefabs/legaia slot machine.glb",
                  "art": "Assets/LegaiaImports/slot-art",
                  "position": [37, 0.979, 36.416], "rotation": [0, -71.178, 0], "scale": 0.012 }
```

The snapshot
writes the Unity project's copy of the file - port it back into the
kit's `Settings/` folder, which is the source of truth (the sync script
refuses to clobber a newer project-side file until you do). **Place
common prefabs near spawn (existing root)** rebuilds just this
container against the built scene, so re-placing the TV never forces a
full rebuild; unticking everything removes it. `delete_objects` in the
settings file also searches this container.

Shading: after the build, every generated material is converted to the
kit's lit vertex-colour shaders - the same pass the slot-machine cabinet
gets - so the furniture sits under the scene's sun and ambient instead
of Standard-PBR defaults. The mirror surfaces and the video screen keep
their display shaders, and spawned prefabs (the SDK pens, anything in
an extra slot) keep their authored materials.

### Community prefabs worth knowing

Surveyed when this section was written, for the *Extra prefabs* slots.
The kit bundles none of them - each has its own author, licence and
install route (most via a VCC / VPM listing), and the ones without a
licence file are for you to clear with the author before shipping:

| Need | Prefab | Licence | Notes |
|---|---|---|---|
| Video | [USharpVideo](https://github.com/MerlinVR/USharpVideo) (MerlinVR) | MIT | The reference UdonSharp player; the kit's TV follows its sync model. Unmaintained since 2024 but still works. |
| Video | ProTV (ArchiTechAnon), VideoTXL (Texelsaur) | see author | Full-featured players (playlists, queues, screens); VCC listings on their authors' sites. |
| Pens | [QvPen](https://github.com/ureishi/QvPen) (ureishi) | none published | The de-facto world pen; VCC listing on the repo. |
| Cards | [Vowgan's Deck of Cards](https://github.com/VirtualVisions/VowganPrefabs) | none published (Booth / Patreon) | Chess set, clocks, music player in the same repo. |
| Cards | [BigDeckIsBackInTown](https://github.com/MMMaellon/BigDeckIsBackInTown) (MMMaellon) | none published | Performance-minded deck with dealing spots; VCC listing on the repo. |
| Chess | [EmyChess](https://github.com/emymin/EmyChess) | GPL-3.0 | Note the copyleft licence before mixing it into a world. |
| Mirror / chair / portal / avatar pedestal | VRChat worlds SDK samples | SDK | `VRCMirror`, `VRCChair3`, `VRCPortalMarker` (needs a world id, must sit at the scene root), `AvatarPedestal` (needs an avatar id) - drop them into an Extra prefab slot. |

The kit builds its own mirror, TV and deck rather than depending on any
of these because the three are small, the SDK ships every component
they need, and a kit that can build a scene from nothing but the SDK
stays reproducible from a fresh Creator Companion project.

## The casino slot machine

The Sol/Vidna casino slot minigame
([`docs/subsystems/minigame-slot-machine.md`](../../docs/subsystems/minigame-slot-machine.md))
as a playable cabinet: walk up, press the three face buttons, spin. The
rules are the engine's ported kernel - the retail reel strips and LCG,
the flat 3-coin bet across all five paylines, the net-take-bracketed
feature odds, the kick/punch jackpot symbols opening 1/3 bonus rounds,
and the bonus round's unsteered stops paying the product of the three
numbers (1..1000 coins). The balance starts at retail's 70-coin
dev-launch fallback and refills on empty (free play - a VRChat world has
no casino coin bank to cash out to).

1. Export the machine's art off your disc (into the Unity project's
   import folder, which stays local like every other export):

   ```bash
   cargo build --release -p legaia-asset
   ./target/release/asset slot-art extracted/PROT/0975_*.BIN \
       extracted/PROT/1200_*.BIN \
       --out "<unity project>/Assets/LegaiaImports/slot-art"
   ```

   That is the 10 reel symbols + 10 bonus numerals (each through its own
   CLUT column - the palette is load-bearing), the payline lamps,
   medallions, reel-stop pedestals, marquee panel + mascots, the 21
   dot-matrix messages, the paytable board, and `slot-machine.json`
   (payout table + the disc's own geometry tables).
2. Put a cabinet mesh in the scene. The repo ships no cabinet (ours
   carries disc-derived side art, so it can't) - any model works if it
   meets the builder's contract, and every part of it has a fallback:

   - a node named `screen` (the **Screen node** field): a flat face
     looking along the cabinet's **+Z** that the composition snaps to.
     Without one, clear the field and place the screen by hand via the
     position/yaw/width fields;
   - three button nodes (default `Circle.001;Circle.002;Circle.003`,
     the **Button nodes** field), wired left-to-right as the player
     sees them. Missing nodes get generated fallback pads;
   - no cabinet at all also builds - the machine lands at the scene
     root with fallback pads, ready to be framed by any prop.

   A Blender-baked cabinet often fails glTFast's import with `UVMulti`
   errors (materials sampling UV set 2/3; web viewers render the same
   file fine) - run `python fix-glb-uvsets.py in.glb out.glb` on it
   first, and again after any re-export.
3. Menu **Legaia > Build Slot Machine...**, point it at the cabinet root
   and the art folder, **Build slot machine**. The whole screen
   composition is parented *directly onto the cabinet's screen face* -
   no hidden studio, no camera, no RenderTexture. The composition root
   scales the retail 640x240 frame to the screen (window derived from
   the manifest's projection block - `minigame_slot_scene` constants),
   mirrors x once (a +z-facing quad reads mirrored to the player, so
   exactly one flip is correct - every kit slot shader is `Cull Off`
   because that mirror reverses winding), flattens z to millimetres of
   relief (overlay order comes from a material renderQueue ladder, not
   the flattened z gaps), and applies the retail projection in software
   (`k = z0/(z0-z)` about the model origin, z0 = 9324; the behaviour
   applies the same k to the 8 drum faces per frame). Billboard
   half-extents are view-space in the disc tables and divide by the
   camera matrix's x scale (and gain the aspect in y) to reach model
   units; the paytable and HUD are retail's raw screen-space draws and
   bypass the projection. A cabinet node named `screen` (the **Screen
   node** field)
   pins the build automatically: position, yaw and width are derived
   from that face, in cabinet-local space, so import scale is respected
   and a mesh swap is "select cabinet, Build". Without one, drag the
   `LegaiaSlotGame` root to place the screen, then copy its transform
   into the window fields so a rebuild lands in the same place.
   Per-frame visual updates pause per-client when nobody is near the
   machine (the last-drawn frame stays up). Named button nodes get
   fitted colliders + `LegaiaSlotButton`; missing ones (a stale mesh)
   get fallback pads under the screen. **Match world shading** (default
   on) converts the cabinet's imported glTFast PBR materials to the
   kit's lit vertex-color shaders so the prop sits under the same sun
   and ambient as the scene; the screen composition stays unlit - it
   is a display, it should glow. The HUD uses TextMeshPro (legacy
   TextMesh is not exposed to Udon): accept Unity's **Import TMP
   Essentials** prompt when it appears, or the balance/status text
   renders nothing - then rebuild once so the text components pick up
   the default font.

Sync model: a player takes ownership of the machine by pressing a button
**while it is idle**, and holds it through the spin - one seat per
session, as retail (this also closes the race where a second player
could re-stop a reel). Outcomes (stop rows, wins, balance) sync, and
every client animates the reels locally from the same seed-deterministic
strips - so onlookers see the same spin without per-frame traffic. Both
RNG streams sync alongside the outcomes, so the next owner continues the
same streams instead of forking them.

Testing: **Legaia > Verify Slot Rules** replays `slot-verify.json` (a
scripted trace generated by the repo's `engine-core`
`slot_verify_fixture.rs` test) against the actual UdonSharp class and
diffs every field - run it after touching `LegaiaSlotMachine.cs`, and
regenerate the fixture (`LEGAIA_BLESS_SLOT_FIXTURE=1 cargo test -p
legaia-engine-core --test slot_verify_fixture`) after an intentional
rules change. **Legaia > Slot Screen Snapshot** films the on-cabinet
composition head-on from the player's side (what a player sees, no
flip) and saves a PNG for eyeballing against the site's minigames page.

Faithful vs approximated, on top of the engine port's own notes: the
reel drum is retail's (8 faces re-derived per frame at 22.5 degrees on
the y=585 / z=512 ellipse, payline row on the peak-shade face, the
rand%5 sub-row landing nudge applied) and the depth-cue shade is
retail's formula per pixel in `SlotReelFace.shader`; the dot-matrix
marquee is composed at message granularity rather than per-dot (tally /
pips / payout caption / scrolled attract legend), the cash-out submenu
is dropped, an unclaimed payout auto-collects after a hold
(`autoCollect` off restores retail's wait-for-input), the `richer_odds`
widen roll is not modelled, and the machine's sound cues are not yet
wired.

## Coins

Every player carries one purse (`LegaiaWallet`, at
`Legaia_common_prefabs/wallet`), kept in VRChat **PlayerData** under
`legaia.coins` and restored when you join - the balance you leave with
is the balance you come back to, on any instance, on any day. A first
visit is seeded with 70 coins, retail's casino entry balance. A client
can only ever write its own purse, which is the anti-cheat model rather
than a limitation: nothing in the world can spend or grant another
player's coins, and a panel that shows what someone else holds reads it
straight from their replicated PlayerData. Consumers are wired to the
purse at build time and fall back to finding it by that path at
runtime, so pass order does not matter. Persistence needs a worlds SDK
with PlayerData (3.7.4+); before the restore lands, a few seconds after
joining, the purse holds the seed in memory and writes through
afterwards.

Where coins go and come from:

- **The slot machine spends and pays the purse.** The cabinet has no
  balance of its own any more: a spin takes 3 coins (1 on a bonus or
  feature spin) from the seated player and a collect pays them. The
  number on the glass is the OWNER's coins, whoever is at the machine,
  updated within half a second of the purse changing anywhere. Out of
  coins means out of coins: the machine shows `OUT OF COINS` and refuses
  (a refusal draws nothing, so the RNG stream sits where the next
  affordable spin finds it). The old free-play refill only applies to a
  cabinet built without a purse, and the retail rules are untouched
  (`LegaiaEconomyChecks.SlotRules` replays the fixture against the class).
- **The card table** bets from it - see "Common prefabs" above.
- **Fishing** is the reliable earner. Every shoreline fishing spot has a
  stake with a bucket beside it: Interact ("Fish") to start. The rod
  tracks your right hand (your head on desktop), the float casts out,
  villagers stay off the spot while you have it. After 6-20 s something
  bites: the float dips and a `!` shows over the stake for 1.6 s -
  Interact inside that window to land it, 2-8 coins and a `+n coins`
  line, then the float goes back out. Interact outside the window, or
  walk more than 3 m away, to pack up. A villager already fishing there
  keeps the spot. Fishing is entirely local: two players on the same
  shore see their own float and bites, and only their own purse is
  credited.
- **Hunting** is the fast, rude earner. Take a **weapon** off the
  equipment rack (only the manifest's Weapon rows count - a shield will
  not do it) and swing it through a villager at 2.5 m/s or more. The
  villager topples where it stood, fades out after ~3 s and drops 5-15
  coins there; Interact the coin ("Take coins") to pocket it. A drop
  lies for 90 s, at most twelve are on the ground at once (the oldest
  gives up its place), and two players clicking the same coin resolve
  the same way on every client - the take is one server-ordered network
  event and only the first is paid. The villager **respawns after 120 s**
  at its own spawn point and goes back to its routine; one swing through
  a body is one strike (a 1.5 s per-villager cooldown), and only the
  swinging player's own client registers it. A rack placed before this
  layer existed carries no weapon flags - re-run **Place equipment rack
  near spawn** once.

Per-scene tuning, in the settings file's `living_town` block:
`"bounty": false` removes the hitboxes and the coin pool (a scene where
villagers cannot be struck), `"respawn_seconds"` sets how long a slain
villager stays down (120), `"coin_drop": [min, max]` what one strike is
worth (5-15). There is no coin sink other than the two games yet:
fishing and hunting only ever add.

Headless checks: `LegaiaWorld.LegaiaEconomyChecks.Run` (edit mode),
`.SlotRules`, and `.Soak` (play mode, no `-quit`: strikes a villager
through the hitbox's debug hook, sees the coin land and the villager
come back); `LegaiaWorld.LegaiaCardGameChecks.Run` and `.Soak` (play
mode, no `-quit`, `-legaiaCardSeconds N -legaiaCardScale S`: villagers
play hands among themselves, pot conservation asserted, both modes).

## Optional realism enhancements

The builder window's **Realism enhancements** foldout layers a set of
optional passes over the built root. Every pass defaults **on** - untick
them all for the faithful retail-shaded scene. Everything the passes
create is generated from scratch (shaders,
dome/grass geometry, synthesized audio): no game data is produced or
shipped beyond what the export already decoded. The **Apply enhancements to the
already-built root** button reruns just these passes over an existing
`Legaia_<scene>` root, so tuning a slider doesn't force a rebuild; each
pass is idempotent (it refreshes rather than stacks).

- **Realistic lighting**: the exported glbs are `KHR_materials_unlit` and
  carry **no normals**, so Unity lights can't touch them as imported. The
  pass duplicates every mesh into `Assets/LegaiaGenerated/<scene>/realism/`
  with smoothed, position-welded normals (sign-aligned - the PSX source
  winding is mixed, so raw face normals point both ways and would cancel;
  the lit shaders then light with the sign-independent two-sided Lambert
  `|N.L|`, since no per-vertex sign choice survives this data), swaps
  every material for `Legaia/Lit Vertex Color` (cutout or transparent by
  queue), and adds a warm directional sun with soft shadows plus a
  trilight ambient. Semi-transparent materials the export names
  `legaia_semi_abr1` / `abr2` / `abr3` (PSX additive / subtractive /
  quarter-additive blend rates - core glTF can only express alpha blend,
  so the name carries the real rate) route instead to the unlit
  `Legaia/Vertex Color (Additive)` shader: retail's window light shafts
  and glows only ever brighten what's behind them, and alpha-blending
  them reads as a grey film. Rate 0 (the 50/50 average - water sheets)
  IS alpha blending and stays on the lit transparent shader. Re-exported
  glbs are required for this: pre-existing exports carry one unnamed
  BLEND material and keep the old grey behaviour. The baked `COLOR_0` retail shading keeps modulating
  every surface, so the scene holds its palette - lighting layers on top
  instead of replacing it. NPC and prop materials additionally get
  **light wrap** (`_LightWrap`, slider "NPC/prop light wrap"): the
  `|N.L|` terminator cuts a harsh dark band right across a low-poly
  villager's face, so their angular term is flattened toward even
  lighting (shadow maps still attenuate); world surfaces keep the full
  directional response.
- **Day / night cycle** (under lighting): the `LegaiaDayNight` Udon
  behaviour sweeps the sun through a full day on a fixed cycle, with night
  compressed (`dayShare`). Every client derives the same angle from the
  shared server clock, so the cycle is synced with no networking events.
  Night genuinely darkens the landscape: the behaviour sweeps the trilight
  ambient (and fog colour) down to a moonlit, blue-shifted fraction of
  their daytime values ("Night darkness" slider, default 0.02 - night is
  nearly black so the lamps and fires carve out the light; sun intensity
  alone leaves the ambient day-bright after sunset). **Night lamps** places a small
  warm light (no visible bulb mesh - the pool of light on the wall is
  the whole effect) at each village building **window**, anchored on
  the world mesh itself: the retail scene authors semi-transparent glow
  volumes exactly where light spills out of a hut window (town01 repeats
  one identically-sized glow object across three huts), so each
  village-side BLEND submesh of window-glow proportions anchors a
  tight-radius light. The glow volume is the light *shaft* angled down
  toward the ground - its centroid hangs in mid-air off the wall - so
  the lamp anchors on the shaft's own geometry: the centroid of its top
  band of vertices is the window opening, nudged slightly along the
  spill direction (a raycast wall-snap was tried first and grabbed
  unrelated nearby walls such as the palisade). Scenes with no authored
  glows fall back to a lamp above each village-side doorway (manifest
  teleport endpoints). The `night_lamps` container is enabled by the
  day/night behaviour only while the sun is below the horizon.
  **Night torches** plants a burning stake torch (same flame stack as
  the camp props: fire + smoke particles, flickering light, crackle -
  no pickup) beside each village doorway and by each tree - trees are
  found in the world mesh itself as clusters of green-reading upward
  triangles floating well above the local ground (the grass pass's
  canopy-rejection test, inverted). They live in a top-level
  `Legaia_night_torches` container the day/night behaviour enables
  alongside the lamps.
- **Sky + distance fog**: a procedural-skybox material (it tracks
  `RenderSettings.sun`, so with day/night on the sky darkens by itself)
  and linear fog scaled to the built root's bounds.
- **Ground foliage**: procedural grass - single-triangle blades in tufts,
  scattered over upward-facing world triangles whose ground colour reads
  green (texel x mean vertex colour at the triangle centre, the same
  product the retail shading displays). Ground only: a per-cell lowest
  upward-surface grid rejects any green triangle floating above other
  geometry, so tree canopies and roofs never sprout grass. Blades are tinted from the sampled
  ground so they blend with the terrain, and sway via `Legaia/Grass Wind`
  (weight in vertex alpha, world-position phase). Tune **density** and the
  **green threshold** (lower = more coverage, higher = keeps grass off
  paths); the scatter is deterministic per seed, capped at 25k tufts, and
  each rerun rescatters instead of stacking.
- **Interior room shells**: the doorway-teleport interiors are unused
  corners of the same map, so from inside a room you see the skybox above
  and the floating village past the doorway - retail frames these rooms
  against black. The pass detects each detached room from the manifest's
  own teleport data (endpoints beyond a spawn-distance threshold,
  clustered per room, then flood-filled outward to the whole building's
  meshes so the dome centres on the room, not on its doorway) and wraps
  it in a black ellipsoid dome fitted per-axis to the room's own geometry
  (a circumscribing sphere reached its half-diagonal in every direction
  and bled into neighbouring rooms), wound to face **inward only**:
  black space from inside, backface-culled (invisible) from outside,
  casting no shadow so the sun still lights the room. **Window light** adds a warm fill light per room
  so it reads window-lit inside its black surround.
- **Smooth textures**: bilinear + anisotropic filtering on every texture
  under the root, instead of the exports' PSX point sampling. This edits
  the imported texture objects in place, so a glb **reimport resets it** -
  rerun the pass after one.
- **Ambient audio**: four long synthesized 2D beds (a wind/surf base, a
  day bed, a night bed and a wind-gust bed) plus spatial emitters at the
  shoreline, in the tree canopies and on the windmill, all driven by one
  `LegaiaAmbienceMixer` behaviour that crossfades them with the sun and
  takes the weather layer's wind level. Generated audio, not
  from the disc - the atmosphere holds even with the music muted from the
  settings panel, and any role can be swapped for your own recording.
  Details, loop lengths and the settings block: "Ambient audio" below.
- **Villagers wander**: wires `LegaiaNpcWander` on every talk-kind NPC
  from the manifest (matched by spawn position), so the town strolls
  instead of standing still. The behaviour is collision-aware: strolls are
  clamped against the world's colliders, a blocked walk re-picks instead
  of clipping through a hut, and a downward ray follows the floor.
  Movement is forward-only: a direction change pivots the whole body in
  place first, then steps off - an NPC never translates while mis-facing.
  Facing is **measured, not derived**: the exported rigs have no skins
  (each TMD object is a rigid mesh node) and the authored facing is baked
  into the node rest rotations themselves (the MAN placement has no
  facing byte), under a stack of mirrors and importer conversions that
  defeats sign-by-sign algebra. So at Start the behaviour picks the
  largest mesh node that rests upright (the torso), reads the direction
  it visibly faces off its `localToWorldMatrix` every walking frame
  (baked yaw, idle sway, every mirror included by construction), probes
  which way that visual forward responds to a transform yaw, and servos
  the yaw until the mesh faces the walk direction. The face-axis
  invariant (textured 4-view renders of every town01 model - wireframes
  cannot tell front from back) is **+Z in the glb scene frame at
  rest**; it is NOT a fixed node-local axis, because one rig family
  rests its nodes at -90 degrees with the vertices counter-rotated
  (npc_12 and kin, which walked sideways until the anchor's rest
  rotation was folded out of the measurement at Start). `flipFacing`
  covers a model violating the invariant; `facingYawOffset` adds a
  manual trim, and the realism foldout's **Facing overrides** field
  (`npc_30:90`, keys matching the NPC glb file name) applies such trims
  durably across rebuilds. Wall/floor ray heights are measured from the
  rendered model at Start, so they track any export scale.
- **Weather**: a synced-by-clock weather schedule (`LegaiaWeather`), the
  same trick the day/night cycle uses - every client derives the current
  spell from the shared server clock, so a grey stretch arrives
  everywhere at the same second with no networking at all. The schedule
  is a repeating ring of 16 spells, each 3-8 minutes, kind and length
  hashed from the entry index. Three kinds - **clear**, **overcast**,
  **windy** - never two of a kind in a row, with the targets
  cross-fading over ~45 s at each seam. Two effects, each its own toggle
  on the behaviour:
  - **Sky** - the trilight ambient and the fog colour are greyed and
    dimmed by cloud cover and the fog pulled in. It runs in
    `LateUpdate`, multiplying what `LegaiaDayNight` wrote in `Update`
    that same frame, so the two never fight over the same render
    settings; with no cycle in the scene the behaviour multiplies its
    own Start-captured base instead. Either way the darkening applies
    exactly once per frame and never accumulates.
  - **Wind** - `_WindGust` on the procedural grass material scales the
    sway's amplitude and speed, so a gust bends the whole meadow. It
    is a material property and not a global because
    `Shader.SetGlobalFloat` is **not exposed to Udon** (`Material.SetFloat`
    is) - and the property's default of 1 is the right failure mode: a
    world built without the weather pass sways exactly as it always did.

  The behaviour publishes `cloudiness` and `windStrength` (0..1), and
  pushes `windLevel` into the ambience mixer when the scene has one.
  `JumpToClear` jumps the shared schedule the way the settings panel's
  Day / Night buttons jump the sun.
- **Living props**: the shoreline **fishing spots** - 3-4
  `LegaiaNpcStation`s (kind 1) in a `living_props` container under the
  built root, where the NPC director finds them. The builder locates
  them from the world mesh alone: water is the same large-flat-and-
  horizontal transparent sheet the collider pass keeps a floor over
  (see "Transparent surfaces"), land is the lowest upward-facing opaque
  triangle per 0.75 m cell, and a shore cell is a land cell 0.02-2 m
  above the water with water within ~1.5 m, on the village side of the
  map. Each candidate then has to survive physics - a floor ray at the
  expected height and a clear standing capsule - because a raycast
  alone cannot tell land from water here (the merged collider
  deliberately includes the sea). While a villager stands at one it
  holds out a rod with a line down to a bobbing float, ripple rings
  spread on the water, and every 20-60 s something bites and a fish
  flashes up the line. The rod is **not** parented to the NPC: these
  rigs have no hand bone, so it is placed each frame at a hand height
  measured from the NPC's own rendered bounds. A player standing on the
  spot frees the station, so nobody gets fished through.

Caveats: the sun / ambient / skybox / fog are **per-Unity-scene render
settings** - applying them from one built root is global, the last applied
root wins, and turning the options off later does not revert them (reset
via `Window > Rendering > Lighting`, and delete the root's `LegaiaSun` /
`foliage` / `interiors` / `ambience` / `night_lamps` children). The day/night and wander passes need the
VRChat SDK, same as doors and teleports. And the grass + realtime shadows
budget is a PC-world budget - trim density and shadow strength for a Quest
target.

### Ambient audio

The ambience pass builds an `ambience` container under the built root and
puts one `LegaiaAmbienceMixer` Udon behaviour on it. That behaviour owns
every ambient volume in the world, so the day/night cycle, the weather
layer and distance never fight over the same `AudioSource`.

Four flat 2D beds, all synthesized into
`Assets/LegaiaGenerated/<scene>/realism/`:

| Bed | Loop | What it is |
|---|---|---|
| `bed_base` | 75 s | Wind over distant surf. Always audible. |
| `bed_day` | 100 s | Breeze, leaf rustle, distant birds. Faded in with the sun. |
| `bed_night` | 100 s | Cricket chorus whose density drifts, plus frogs and a far owl. |
| `bed_wind_gust` | 32 s | Strong gusts with two resonant howls. Silent until asked for. |

Spatial emitters, placed from the scene's own geometry:

| Emitter | Loop | Where it goes |
|---|---|---|
| `waves_*` | 90 s | Three to five along the water edge nearest the village, nudged onto the land side. Water sheets are recognised the way the collider pass recognises them: a large, flat, semi-transparent submesh is the sea, any other semi-transparent shape is a window light shaft. Far 42 m. |
| `birds_*` | 52 s | Up to six tree canopies (the night-torch pass's canopy clusters), 3.5 m up the trunk. Day only. Far 25 m. |
| `wildlife_*` | 64 s | Owls, frogs and crickets by the further trees and at the water. Night only. Far 28 m. |
| `windmill_*` | 12 s | Each free-running animated prop over 3 m tall: a whoosh per blade pass on a 4 s rotation, plus one timber creak per turn. Far 26 m. |

**Why they are long, and why they do not tick.** The first generation of
these beds was 12-16 s of filtered noise gain-modulated by sine LFOs at a
whole number of cycles per loop, and it read as a fast, annoying repeat.
Nothing here is driven by a sine LFO or sits on a fixed grid. All the slow
motion comes from multi-octave value noise (0.02-0.5 Hz) whose control
points wrap circularly over the clip: the envelope is exactly periodic at
the *loop* length and carries no shorter period. Every discrete sound -
bird call, wave wash, cricket chirp, drip, owl hoot - is Poisson-scheduled
from a seeded RNG with its pitch, length, note count and rhythm jittered
per event, and bird calls draw from five different voices (swept chirp,
trill, two-note whistle, peep, a rare distant crow). Continuous layers are
rendered past the end and crossfaded onto the head; events wrap the seam
instead of dodging it, so there is no density dip at the loop point.
Generation is seeded and deterministic, takes well under a second per
clip, and writes 22050 Hz mono wavs that are re-imported as streaming
Vorbis.

**Volumes.** The foldout's **Ambient volume** stays the master: every
level is a multiple of it, the beds land near 0.14-0.22, and the emitters
sit lower still at the listener. The beds are normalized to about
-18 dBFS RMS so the BGM stays on top of them.

**Weather.** The mixer exposes one float the weather layer writes,
`windLevel`, 0..1 and slewed here so a caller may step it instantly. It
fades `bed_wind_gust` in and lifts the base bed.

**Your own audio.** Nothing here ships third-party audio, and the
synthesized beds are a floor, not a ceiling - a good field recording will
beat them. Drop any AudioClip into the project and name it per role in
`Settings/<scene>.settings.json`:

```json
"ambience": {
  "day": "Assets/LegaiaWorld/Audio/day.wav",
  "night": "Assets/LegaiaWorld/Audio/night.wav",
  "base": "...", "waves": "...", "wind_gust": "...",
  "tree_birds": "...", "night_wildlife": "...", "windmill": "..."
}
```

Every key is optional and a role with no entry keeps its synthesized clip
(the wav for a replaced role is never generated). Aim for seamless loops
of a minute or more. Vetted places to find CC0 / royalty-free ambience:
freesound.org with the licence filter set to CC0, Pixabay's sound
section, and OpenGameArt's CC0 tag. Check the licence on the individual
file - a CC-BY clip needs attribution in your world description, and the
kit itself ships no third-party audio.


## Troubleshooting

- **Villagers sit with their feet on the seat, or measure a third of
  their height** (the walk's probe ray at shin height, the hip landing
  0.3 m above the stool): a mutating struct method in UdonSharp.
  `Bounds b = rs[0].bounds; b.Encapsulate(rs[i].bounds)` compiles, but
  under Udon the extern mutates a *copy* and `b` stays the first
  renderer's bounds - the head - so every villager measured 0.35 m tall
  with its floor 0.7 m up and a negative hip. The kit folds renderer
  bounds by hand (min/max of `bounds.min.y` / `bounds.max.y`); never
  call `Encapsulate`, `Expand`, `SetMinMax` or any other struct mutator
  on a local or field in U# code. The play-mode soak's "seat geometry"
  lines (origin, hip, knee and sole above the stool floor) fail on a hip
  more than 6 cm off the seat top or a sole above it.
- **Doubled villagers standing inside each other** (town01: the two pairs
  of kids at the north square): a stale export. Retail stages some
  placements on another actor's exact tile and teleports them across town
  in the record's spawn prologue before the first frame; the manifest now
  resolves that relocation (`npc_catalog` +
  `placement_spawn_relocation`), places the runners at their real spots,
  and hides the dev records retail parks off-map. Re-run `export-glb` and
  rebuild the scene.
- **Everything looks mirrored**: it is - the raw import is X-mirrored
  relative to the site's field-scene viewer, and the builder's **Match
  explorer orientation** option (default on) mirrors the built root to
  compensate. This was settled *empirically* with a landmark test on
  town01, after deriving it from the shader reflection chain
  (`u_pair_front` in `site/js/webgl-shaders.js`) produced confident wrong
  answers in **both** directions: stand at the sea looking at the village
  (the sea-to-gate axis pins the viewpoint, so only parity can differ) -
  the raw glb puts the big terrace house left and the paired small huts
  right, the explorer page shows the opposite sides, and no rotation
  swaps sides across a content-pinned axis. Re-run that test rather than
  re-counting reflections. The double-sided merged collider keeps physics
  solid under the negative scale. A prop facing backward on a *different*
  importer is the separate handedness convention - flip `YAW_SIGN` in
  `LegaiaWorldBuilder.cs`.
- **Individual buildings face the wrong way** (windmill blades edge-on,
  a hut's door on the wrong side) while the overall layout is right: two
  separate causes, and each was fixed once - re-export with a current
  build and re-run a current builder before debugging further.
  1. *Yaw sign in the bake* (`legaia_asset::scene_gltf`): the site's
     `placementModelScaledY` has a transposed inline rotation block, and
     the bake once emitted the unnegated param, facing every yawed
     instance backwards while leaving every position (and any layout
     check) correct. Fixed at the node-quaternion emission; the manifest
     yaw flipped with it.
  2. *Handedness of placed props* (the builder): the world glb bakes the
     site's Y-mirror into its vertices (det -1) while NPC / prop glbs
     are proper-rotation models (root `Rx(180)`, det +1) - opposite
     chirality, so NO yaw value can align a placed prop with its baked
     frame-0 twin (a mirrored hut has its door on the wrong side at
     every angle; this is what made yaw-sign experiments look
     inconsistent). The builder supplies the missing mirror with a
     negative Z instance scale (`PROP_NPC_SCALE_Z`); with it, prop and
     twin coincide exactly, which is the invariant to check first
     (temporarily untick **Hide static prop twins**: each animated prop
     must z-fight its twin, not sit rotated against it).
- **Collider errors at build time**: with the default merged collider the
  cook runs off a generated readable asset and this doesn't arise; if you
  switched to per-mesh colliders, enable **Read/Write** in the glb's
  import inspector (Unity needs readable meshes to cook MeshColliders
  into a client build).
- **Falling through the floor**: use the default **Merged + welded**
  world collider. Two distinct causes it removes: per-mesh colliders leave
  hairline gaps where adjacent tile meshes meet (and silently vanish from
  client builds when the glb isn't Read/Write) - and, the big one, PhysX
  triangle meshes collide on the wound face only, while the PSX source
  data's winding is **mixed** (retail culled per-view via NCLIP; every
  renderer here draws double-sided, so it never shows). A single-sided
  collider therefore drops you through roughly half the floors. The
  merged collider appends every triangle reversed, so all geometry is
  solid from both sides.
- **Transparent surfaces look wrong**: PSX black-is-transparent bakes as
  alpha-0 with MASK (cutout) materials - correct for foliage and grates.
  Semi-transparent (ABE) prims - water sheets, light pools - split into a
  second `BLEND` material at half alpha (retail's dominant average blend
  mode); that also keeps the sea's stacked scroll layers from z-fighting,
  since blended materials skip depth writes. Additive/subtractive ABE
  modes flatten to the same alpha blend - nudge those few materials to
  Additive by hand where it matters.
- **Too big / too small**: re-export with a different `--scale`; the
  manifest records the scale used so the builder stays consistent.
- **A hand-placed NPC or prop is a giant**: current exports bake the
  scale onto each NPC/prop glb's root node
  (`conventions.npc_prop_units: "scaled"`), so a dragged-in file is
  world-sized as-is - re-export if yours predates that. On an older
  raw-PSX-units export, set `localScale = manifest scale` per instance
  (the builder detects the flag and does the right thing either way).
- **A scene looks empty**: world-map scenes (`deele1`…) have no MAN NPCs,
  and some scenes are cutscene-only shells; `export-glb --all-scenes`
  reports what each scene yielded.
- **Doors don't open / doorway teleports do nothing**: the `LegaiaDoorway`
  and `LegaiaDoor` behaviours need the VRChat worlds SDK (UdonSharp) in
  the project - without it the builder logs one warning per object and
  leaves the trigger inert. They also only react to *players*, so test in
  Build & Test / ClientSim, not by flying the editor scene camera through
  them. A door that opens but never teleports is the separate case of a
  scene-exit portal whose target scene isn't built in this Unity scene -
  the builder wires those only when `Legaia_<target>` exists.
- **Doors swing on a loop / teleports silently no-op after an older
  build**: both were builder defects - door tagging keyed on a join that
  missed every house door (fixed in the exporter: re-export the scene so
  the manifest's `is_door` flags are current), and Udon field values were
  set on the U# proxy without `CopyProxyToUdon`, so the backing behaviour
  kept null defaults. Rebuild with the current kit **and** a current
  manifest; the builder offers to replace a stale `Legaia_<scene>` root.
  Interior doors, cupboards and drawers looping their opening is the
  same symptom one layer deeper: those stand near no teleport or portal,
  so no proximity join can ever tag them - the manifest's per-prop
  `cyclic` flag is what routes them to the approach-open path, and it
  too needs a current manifest.
- **The windmill doesn't spin**: an older manifest judged `cyclic` by
  clip shape (last keyframe returns to the first), and the windmill's
  spin ends ~179° displaced - so it was mis-filed as a one-shot and
  frozen at frame 0. The flag is now the bind record's own retail
  verdict (an empty spawn pass keeps the actor's looping template
  flags; a door's reset-hold parks it) - re-export the scene and
  rebuild.
- **"Unable to find valid U# program asset associated with script"**:
  UdonSharp only auto-creates program assets for scripts made through its
  own Create menu, so the kit's bare `.cs` files have none and U# refuses
  to attach them (an older builder let that exception abort the whole
  build - nothing after the first door got wired). The builder now
  creates the missing `UdonSharpProgramAsset`s next to the scripts,
  resets U#'s lookup cache and compiles before wiring; each failed wire
  is also contained to its own object instead of ending the build.
- **The shoreline doesn't move**: the morph clip lives in the world glb -
  re-export with a current build (the manifest should carry `world_anim`),
  leave **Animate world morphs** on, and check the glTFast import kept
  animations enabled (the glb's import inspector, Animation tab).

## Faithful vs. approximated

The world geometry, textures, packet-colour shading, placement transforms,
floor heights, walk surface, doorway trigger/landing data, shoreline morph
deltas and the BGM render are the engine's own retail-parity kernels
(`engine-core::scene_assembly` and friends - the same code the site's
field-scene viewer renders). What is approximated: NPCs loop their spawn
clip instead of running their field-VM scripts, non-door props free-run
instead of waiting for script triggers, a door's full record choreography
collapses to open-on-approach + teleport, script-door arms are frozen at
the cold-entry story-flag state, and the shoreline's arming cadence is the
engine's scene-entry pulse enhancement. The card table's talk is new
writing keyed to the town (the manifest's dialog first line only supplies
a villager's *name* when it carries a speaker prefix); the retail MES
dialogue and shop counters remain the natural next Udon layer - the
manifest already carries each NPC's kind (`talk`/`door`/`prop`) and
dialog first line to seed it.

The realism foldout sits entirely on the *enhancement* side of this line:
lighting, sky, grass, shells, ambience and wander are deliberate
departures from retail, each its own toggle. Every pass ships enabled by
default - the project's ship-the-better-experience-by-default policy -
and unticking them all restores the faithful retail-shaded build, one
toggle away, same as the engine's own knobs. (The interior
shells are the one pass that *restores* retail framing: those rooms sit
against black space in the real game.)
