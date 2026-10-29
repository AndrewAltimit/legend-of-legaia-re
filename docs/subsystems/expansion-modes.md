# Expansion game modes

The port carries game modes retail never had: Texas Hold'em, a trading-card game, mini-golf, checkpoint racing and prop hunt. Their rules live in `legaia_engine_modes` (`crates/engine-modes`), a `World`-free crate; an expansion pack picks the numbers and layouts in its `modes.toml` and `cards.toml` (`legaia_expansion_format::modes` / `::cards`, format reference in the expansion-pack format page). Nothing here touches retail state, so a disc with no pack loaded plays bit-identically.

This page covers the session contract every mode keeps, why it is shaped for lockstep netplay, and each mode's rules.

## At a glance

| | |
|---|---|
| Crate | `crates/engine-modes` (`legaia_engine_modes`), depends on `engine-system` (pad layout) and `expansion-format` (pack schema) |
| Contract | `session::ModeSession`: `tick(&[PadWord]) -> Vec<Event>`, `view()`, `state_hash()`, `is_finished()`, `results()` |
| Math | `fixed`: 20.12 fixed point, 12-bit angles, table sine, integer `atan2` and square root |
| RNG | `session::ModeRng` (SplitMix64), one per session, seeded from the config |
| Modes | `poker`, `cards` + `cards::duel` (and the economy: `cards::economy`, `cards::screen`, `cards::trade`), `golf`, `racing`, `prop_hunt` |
| Pack schema | `[[poker]]`, `[[card_duels]]`, `[[golf]]`, `[[races]]`, `[[prop_hunt]]` in `modes.toml`; `[[cards]]`, `starter`, `[[boosters]]`, `[[dealers]]`, `[[rewards]]` in `cards.toml` |
| Example pack | `packs/rebirth/modes.toml`, `packs/rebirth/cards.toml` |
| Built-in set | `crates/engine-modes/builtin/` - one practice instance per mode, offered with no pack |
| Host seat | `legaia_engine_expansion::modes::PackModeSeat` on `World::expansion`, under `SceneMode::Expansion` |
| Screens | `legaia_engine_ui::expansion_modes`, projected by `legaia_engine_screens::expansion_modes` |
| Tests | per-module rule tests; `crates/engine-modes/tests/` determinism, random-pad soak and Rebirth conversion |

## The session contract

A mode session is a pure function of three things: its config (including the seed), the order of its ticks, and the pad words fed to each tick. `tick` takes one raw pad word per seat (`PadButton` layout); a short slice reads as released pads. A seat is a human or an AI of a tier (`Novice`, `Regular`, `Expert`); an AI seat ignores its pad word and decides from the session's own RNG, so a missing player becomes an AI seat without changing the input shape.

A host draws only from `view()`, an owned snapshot, and changes the session only through `tick` (each mode also exposes its rule entry points, such as `PokerSession::act` or `GolfSession::strike`, which the pad path itself ends in; tests and alternative hosts use them directly). When `is_finished()` turns true, `results()` gives placements (1-based, ties sharing a place) and the rewards each seat takes away: a currency change, a booster pack or a card, all by pack id. Granting them to a save is the host's job.

### Why lockstep holds

Lockstep netplay sends only pad words between peers; every peer runs the full session and must arrive at the same state after every tick. The kernels hold to the rules that make that true:

