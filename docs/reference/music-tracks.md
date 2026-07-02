# Music-track disambiguation

A cross-reference of every in-game music track across the four naming spaces
that exist for it. A single Legaia BGM cue has, in practice, four different
names depending on where you read it, and they do not line up one-to-one:

1. **Debug sound-test ID** - the internal asset/track identifier the game's
   debug sound test exposes (`M01`, `M14A`, `M26B1`, plus a few non-`M`
   labels like `ALNDRA`, `JBGM`, `KANASIMI`). This is the handle closest to
   the on-disc track order.
2. **Debug sound-test title** - the short Japanese working title shown beside
   that ID in the debug menu (e.g. `霧のあるフィールド`, "field with mist").
   These describe the *scene the track was written for*, not its mood.
3. **In-game context** - where the track is actually heard during play
   (often broader or different from the debug title - a track written for one
   scene gets reused elsewhere).
4. **Official OST title** - the title the published soundtrack album gives the
   track, where one exists (many debug entries have no OST counterpart).

This table is the label layer for the SEQ/BGM tracks the preservation tools
extract and the engine resolves. BGM lookup in retail is PROT-relative (there
is no literal track-name table in the executable - see
[`audio.md`](../subsystems/audio.md) and [`seq.md`](../formats/seq.md)), so a
human-readable map between the debug IDs and where each cue plays is the
ground-truth labelling that the curated-data tables provide for items and
enemies.

## Provenance + confidence

