# legaia-minigame-formats

The minigames' disc data. The rules engines that consume it live in
[`legaia-engine-minigames`](../engine-minigames/README.md) and
[`legaia-engine-fishing`](../engine-fishing/README.md).

| Minigame | Modules |
|---|---|
| Fishing | `fishing_species`, `fishing_exchange`, `fishing_sprites`, `fishing_captions`. See [`minigame-fishing.md`](../../docs/subsystems/minigame-fishing.md). |
| Slot machine | `slot_payout`, `minigame_slot_scene` (the 3D scene). See [`minigame-slot-machine.md`](../../docs/subsystems/minigame-slot-machine.md). |
| Dance | `dance_chart`, `dance_cast`, `dance_art`. See [`minigame-dance.md`](../../docs/subsystems/minigame-dance.md). |
| Baka Fighter | `baka_opponents`. See [`minigame-baka-fighter.md`](../../docs/subsystems/minigame-baka-fighter.md). |
| Muscle Dome | `muscle_dome`, `ringside_still` ([`ringside-still.md`](../../docs/formats/ringside-still.md)). See [`minigame-muscle-dome.md`](../../docs/subsystems/minigame-muscle-dome.md). |
| Shared | `minigame_art` (the art packs the overlays upload), `minigame_sfx` (the runtime cue bank). |

## Layering

Sits on [`legaia-container-formats`](../container-formats/README.md) and the
leaf codecs (`legaia-lzs`, `legaia-prot`, `legaia-tim`, `legaia-tmd`,
`legaia-vab`). It must not depend on the scene, UI or battle-model format
crates, nor on `legaia-disc-audit`. `legaia-asset` re-exports each module at
its old path.