- **No floats in state.** Positions, velocities and speeds are 20.12 fixed point (`fixed::Fx`). `f64::sin` and `f64::sqrt` may round differently across libm builds, CPUs and wasm, so the trig is a literal quarter-wave sine table with linear interpolation, `atan2` is a binary search over that table, and square roots are integer Newton. Widening to `i64` / `i128` replaces any product that could overflow.
- **No ambient randomness.** Each session owns a `ModeRng`; AI decisions, deck shuffles, Monte Carlo equity samples and booster rolls all draw from it. No clock, no global generator, no host state.
- **No unordered iteration.** State is `Vec` and `BTreeMap`, never a hash map.
- **A checkable digest.** `state_hash()` is FNV-1a 64 over every field the rules read, written field by field (not `std`'s `DefaultHasher`, whose algorithm is not stable across Rust releases). Peers compare it to detect a desync; the determinism tests compare it frame by frame.

Hidden information (poker hole cards) is part of every peer's state under lockstep; a host shows each player only their own seat's cards. A deployment that needs hidden information to stay hidden needs an authoritative server instead, which the same session supports by running it on the server and sending views.

Prop hunt is geometry-agnostic: its built-in `Arena` (bounds and rectangle blockers) is enough to play and test the rules, and a host with real scene collision feeds positions through `PropHuntSession::tick_with_positions`, which makes those positions part of the lockstep input.

## Poker

No-limit Texas Hold'em, 2 to 6 seats. Every seat buys in for the table's chips; a pack's `chip_value` and `currency` (gold or casino coins) turn the final stack into the currency reward, `(chips - buy_in) * chip_value`.

- **Blinds.** The seat after the button posts the small blind and the next the big blind. Heads-up, the button posts the small blind, acts first preflop and last afterwards. `blind_double_every` doubles both every N hands.
- **Betting.** Preflop the current bet is the big blind even when the big blind is all-in for less, and the big blind keeps its option. A raise must add at least the last full raise (the big blind at a street's start). An all-in that raises by less is a short raise: every other seat acts again, but a seat that had already acted on the street may only call or fold.
- **Run-out.** When at most one seat can still bet, the rest of the board is dealt straight to showdown.
- **Pots.** Each distinct contribution level of a seat still in the hand closes a pot holding what every seat put in between the previous level and this one; seats that reached the level are eligible. Folded chips above the top live level join the last pot, and a bet nobody called returns as a one-seat pot.
- **Showdown.** The evaluator scores every 5-card subset of the 7 cards and packs category plus five tie-break ranks into one comparable `u32`; the wheel (A-2-3-4-5) is a five-high straight. A split pot's odd chips go one at a time to the tied winners nearest the button's left. The evaluator test enumerates all 2,598,960 five-card hands against the textbook category counts.
- **End.** One seat holds every chip, every human has busted, or `max_hands` is reached.

The pad menu: Left / Right choose Fold, Check-Call, Raise or All-in; Up / Down size the raise in big-blind steps; Cross commits. AI seats reduce a spot to a win equity: the novice guesses it from its hand category and calls loosely; the regular and the expert sample it by Monte Carlo against random holdings (60 and 200 deals), compare it with pot odds and their fair share of the pot, and the expert bluffs now and then.

## Trading cards

A pack's `cards.toml` defines the cards. Each has an id, a name and flavour text of the pack's own, a rarity (common, uncommon, rare, legendary), an optional element, four side strengths for the duel, and a disc coordinate for its art (a retail monster by id, an item). `cards::CardBook` wraps the section:

- **Boosters.** A booster set has a size, an optional card pool (default: every card), rarity weights and an optional guaranteed rarity for the last slot. Each slot rolls a rarity on the weights, with any rarity the pool lacks weighted zero, then a card uniformly within it.
- **Rewards.** A reward rule ties a source (a retail monster id, a pack formation, a pack boss, or a pack mode id) to a booster or a card, a chance and a count; `roll_rewards` rolls each.
- **Selling.** A card's own `sell` price, else the section's per-rarity table, else a built-in default.
- **Collection.** Copies per id plus a binder order (every owned id once, new ids appended, the last copy leaving the binder). It is serde-derived for saves, and `normalize` repairs one read from an older save.
- **Trades.** A `TradeOffer` lists what each side gives. `execute` validates both collections first (and, given a book, that every card exists), then moves every line; a refused trade leaves both collections untouched.

### The card economy

The player's cards are pack progress: `cards::economy::CardProgress` (the collection, unopened boosters by set, the chosen duel deck, the ids already shown, whether the starter cards were handed over, and the card stream's state) sits in `ExpansionProgress::cards` and rides the save's expansion block as tail record 3, written only once it holds anything - so a save with no card state, and every save made without a pack, keeps its bytes. Like quests, it follows the pack: parked in the dormant block when the pack is deactivated, adopted again when it returns. `legaia_engine_expansion::cards` is the plumbing around it, and `world::expansion_cards` the `World` side.

- **Grants.** Every path lands in the collection: quest rewards (`quests.card_inbox`) and mode wins (`pending_card_rewards`) are drained by `World::collect_card_grants`, which the quest settle and the mode settle both call; battle drops are credited where they roll. An id the book does not know is dropped. Each credited grant raises a `CardNotice`; the world turns notices into toast lines ("Got card: Mist Wisp (new!)") shown for 150 frames, three at a time, and keeps them for a host to take (`World::take_card_notices`).
- **Starter.** The section's `starter` cards are handed over once, the first time the card screen opens or a pack duel is dealt.
- **Battle drops.** `World::apply_battle_loot`, after the retail spoils and the pack boss reward, makes one call into `expansion_card_drops`: the sources are each defeated seat's retail monster id, the pack formation's local key and the pack boss whose formation was fought, and every matching rule rolls. A no-reward fight (the special-battle word set) rolls nothing.
- **The card stream.** Every card roll - drops and boosters - draws from `CardProgress::rng`, a SplitMix stream saved with the collection. It is seeded once from a read of the world's frame counter and RNG word mixed with the pack id, and never touches the game's RNG: a fight's spoils, its item drop and the world's RNG word come out identical with a pack active or not, and a reloaded save opens the same boosters.
- **Shop.** `buy_booster` charges a dealer's priced stock out of the party purse; `open_booster` consumes one and adds its cards; `sell_card` pays the dealer's `sell_percent` of the card's sell value into the purse, clamped at 9,999,999.
- **Decks and stakes.** A pack Grid Duel deals the player `CardProgress::duel_deck`: the chosen deck while all five are still owned, else the strongest five owned cards (a card fills as many slots as it has copies), else - with fewer than five owned - the duel's lent starter (the first five cards of the section). Under the duel's `stakes` rule the winner's take is real: a card the player wins arrives as a pending reward, and a card the opponent takes from the player's deck leaves the collection (`settle_card_stakes`; a lent card costs nothing). The built-in practice duel keeps the starter deck.

### Trading between players

`cards::trade::TradeTable` runs one trade on an authority - the netplay host. The proposer puts an offer on the table, which bumps its revision and clears both confirmations; each side confirms that revision; once both have, the host's `settle` validates the offer against its own collection and the partner's last reported holdings and swaps, every line or none. A failed validation reopens the table with the confirmations cleared. The partner then applies its half with `TradeOffer::apply_one_side`, validated against its own collection.

The wire form is `TradeMsg` (holdings, offer, confirm, cancel, settled, refused) with its own byte codec. It is the payload of the netplay protocol's existing host-authoritative messages - `Action` from a client, `Event` from the host - so the netplay crate carries trades with no message of its own.

### Card screens

`cards::screen::CardScreen` is one pad-driven state machine over a `CardProgress`, the party purse and the card book; `World` seats it as `ExpansionState::cards.seat` under `SceneMode::Expansion`, and `World::tick_pack_mode` hands it the frame before the mode seat.

| Stage | Shows and does |
|---|---|
| Card menu | Binder, Open boosters, Sell cards, Duel deck. Opened by `World::open_card_menu` |
| Dealer counter | The dealer's priced stock as Buy rows, then Sell cards, Open boosters, Binder. Opened by a map's `card_dealer` trigger (`World::open_card_dealer`) |
| Binder | The whole set four by two per page: an owned card's face (portrait, element strip, name, sides, copies, a NEW tag for a card never shown), a missing one as a numbered blank. L1 / R1 page; Square cycles the sort (set order, newest last, rarity, strength, copies), Triangle the filter (all, owned, missing, each rarity); Cross opens the detail |
| Card detail | The portrait large, the sides on a cross, rarity, element, copies, sell value and the flavour text; Left / Right browse |
| Boosters | Unopened sets with their counts and what a pack holds; Cross opens one |
| Reveal | The pack's cards face down; Cross turns the next, Triangle all, and a card never owned before is marked NEW |
| Sell | Owned cards with what the dealer pays; Cross sells one copy |
| Deck builder | Owned cards with copies in the deck, and the deck's five slots; Cross adds, Square removes the last, Triangle fills with the strongest, Circle keeps a full deck |

Circle backs out a stage and Start leaves from any; the header shows the purse and the set completion. `legaia_engine_ui::expansion_modes::card_screen_prims` / `card_screen_texts` draw every stage, and `legaia_engine_screens::expansion_modes` routes them through the same `pack_mode_prims` / `pack_mode_stage_texts` calls as the mode screens, so both hosts draw them with no code of their own. The toasts ride `pack_mode_stage_texts` in every mode: over play at the top of the stage (a battle drop shows over the results), above the footer inside an expansion screen.

### Portraits

A card whose art is a retail monster shows that monster. The first time the card screen opens with a pack whose portraits are not built, the scene host renders one `64 x 48` RGBA image per monster off the player's own disc (`legaia_actor_formats::monster_portrait`): the battle mesh and texture pool staged as battle slot 0 (boss effect page included), posed in the idle action's first frame, drawn by the `mesh_raster` software rasteriser from a three-quarter view. The images stay in memory on the world (`ExpansionState::cards.portraits`); a pack carries only the monster id.

The screen draws an image as flat stage-pixel runs of equal 15-bit colour, scaled nearest to its box, so it reaches both hosts through the ordinary screen-prim pass with no texture upload. Without a disc a card shows its element colour in the art box.

### Grid Duel

The card battle. Two players, five cards each with both hands face up, a 3x3 grid; a seeded coin flip picks who starts. Players alternate placing a card on an empty cell (the starter places five, the other four).

- **Capture.** A placed card takes each orthogonally adjacent opponent card whose facing side is strictly lower than its own.
- **Elemental cells** (optional). One to three cells carry an element; a card of that element plays every side at +1 there, a card of another element at -1 (never below 1).
- **Chain** (optional). Each card a placement captured then captures its own weaker opponent neighbours, one step and no further.
- **Score.** Owned grid cards plus unplayed hand cards; more wins, equal draws.
- **Stakes** (optional). The winner takes one card of the loser's deck: picked with the pad, or the strongest for an AI winner. In a pack the card moves between collections ([The card economy](#the-card-economy)).

The novice AI plays a random legal move; the regular maximises immediate captures while not exposing weak sides; the expert also subtracts the opponent's best reply.

## Golf

A course is a list of holes, each a tile grid written as strings in `modes.toml` (`.` fairway, `,` rough, `s` sand, `w` water, `#` wall, space out of bounds, `T` tee, `O` cup, `^ v < >` downhill slopes) with an optional height-digit grid of the same shape. A tile is 32 world units.

- **Ball.** A point integrated in four sub-steps per frame: arrow slopes and the negative height gradient accelerate it, surface friction (fairway, then rough, then sand, each stronger) and a small constant drag slow it, each axis moves separately and reflects off walls at 60 %, and a ball inside the cup radius below a speed limit drops. A ball that settles below a rest speed for 20 frames, or rolls for 20 seconds, stops.
- **Hazards.** Water or out of bounds costs a penalty stroke and replays from where the ball was struck.
- **Stroke.** Left / Right aim (Square held for fine steps), Cross starts a power meter that climbs to 100 and falls back, Cross again strikes (Circle returns to aiming).
- **Order.** Tee shots in seat order on the first hole, then lowest previous-hole score first; afterwards the ball farthest from the cup plays. A player who reaches the hole's cap (`max_strokes`, default par + 5) picks up and scores the cap.
- **Result.** Lowest total strokes wins.

AI seats look ahead with the same physics: they simulate candidate aims and powers, keep the shot ending nearest the cup (holing out first, hazards last), then perturb it by their tier's error.

## Racing

A track is a closed centerline polyline with a half-width; every point within the half-width of a segment is drivable, and every centerline point is a checkpoint.

- **Checkpoints.** A racer passes the next checkpoint when the segment nearest to it becomes the segment leaving that checkpoint. The switch happens on the corner's bisector, so a cut corner still counts and a skipped segment never does; only the next checkpoint in order counts. Passing checkpoint 0 closes a lap.
- **Driving.** Heading plus scalar speed: Cross accelerates, Square brakes, Left / Right steer, R1 spends a full boost meter (it refills a unit a frame), and boost-pad segments give a short boost free.
- **Contact.** At the edge the racer is pushed back inside, the velocity's outward component is dropped and the rest turns the heading along the edge at seven eighths speed. Overlapping racers are pushed apart and the one behind loses a quarter of its speed. A finished racer coasts and no longer collides.
- **End.** All racers finished, `finish_grace` frames after the first finisher, or the time limit; unfinished racers place behind the finishers by laps, checkpoints and distance to the next one.

AI racers steer by pure pursuit at a point two half-widths ahead along the centerline, offset by a per-racer line; tiers cap their top speed, brake for sharp turns and use the boost.

## Prop hunt

Seats split into hunters and hiders, rotating each round so every seat hunts in turn.

1. **Choose.** Hiders pick a prop from the arena's allowed list (Left / Right, Cross); the cursor's prop is taken when the timer runs out.
2. **Hide.** Hiders move; hunters are held at the spawn until the hide timer ends.
3. **Hunt.** Hunters move and inspect (Cross): the nearest prop within reach (its radius plus a fixed reach) is checked, a hider winning a distance tie. A disguised hider is found and out for the round; a decoy costs the hunter `miss_penalty` health, and a hunter at zero is out; nothing in reach is a free whiff. Inspects have a cooldown. Hiders may taunt (Triangle) for a few points, which reports their position to every host.

The round ends when every hider is found (hunters win), every hunter is out, or the timer expires (hiders win). Hiders score a point per second of hunt survived and a bonus for surviving; hunters score per find and a bonus for a won round. After the last round the highest total wins.

AI hiders stand beside a decoy of their own prop (the expert picks the prop with the most decoys), and novices fidget and taunt. AI hunters inspect unchecked props nearest first; regulars and experts prefer a prop they saw move.

## Hosts

Both play hosts run the modes inside the running game, through one seat on the `World` and one screen composition.

### The seat

`active::ActiveMode` is any one of the five sessions behind a common surface: build an instance by id from a mode section and a card section, tick it, read its view, settle it. `PackModeSeat` (in `engine-expansion`) holds one `ActiveMode` at a time, or the instance picker, or a finished session's results, and the mode the world returns to. `World::open_pack_mode_picker` and `World::enter_pack_mode(id)` seat it and switch the world to `SceneMode::Expansion`; the mode tick then runs `World::tick_pack_mode` in place of field and battle dispatch, feeding seat 0 the frame's pad word. `World::exit_pack_mode` closes it from outside (a session still in play is forfeited).

The instances are the active pack's `modes.toml` (with its `cards.toml` as the duel's card book) when the pack defines any mode, else the built-in practice set. A pack duel deals the player their collection's deck ([The card economy](#the-card-economy)); the practice duel, and a pack duel before the player owns five cards, deal the first five cards of the card section. A golf round seats one regular AI beside the player.

