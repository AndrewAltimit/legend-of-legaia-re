# legaia-sound-formats

The sound containers a PROT entry can be. These modules find and bound the
banks inside an entry; decoding a bank or a sequence is
[`legaia-vab`](../vab/README.md) / [`legaia-seq`](../seq/README.md).

| Module | Covers |
|---|---|
| `bse_bank` | `bse.dat`, the battle occupant of the runtime SFX descriptor bank. See [`bse-dat.md`](../../docs/formats/bse-dat.md). |
| `vab_multi_bank` | `monster.snd`, the multi-bank VAB archive. |
| `sound_pack` | The `sound_data2` / `.dpk` VAB + SEQ bundle. See [`sound-driver.md`](../../docs/formats/sound-driver.md). |
| `scene_vab_stream` | The VAB-prefixed scene stream detector. See [`vab.md`](../../docs/formats/vab.md). |

## Layering

Sits on [`legaia-container-formats`](../container-formats/README.md) for the
DATA_FIELD chunk walk, plus `legaia-bytes`. It must not depend on the scene,
UI, minigame or battle-model format crates, nor on `legaia-disc-audit`.
`legaia-asset` re-exports each module at its old path.