Compiled by **Henrique Stanke Scandelari (Stann0x,
[github.com/Stann0xus](https://github.com/Stann0xus))** and contributed to this
project as a reference. The romaji, English glosses, and the proposed
relocalization titles are his analysis; the debug-mode IDs/titles and OST
titles are factual short labels transcribed from the game's own debug sound
test and the published soundtrack listing.

Treated here exactly like the other curated reference tables
([`gamedata.md`](gamedata.md)): only the *factual* labels are committed - track
IDs, the short scene-name titles, and where each cue plays - as ground-truth
labels for the binary SEQ/VAB records under reverse engineering. No Sony-owned
bytes (sequence data, sample data, executable) are reproduced; the audio itself
is decoded at runtime from the user's own disc.

Confidence is **Inferred** unless a track has been matched to its OST entry, in
which case the OST columns are **Confirmed** against the published listing. The
last three rows (highlighted in the source as uncertain) are flagged inline.

## The disc-side join: the `music_01` bank IS the sound-test order

The sound-test index (`#` below) attaches structurally to the disc: the
`music_01` CDNAME block (extraction entries `990..=1071`, raw TOC `992..`,
82 slots) is the global BGM bank, and **bank slot `i` = sound-test index
`i`**. A field-VM op-`0x35` BGM id `>= 2000` resolves through the global
pool (`FUN_800243F0`, see [script-vm § BGM lookup](../subsystems/script-vm.md#bgm-lookup-table)),
so **global id `2000 + i` plays sound-test track `i`**. Pinned by:

- **Per-scene op-`0x35` census** (`engine-core/tests/scene_bgm_labels_disc.rs`):
  walking every scene MAN's scripts and joining each started id to this
  table lands the right label across the corpus - `town01` starts `2016` =
  `M14B` "Rim Elm theme", the three kingdom maps start `2000`/`2001` =
  `M01`/`M02` overworld pair, `bylon` starts `2019` = `M17` "Byron
  Monastery", Sol's floors start `M16` casino / `M23` bar / `M100` Sol /
  `M112` disco, `geremi` starts `2047` = `M102` "Jeremi", the Bio-Castle
  interiors start `2013` = `M13`.
- **Pochi alignment**: the bank's four placeholder-filled slots (extraction
  `1066..=1069`) land exactly on the four dev-leftover rows (#76..=79 - the
  M13 flute, `M117`, `MPIANO`, `LEVELUP`), removed from the retail NA disc
  but still holding their sound-test slots.
- **Bank copies**: the battle themes (#26..) and title theme (#65)
  byte-match their `sound_data2` boot/battle-bank copies (extraction
  879..884), the banks those cues actually stream from.

Slot 81 (extraction `1071`) is a spare past the last sound-test row. Scenes
carry **no local SEQ data** - every SEQ stream on the disc lives in this
bank + the `sound_data2` banks (+ the `monster_test` dev bank and
`teien`'s scene-local copy of `M01`) - so the scene-local id space
(`< 2000`) is the rare exception. The engine's
resolver is `legaia_engine_core::music_labels` (`label_for_bgm_id` /
`label_for_prot_entry`); the play-window HUD names the playing track and
the asset-viewer `seq` command names a bank slot's file.

## Notable entries

- **Borrowed placeholder cues.** `ALNDRA` (#72) is *Alundra*'s Zazzan battle
  theme and `JBGM` (#73) is the *Wild Arms* battle theme - both other
  Contrail/Sony titles, present on the Legaia disc as dev placeholders.
- **Dev test files.** `DUMMY` (#74, debug title "I'M A DUMMY-DAYO!"), `PIANO`
  (#78), `LEVELUP` (#79), and `A` (#80) are test/placeholder entries, not
  shipping music.
- **Reused themes under different IDs.** Several debug IDs share a title
  (`中ボス` / "medium boss" appears for `M27`, `M108`, `M33`; `ディスコ` /
  "disco" for `M112`, `M120`, `M117`) - the same musical idea staged for
  different scenes.

## Track table

`#` is the debug sound-test order. `-` marks a column the source leaves blank.
"OST gloss" is the English reading of the OST title; "Relocalization" is
Stann0x's proposed English title.

| # | ID | Debug title (JP) | Debug romaji | Debug gloss | In-game context | OST title (JP) | OST romaji | OST gloss | Relocalization |
|---|---|---|---|---|---|---|---|---|---|
| 0 | M01 | 霧のあるフィールド | KIRI NO ARU FĪRUDO | Field with mist | Overworld with mist | 霧の荒野 | KIRI NO KŌYA | The misty wasteland | The Misty Barren Fields |
| 1 | M02 | 霧の消えたフィールド | KIRI NO KIETA FĪRUDO | Field where the mist disappeared | Overworld with no mist | よろこびの大地 | YOROKOBI NO DAICHI | The land of joy | The Land of Joy |
| 2 | M03 | 創世樹復活後のダンジョン | SŌSEIJU FUKKATSU-GO NO DANJON | Dungeon after Genesis Tree revival | Mt. Rikuroa after tree is revived | 風, 樹, 水 | KAZE, KI, SUI | Wind, tree, water | Winds, Trees and Water |
| 3 | M04 | 森のダンジョンA | MORI NO DANJON A | Forest dungeon A/B | Underground path to Octam | - | - | - | Path of Secrets |
| 4 | M04B | 森のダンジョンB | MORI NO DANJON B | Forest of mystery | East Voz Forest | 神秘の森 | SHINPI NO MORI | The mysterious forest | Forest of Mystery |
| 5 | M05 | 霧のダンジョン | KIRI NO DANJON | Misty dungeon | Town/dungeon in mist | - | - | - | The Misty Town |
| 6 | M06 | リクロア山霧 | RIKUROA YAMA KIRI | Rikuroa mountain fog | Mt. Rikuroa/Mt. Dhini in the mist | - | - | - | The Misty Mountains |
| 7 | M07 | ガーメル城 | GĀMERU-JŌ | Garmel Castle | Fire path/Xain's nest | 王国騎士団 | ŌKOKUKISHI-DAN | The royal knights | The Fire Fortress |
| 8 | M08 | 遺跡ダンジョン | ISEKI DANJON | Ruin dungeon | Uru Mais | 遺跡の呼び声 | ISEKI NO YOBIGOE | The call of the ruins | Call of the Ruins |
| 9 | M09 | 獣衣界への道 | JŪI-KAI E NO MICHI | The path to the beast-garment world | Field with Juggernaut | 闇の鼓動 | YAMI NO KODŌ | The pulse of darkness | Heartbeat of Darkness |
| 10 | M10 | 霧の発生 | KIRI NO HASSEI | Mist outbreak | Drake/Vidna in mist | 霧 | KIRI | Mist | The Mist |
| 11 | M11 | 凶獣界 | KYŌJŪ-KAI | Evil beast realm | Rogue's dungeon | - | - | - | The Profane Realm |
| 12 | M12 | 聖獣界 | SEIJŪ-KAI | Holy beast realm | Octam's station | - | - | - | The Sacred Realm |
| 13 | M13 | 生物城 | SEIBUTSU-JŌ | Live creature castle | Bio-Castle dungeon | 生物城 | SEIBUTSU-JŌ | The creature castle | Bio-Castle |
| 14 | M14 | - | - | - | M14A with sustained note | - | - | - | - |
| 15 | M14A | リムエルム夕方 | RIMU-ERUMU YŪGATA | Rim Elm evening | Juno's death/Rim Elm sunrise | 夕映えのリムエルム | YŪBAE NO RIMU-ERUMU | Rim Elm in the sunset glow | Rim Elm's Evening Glow |
| 16 | M14B | リムエルム昼 | RIMU-ERUMU HIRU | Rim Elm noon | Rim Elm theme | リムエルム | RIMUERUMU | Rim Elm | Rim Elm's Sunshine |
| 17 | M15 | 吹きだまりの洞窟 | FUKIDAMARI NO DŌKUTSU | Snow-drift cave | Snowdrift Cave | ぬくもりの洞窟 | NUKUMORI NO DŌKUTSU | The cavern of warmth | Cave of Warmth |
| 18 | M16 | サブゲーム | SABUGĒMU | Sub-game | Sol casino | ワイルド宣言 | WAIRUDO SENGEN | Wild declaration | Wild Declaration |
| 19 | M17 | バイロン寺院 | BAIRON JIIN | Byron Temple | Byron Monastery | 我らバイロン僧兵団 | WARERA BAIRON SŌHEI-DAN | We are the Byron monk warriors | We, the Byron Monk Warriors |
| 20 | M18 | コル上層 | KORU JŌSŌ | Top of the valley | Seru Kai | - | NOARU | Noaru | Valley of Noaru |
| 21 | M20 | 復活したドルク城 | FUKKATSU SHITA DORUKU-JŌ | Dolk Castle revived | Drake Castle after mist | にぎやかな王宮 | NIGIYAKANA ŌKYŪ | The bustling royal palace | Lively Royal Castle |
| 22 | M21 | ドーマン博士の研究所 | DŌMAN HAKASE NO KENKYŪJO | Doctor Doman's laboratory | Dr. Usha's research lab | - | - | - | Dr. Usha's Research Institute |
| 23 | M22 | みんなが踊ってる部屋 | MIN'NA GA ODOTTERU HEYA | The room where everyone dances | Mei's theme | メイ | MEI | Mei | Where Everyone Dances |
| 24 | M23 | 酒場 | SAKABA | Bar | Sol's bar | - | - | - | Where Everyone Drinks |
| 25 | M25 | 創世樹周辺の曲 | SŌSEIJU SHŪHEN NO KYOKU | Music about the Genesis Tree | Genesis Tree theme | 静かなる創世樹 | SHIZUKANARU SŌSEIJU | The silent Genesis Tree | The Quiet Genesis Tree |
| 26 | M26B1 | 雑魚戦闘バージョン1 | ZAKO SENTŌ BĀJON 1 | Minor enemy battle version 1 | Battle theme 1 | - | - | Brand of the Holy Knuckles | Brand of the Holy Knuckles |
| 27 | M26B2 | 雑魚戦闘バージョン2 | ZAKO SENTŌ BĀJON 2 | Minor enemy battle version 2 | Battle theme 2 | - | - | - | - |
| 28 | M27 | 中ボス | CHŪ BOSU | Medium boss | Boss theme 1 | 襲撃 | SHŪGEKI | Assault | Attack! |
| 29 | M28 | 霧の巣ボス戦 | KIRI NO SU BOSU SEN | Mist nest boss battle | Koru battle theme | - | - | - | The Cold Koru |
| 30 | M29 | ソンギ戦闘 | SONGI SENTŌ | Battle with Songi | Songi battle | 我が名はソンギ | WAGA NA WA SONGI | My name is Songi | I Am Songi! |
| 31 | M30 | コート戦1 | KŌTO-SEN 1 | Cort battle 1 | Cort battle 1 | - | - | - | Cort's War |
| 32 | M32 | 中ボス序曲 | CHŪ BOSU JOKYOKU | Medium boss overture | Boss theme 2 | 霧の使徒 | KIRI NO SHITO | Apostles of the mist | Apostles of the Mist |
| 33 | M34 | ソンギ出現序曲 | SONGI SHUTSUGEN JOKYOKU | Songi appearance overture | Songi theme | ソンギ推参 | SONGI SUIZAN | Songi's arrival | Songi's Overture |
| 34 | M35 | ジャガーノート襲撃2、3用 | JAGĀNŌTO SHŪGEKI 2, 3 YŌ | For Juggernaut attack 2 and 3 | Juggernaut appears | - | - | - | Juggernaut's Attack |
| 35 | M36 | 創世樹復活イベント | SŌSEIJU FUKKATSU IBENTO | Genesis Tree revival event | Genesis Tree resurrection | 創世樹の目覚め | SŌSEIJU NO MEZAME | The awakening of the Genesis Tree | Awakening the Genesis Tree |
| 36 | M37 | 神秘的イベント2 | SHINPITEKI IBENTO 2 | Mystical event 2 | Ra-Seru speaks/Hari | - | - | - | Mystical Happening |
| 37 | M38 | 風来獣車 | FŪRAISHŪ-SHA | Wandering beast-vehicle | Gondola flying train theme | 風来獣車 | FŪRAISHŪ-SHA | The wandering beast-vehicle | The Flying Train |
| 38 | M39 | ソーン族の空中飛行 | SŌN-ZOKU NO KŪCHŪ HIKŌ | Aerial flight of the Soren tribe | Soren flight | - | - | - | The Flying Soren Tribe |
| 39 | M40 | 予期せぬ出来事 | YOKI SENU DEKIGOTO | Unpredicted event | Byron under attack/tense theme | 予期せぬ出来事 | YOKISENU DEKIGOTO | The unforeseen event | Unforeseen Event |
| 40 | M41 | 神秘的イベント1 | SHINPITEKI IBENTO 1 | Mystical event 1 | Noa's dreams | - | - | - | Mystical Dreaming |
| 41 | M42 | 悲しいイベント全般 | KANASHĪ IBENTO ZENPAN | Sad events in general | Conkram/sad events | 霧の都 | KIRI NO MIYAKO | The mist-capital | The Misty Capitol |
| 42 | M47 | 脱出イベント | DASSHUTSU IBENTO | Escape event | Floating castle escape | - | - | Hurry up | Hurry Up! |
| 43 | M48 | スーパースペクタル | SŪPĀ SUPEKUTARU | Super spectacle | Mist dungeons | 霧の巣 | KIRI NO SU | The nest of mist | Misty Nest |
| 44 | M49 | 人間の世界 | NINGEN NO SEKAI | Human world | Opening title pt 1 (incomplete?) | - | - | - | - |
| 45 | M100 | イベント未定 | IBENTO MITEI | Undecided event | Sol | - | - | - | The Sol Tower |
| 46 | M101 | バン夢のシーン | BAN YUME NO SHĪN | Vahn dream scene | Vahn dream scene | - | - | - | Vahn Dreams |
| 47 | M102 | 町の曲 | MACHI NO KYOKU | Town song | Jeremi | 街の灯 | MACHI NO AKARI | The town's light | Light of the Town |
| 48 | M104 | バイロン寺院で宴の曲 | BAIRON JĪN DE UTAGE NO KYOKU | Music for a banquet at Byron Temple | Party in Byron Monastery | - | - | - | Byron Party |
| 49 | M106 | 思い出の曲 | OMOIDE NO KYOKU | Music of memories | Nostalgic theme | 思い出のメロディー | OMOIDE NO MERODĪ | Melody of memories | The Melody of Memories |
| 50 | M107 | 前向きな汎用イベント | MAEMUKI NA HANYŌ IBENTO | Positive generic event | Noa joins the party | - | - | - | Positive Energy! |
| 51 | M108 | 中ボス | CHŪ BOSU | Medium boss | Koru battle theme 2 | - | - | - | The Hot Heat |
| 52 | M109 | 不気味な気配 | BUKIMI NA KEHAI | Eerie presence | Tense moment | - | - | - | Ominous Presence |
| 53 | M110 | 中ボス出現序曲 | CHŪ BOSU SHUTSUGEN JOKYOKU | Medium boss appearance overture | Boss overture/Vahn-Saryu | - | - | - | Lords of the Mist |
| 54 | M111 | ニルボア | NIRUBOA | Nivora | Nivora Ravine | 静かなる破滅 | SHIZUKANARU HAMETSU | The silent ruin | Silent Doom |
| 55 | M112 | ディスコ | DISUKO | Disco | Sol disco fever | - | - | - | Sol's Disco Fever! |
| 56 | M113 | スーパースペクタル2 | SŪPĀ SUPEKUTARU 2 | Super spectacle 2 | Mist generator dungeon 2 | - | - | - | Misty Hideout |
| 57 | KANASIMI | 悲しみイベント | KANASHIMI IBENTO | Sorrowful event | Cara's theme/Alundra requiem | - | - | - | Requiem |
| 58 | M114 | 普通の街2 | FUTSŪ NO MACHI 2 | Ordinary town 2 | Vidna | - | - | - | Town of the Wind |
| 59 | M115 | ディスコ予選 | DISUKO YOSEN | Disco preliminaries | Sol disco preliminary | - | - | - | Funky Ascent |
| 60 | M116 | ディスコ決勝 | DISUKO KESSHŌ | Disco finals | Sol disco final 1 | - | - | - | Tower Boogie |
| 61 | M31 | コート戦2 | KŌTO-SEN 2 | Cort battle 2 | Juggernaut final battle | - | - | - | Apocalypse |
| 62 | M105 | 霧の谷の曲 | KIRI NO TANI NO KYOKU | Mist valley song | Buma/misty valley | - | - | - | The Misty Valley |
| 63 | M118 | オープニングキャラ劇1 | ŌPUNINGU KYARA GEKI 1 | Opening character act 1 | Opening title pt 1 | プロローグ | PURORŌGU | Prologue | Prologue |
| 64 | M119 | オープニングキャラ劇2 | ŌPUNINGU KYARA GEKI 2 | Opening character act 2 | Opening title pt 2 | - | - | - | Henchmen of the Mist |
| 65 | M65 | タイトル画面 | TAITORU GAMEN | Title screen | Title screen theme | タイトル | TAITORU | Title | Legend of Legaia |
| 66 | M120 | ディスコ決勝 | DISUKO KESSHŌ | Disco finals | Sol disco final 2 | - | - | - | Disco Hero |
| 67 | M33 | 中ボス | CHŪ BOSU | Medium boss | Vahn-Saryu | - | - | - | Tense Presence |
| 68 | M50 | エンディング | ENDINGU | Ending | Credits theme | エンドタイトル | ENDO TAITORU | End title | Song of the Genesis Tree |
| 69 | M121 | 生物城崩壊 | SEIBUTSU-JŌ HŌKAI | Creature castle collapse | Bio-Castle collapse | 生物城の崩壊 | SEIBUTSU-JŌ NO HŌKAI | The collapse of the creature castle | The Collapse of the Bio-Castle |
| 70 | M122 | 祝福のリムエルム | SHUKUFUKU NO RIMU-ERUMU | Blessed Rim-Elm | Rim Elm ending | 人間の世界 | NINGEN NO SEKAI | The human world | The Blessed Human World |
| 71 | M123 | コート序曲 | KŌTO JOKYOKU | Cort overture | Bio-Castle | - | - | - | The Rise of the Bio-Castle |
| 72 | ALNDRA | アランドラ | ARANDORA | Alundra | Alundra Zazzan's battle theme | - | - | - | Excitement |
| 73 | JBGM | ワイルドアームズ | WAIRUDO ĀMUZU | Wild Arms | Wild Arms battle theme | - | - | - | Wild Arms Battle Theme |
| 74 | - | - | DAMĪ | Dummy | Dummy test music file | - | - | - | "I'm a Dummy-dayo!" |
| 75 | M47B | スーパースペクタクルB | SŪPĀ SUPEKUTAKURU B | Super spectacle B | Another flying castle escape | - | - | - | Hurry Up! Faster! |
| 76 | M13 | - | - | No name file | Soren flute/Soren theme | - | - | - | The Melody of Memories (Piano) |
| 77 | M117 | ディスコ | DISUKO | Disco | Sol disco fever 2 | - | - | - | Disco Legend |
| 78 | MPIANO | ピアノ | PIANO | Piano | Piano? *(uncertain)* | - | - | - | - |
| 79 | 獸衣 | - | LEVELUP | Levelup | Level up? *(uncertain)* | - | - | - | - |
| 80 | M26A | A | A | A | A? Another test file maybe? *(uncertain)* | - | - | - | - |

## See also

- [`subsystems/audio.md`](../subsystems/audio.md) - the BGM director, SsAPI
  sequencer, and per-scene BGM lookup that play these tracks.
- [`formats/seq.md`](../formats/seq.md) - the SEQ sequenced-music format the
  tracks are stored in.
- [`reference/gamedata.md`](gamedata.md) - the sibling curated-label tables
  (items, enemies, arts) this follows the pattern of.
