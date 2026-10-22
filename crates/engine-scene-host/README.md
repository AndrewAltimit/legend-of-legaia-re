# legaia-engine-scene-host

The scene host: the one owner of a running `World` and the disc index it
loads from. `engine-core` holds the `World` and everything that reads or
writes it; this crate is what a host drives that world through - enter a
scene, tick it, drain what the tick queued.

## What lives here

- **`scene_host`** - `SceneHost`, which owns the `World`, the `ProtIndex`,
  the loaded scene and its model bank, and:
  - enters field, world-map and battle scenes (`scene_entry`): resources,
    placed objects, walk-on triggers, the entry seat, the battle inputs the
    loader reads off the disc, the FMV hand-off;
  - seats a scene-picker entry on a door arrival into the picked scene
    (`arm_picker_entry_seat`, both hosts' picker), falling back to the
    cold-entry seat when no door leads in;
  - ticks a loaded scene and routes what the tick raised: door warps and
    minigame warps, dialog, BGM through the host's `BgmDirector`, sustained
    SFX, the battle stage and intro inputs;
  - registers the effect-model library in the global pool (`effects`) and
    builds the player's rig mesh (`player_rig`);
  - adds the field-overlay strings to the submode screens' line lists
    (`submode_lines`: the code lock, the flag window, the coin counter).
- **`resume`** - where a loaded save resumes and where a New Game starts:
  the scene-entry choreography both hosts run, and the card-load seam
  (`CardLoadHost`).
- **`scene_live`** - the headless live-scene preview the viewers animate: a
  scene entered through the real `SceneHost` and ticked with no pad.
- **`disc_readers`** - the PROT reads that feed parsed tables to kernels at
  scene entry (the battle tutorial script, the field MAN encounter table).
- **`fishing_venue`** - the fishing venue's floor and per-frame tick read off
  the host's world and loaded scene.
- **`place_name_banner`** - the MAN loader's place-name balloon
  (`FUN_8003AEB0`'s tail): the scene name off the MAN, the banner seated when
  `PLACE_NAME_BANNER_FLAG` is armed, the flag cleared, and the leading flag writes
  a scene-entry record makes. Its only callers are this crate's scene load
  and entry tick.

## The split line

A rule that reads or writes `World` without the loaded scene or the disc
index stays in `engine-core`: the tick, New Game seeding (`engine-core::new_game`), the
player-rig source (`World::player_rig_source`). What
this crate adds is the part only the scene's owner can do - reading the disc
at entry, swapping scenes, handing events to the host's audio. The `World`
fields it seeds at entry are public for that reason.

## Place in the layering

```text
engine-shell / web-play / web-viewer          hosts
engine-session, engine-screens, parity          host-shared top
engine-menu-runtime                             the pause menu over World
engine-scene-host                               SceneHost, resume, live preview
engine-core                                     World, the tick, battle loop
engine-scene, engine-* kernels                  World-free rules + scene loading
```

Nothing in `engine-core` names this crate. The hosts, `engine-session`,
`engine-capture`, `engine-screens`, `engine-menu-runtime` and `parity` hold a
`SceneHost`.

## Tests

`tests/*.rs` build as one `integration` binary; select a file by module path
(`cargo test -p legaia-engine-scene-host --test integration foo::`). These
are the engine's scene-level chains, a scene entered and ticked off the disc,
so most of them read a disc and skip and pass when `LEGAIA_DISC_BIN` is
unset.

## Where the behaviour is documented

| Topic | Page |
|---|---|
| Architecture, frame model, hosts | [`engine.md`](../../docs/subsystems/engine.md) |
| Scene bundles and the asset loader | [`scene-bundles.md`](../../docs/formats/scene-bundles.md), [`asset-loader.md`](../../docs/subsystems/asset-loader.md) |
| BGM routing and the cold scene entry | [`audio.md`](../../docs/subsystems/audio.md) |
| Cutscenes and the FMV hand-off | [`cutscene.md`](../../docs/subsystems/cutscene.md) |
| Battle entry from the field | [`battle-round-loop.md`](../../docs/subsystems/battle-round-loop.md) |
