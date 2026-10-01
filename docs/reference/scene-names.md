# Scene names

Every scene on the disc is addressed by a `CDNAME.TXT` label - `town01`,
`balden`, `vozz` - and nothing about the label says where the player is. This
page covers the one table that names them: what each column means, where the
names come from, and the scenes whose identity is not obvious from the label.

The table is [`data/gamedata/scenes.toml`](../../data/gamedata/scenes.toml),
one row per CDNAME block in disc order. It is compiled into
`legaia_gamedata::Database::scene_names` / `scene_by_id`, and `site/_gen.py`
reads the same file to emit `scenes.json`, which the play page's scene picker,
the asset viewer's scene filter, the world and NPC pages and the
[scene-names site page](https://andrewaltimit.github.io/legend-of-legaia-re/reference/scene-names.html)
consume. There is no second copy of the names anywhere.

## Contents

- [Columns](#columns)
- [Where a name comes from](#where-a-name-comes-from)
- [Scenes the label misleads about](#scenes-the-label-misleads-about)
- [Where the contributor's reading and the disc differ](#where-the-contributors-reading-and-the-disc-differ)
- [The slots that crash when loaded](#the-slots-that-crash-when-loaded)
- [Tools](#tools)
- [Provenance](#provenance)

## Columns

| Column | Meaning |
|---|---|
| `id` | The CDNAME label every tool and the engine address the scene by. |
| `cdname` | The `#define` number: a raw in-RAM PROT-TOC index. The block's first extraction entry is `cdname - 2` ([`cdname.md`](../formats/cdname.md#numbering-space)). |
| `category` | `town` / `field` / `world_map` / `cutscene` / `battle` / `audio` / `system` - drives the viewers' filters. |
| `name` | The display name every consumer shows. |
| `banner` | The scene MAN's section-2 name, exactly as the disc spells it. Omitted where the disc carries none. |
| `contributor` | Stann0x's in-game reading of the scene, where one was given. |
| `note` | Why the name is what it is, when that is not obvious. |

## Where a name comes from

The ground truth is the scene's own **banner**: MAN section 2, the name the
loader puts on screen when the player walks in and the one the save screen
writes into a slot ([`place-names.md`](../formats/place-names.md#site-3---the-scene-display-name)).
`crates/patcher/tests/place_names_real.rs`
(`scene_name_table_banners_match_the_disc`) holds every `banner` in the table to
the disc and fails on any scene whose disc banner the table leaves out.

`name` keeps the banner's spelling. Where several scenes share one banner - the
Sol Tower floors, the Bio Castle interiors, Rim Elm's story-state variants -
the name adds a qualifier, so no two playable scenes read the same.

Where a scene has no readable banner (an empty section 2, an untranslated
Shift-JIS one on the `ed*` endings, or no MAN), the name rests on the scene's
contents - its dialogue, its shops, its doors - and the note says what pinned
it. The other two place-name carriers, the quick-travel cells and the world-map
labels, supply the spelling for those (`Usha Research Center`,
`Hunter's Spring`).

## Scenes the label misleads about

The label is a developer abbreviation, often of the Japanese name, and several
read as a different place:

| Scene | Is | Pinned by |
|---|---|---|
| `vell` | West Voz Forest | banner; the Bridge Grass beat |
| `station` | Octam Station | no banner; fare board "Octam Station - Karisto Station", the Sebucus guidebook |
| `station3` | Karisto Station | banner |
| `ropeway`, `ropeway2` | Octam (surface, after the Mist) | banner on `ropeway`; the returned-from-underground townsfolk and inn in both |
| `rayman`, `rayman2` | Octam's underground city | banner "Octam (Underground)" on `rayman2`; Hari in both |
| `taiku`, `taiku2` | Zora's Floating Castle | banner on `taiku2`; the throne-room lever puzzle in both |
| `chitei2` | Jette's Fortress | banner; the Rapid Transport System switches and Jette's "This is my castle" |
| `deroa` | Jette's Fortress entrance | banner; dialogue calls it the Absolute Fortress |
| `dolk`, `dolk2` | Drake Castle (with / after the Mist) | banner on `dolk`; `map01` picks one on flag `0x142` |
| `suimon` | Water Gate | banner |
| `jiji` | Ancient Wind Cave | banner |
| `garmel` | Zeto's Dungeon | banner |
| `bubu1` / `bubu2` | Buma (thawed) / Buma (frozen) | both bannered Buma; `bubu2`'s doors are frozen shut, `bubu1` carries the shops |
| `conc`, `conc2`, `conc3` | Conkram in the past | banner "Conkram (Past)" on all three |
| `concnow` | Conkram in the present | banner "Conkram" |
| `jou` | Rim Elm under the Juggernaut | banner "Rim Elm"; its door leads into `jouina` |
| `jouina`..`jouine`, `juui1`, `juui2` | Bio Castle (inside the Juggernaut) | banner on the `jouin*` five; the Ra-Seru farewell in `juui*` |
| `kor*`, `koin*`, `korout` | Sol Tower | banner on all thirteen; `koin1` hosts the Muscle Dome and Baka Fighter doors, `koin3` the dance |
| `edbubu` | Buma's ending | the label; `eddoman` is Usha's |

## Where the contributor's reading and the disc differ

Both are kept, the disc wins the `name` column. They differ only in spelling
or in which of two in-game names is used:

- `bylon`: *Byron* in the reading, **Biron Monastery** everywhere on the disc.
- `stone`: *Gate of Shadow(s)* in dialogue, **Shadow Gate** in the banner and
  the world-map label.
- `keikoku`: *Valleys of Mist* in the reading, the banner is just **Ravine**.
- `tower` / `teien`: *Jeremi Tower / Garden* in the reading, **Sky Gardens
  Tower / Sky Gardens** on the disc.
- `tunnela` / `tunnelb` / `tunnelc`: *Underground Path* in the reading,
  **Ancient Path** / **Fire Path** in the banners.
- `dohaty`: *Sebucus Mist Generator* in the reading - the generator is inside
  **Dohati's Castle**, which is the banner.
- `deroa`: *Absolute Fortress* in the reading and in dialogue, **Jette's
  Fortress** in the banner.
- `chitei2`: the reading places it in the Floating Castle; the banner and
  Jette's own line put it in **Jette's Fortress**.
- `ropeway2`: the reading follows the label (*Rope Way*); the scene is surface
  **Octam** again.
- `conc2`: the reading guessed the opening; the banner is **Conkram (Past)**
  and the scene is past Conkram with the underground-lab stairs guarded.

## The slots that crash when loaded

Six blocks cannot be entered as field scenes, for three different reasons:

- **`opkorout`, `opmap01`** - every entry of both blocks is a single 2048-byte
  [pochi](../formats/pochi.md) placeholder sector: no map, no asset bundle, no
  MAN. A loader pointed at them decompresses filler.
- **`other4`, `other5`, `other6`** - minigame data the minigame overlays load by
  path: the slot machine's scene and bank, the Baka Fighter's battle-form party
  pack and atlases, the Muscle Dome's ringside stills
  ([`ringside-still.md`](../formats/ringside-still.md)). None carries a
  walkability map, so none is a scene.
- **`other7`** - scene-shaped (map, trigger sidecar, bundle), but a developer
  leftover: an earlier revision of `koin3`'s MAN that nothing in the retail
  game loads ([`cdname.md`](../formats/cdname.md)).

`other1` is the fishing pond's field and does load.

## Tools

- `gamedata-tool list scenes` / `gamedata-tool dump-json scenes` - the table,
  no disc needed.
- `legaia-patcher locations --input <disc>` - every place name the disc
  carries, ending with each scene's banner beside its CDNAME id: the check to
  run against a patched or translated disc.

## Provenance

Scene identifications by **Henrique Stanke Scandelari (Stann0x,
[github.com/Stann0xus](https://github.com/Stann0xus))**, played through in
retail and contributed as a reference, the same way as the
[music-track table](music-tracks.md). Each row is cross-checked against the
disc's own name carriers and the scene's contents before it lands in `name`.
Only short factual labels are committed; no game text beyond place names.