`SceneMode::Expansion` is not a retail mode: `GameMode::for_scene_mode` answers `None` for it, as for `Title`, and nothing reaches it without a host entering the seat, so a world that never does plays exactly as before.

### Rewards

When a session finishes, the seat settles it: the session's own rewards, plus, when the player placed first, every `cards.toml` reward rule whose source is that mode's id. Seat 0's grants land at once - a currency change in the party gold purse or the casino coin bank (clamped to `0..=9,999,999`), boosters and cards in `ExpansionState::pending_card_rewards` for the card economy to collect. Quitting through the quit prompt grants nothing, a lost poker stack included.

### Entry

| Host | Entry |
|---|---|
| Native window | `` ` `` (backquote) on the field or overworld opens the picker; pressed again inside, it leaves. `--mode <INSTANCE>` enters an instance once the launch scene is entered, `--mode picker` opens the picker |
| Browser play page | The Game mode select in the play tools: the picker, every instance by name, and Leave the mode |
| Card screens | Native: `\` (backslash) opens the card menu and closes it; `--mode cards`, `--mode cards-binder` and `--mode cards-dealer[:ID]` open a card screen at launch. Browser: `play_cards_open("menu" / "binder" / "dealer:ID")`, `play_cards_close`, with `play_cards_json` (counts, completion) and `play_card_notices_json`. A pack map opens a dealer through its `card_dealer` trigger |

### Controls

Inside every mode, Start raises the quit prompt (Cross leaves, Circle resumes). The picker takes Up / Down and Cross, and Circle or Start backs out; the results card leaves on Cross.

| Mode | Controls |
|---|---|
| Poker | Left / Right: Fold, Check-Call, Raise, All-in. Up / Down: raise size in big blinds. Cross: act, or skip the showdown |
| Grid Duel | Left / Right: hand card. Cross: lift it, then D-pad to a cell and Cross to place. Circle: put it back |
| Golf | Left / Right: aim (hold Square for fine steps). Cross: start the power meter, Cross again to strike. Circle: back to aiming |
| Racing | Cross: accelerate. Square: brake. Left / Right: steer. R1: boost |
| Prop hunt | Choose: Left / Right and Cross. Then the D-pad moves; a hunter inspects with Cross, a hider taunts with Triangle |

### Screens

`legaia_engine_ui::expansion_modes` builds every screen in the 320x240 stage as flat panels plus dialog-font labels; `mode_screen_prims` and `mode_screen_texts` keep the two halves of one composition, and no panel's geometry depends on a label's measured width. `legaia_engine_screens::expansion_modes` projects the seat into it: while the seat owns the frame, `screen_layers::compose_screen_prims` returns its panels as the whole under-text list (only the pause wipe may cover it), and each host's text layer appends `pack_mode_stage_texts`, scaled with the rest of its stage group.

| Screen | Shows |
|---|---|
| Picker | Every instance with its mode kind, the cursor row, the last entry error |
| Poker | Opponent plates along the far rail (chips or state, dealer button, card backs, street bets, revealed hands at showdown), the board and pot, the player's hole cards and stack, the action menu with the raise size, the event log |
| Grid Duel | The 3x3 grid with elemental cells, both hands down the sides (the selected card lifted), the score and turn line. A card shows its four side strengths, element strip, name and an art frame |
| Golf | The hole top-down from its tiles (surfaces, height shading, slope arrows), cup and flag, every ball, the aim line, the power meter and the scorecard |
| Racing | A top-down track map with boost segments and the start line, every racer with its heading, and the lap / place / speed / boost / order panel. A functional view |
| Prop hunt | A top-down arena with blockers, decoys, hunters and hiders, and the round / timer / role / score panel. A hidden hider draws exactly like a decoy while the player hunts. A functional view |
| Results | Placements with each mode's score, and what was won |

## Tests

- **Rules.** Each module tests its rules directly: hand categories, kickers, ties, the wheel and the exhaustive five-card census; blinds, the big blind's option, minimum raises, short all-ins, side pots and odd chips; booster guarantees, reward chances, trade atomicity and binder order; captures, chains and elemental cells; golf surfaces, walls, hazards, slopes and turn order; checkpoint order, edge sliding and boost; inspect, misses, taunts and role rotation.
- **Determinism.** `tests/determinism.rs` runs each mode from the Rebirth pack twice with the same seed and the same scripted pad stream and asserts identical `state_hash` sequences and results, then asserts a different seed diverges so the hash cannot be vacuous.
- **Soak.** `tests/soak.rs` drives every mode with random pads over many seeds (all-human poker tables included) and asserts no panic, termination within a frame bound, chip conservation, a full duel grid, a complete golf scorecard within the stroke cap, and no avatar inside a wall.
- **Pack.** `tests/rebirth_pack.rs` parses the Rebirth `cards.toml` / `modes.toml` and converts every instance into a session config.
- **Seat.** `engine-modes` `active` builds every built-in instance and rolls a won mode's reward rules; `engine-expansion` `modes` drives the picker, the quit prompt and the results card.
- **World.** `engine-core` `tests/expansion_modes_world.rs` enters each built-in instance from the field, drives it to its end through `World::tick` with scripted pads, and checks the coin bank, the pending card rewards and the restored field.
- **Screens.** `engine-ui` `expansion_modes::tests` checks each screen's key labels and that every panel lies on the stage; `expansion_modes::binder` checks the card screens' labels, the portrait runs and the toast placement.
- **Card economy.** `engine-modes` `cards::economy`, `cards::screen` and `cards::trade` test buying, opening (the same saved stream opens the same pack), selling at a dealer's rate, the starter, the duel deck, every screen stage driven by pad, and the trade table's revisions, atomicity and codec. Further:
  - `engine-expansion` tests the grant drain, the battle sources, stakes, the toasts, and the save block's card record (round trip, and no record without card state).
  - `engine-core` `world::tests::expansion_cards` checks that drops leave the retail spoils and RNG word untouched while crediting the collection, that a no-reward fight drops nothing, the grant drain, stakes both ways, a dealer purchase through `World::tick`, the duel deck and the save block.
  - The disc-gated `engine-scene-host` `expansion_cards_disc` checks every Rebirth card's art against a real monster record, renders every portrait, and builds the cache through the scene host.
