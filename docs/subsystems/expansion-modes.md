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
| Modes | `poker`, `cards` + `cards::duel` (and the economy: `cards::economy`, `cards::screen`, `cards::trade`), `golf`, `racing`, `prop_hunt`; `party` chains them into a party night |
| Pack schema | `[[poker]]`, `[[card_duels]]`, `[[golf]]`, `[[races]]`, `[[prop_hunt]]`, `[[party_nights]]` in `modes.toml`; `[[cards]]`, `starter`, `[[boosters]]`, `[[dealers]]`, `[[rewards]]` in `cards.toml` |
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

Hidden information (poker hole cards) would be part of every peer's state under lockstep, so netplay does not run the turn-based modes that way: poker and Grid Duel run on the host alone, which sends each guest only the view its seat may see ([`netplay.md`](netplay.md#modes)). The real-time modes - golf, racing, prop hunt - hide nothing and run in lockstep.

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

Circle backs out a stage and Start leaves from any; the header shows the purse and the set completion. `legaia_engine_ui::expansion_modes::card_screen_prims` / `card_screen_texts` draw every stage, and `legaia_engine_screens::expansion_modes` routes them through the same `pack_mode_prims` / `pack_mode_stage_texts` calls as the mode screens, so both hosts draw them with no code of their own. The toasts ride `pack_mode_stage_texts` in every mode: over play at the top of the stage (a battle drop shows over the results), above the footer inside a mode screen; the card screen marks new cards with its own NEW tags and shows none.

### Portraits

A card whose art is a retail monster shows that monster. The first time the card screen opens with a pack whose portraits are not built, the scene host renders one `64 x 48` RGBA image per monster off the player's own disc (`legaia_actor_formats::monster_portrait`): the battle mesh and texture pool staged as battle slot 0 (boss effect page included), posed in the idle action's first frame, drawn by the `mesh_raster` software rasteriser from a three-quarter view. The images stay in memory on the world (`ExpansionState::cards.portraits`); a pack carries only the monster id.

The card screens draw an image as flat stage-pixel runs of equal 15-bit colour, scaled nearest to its box, so it reaches both hosts through the ordinary screen-prim pass with no texture upload. Without a disc a card shows its element colour in the art box.

A Grid Duel draws its cards from a texture page instead ([The card page](#the-card-page)), and opening a duel builds the portraits as the card screen does.

### Grid Duel

The card battle. Two players, five cards each with both hands face up, a 3x3 grid; a seeded coin flip picks who starts. Players alternate placing a card on an empty cell (the starter places five, the other four).

- **Capture.** A placed card takes each orthogonally adjacent opponent card whose facing side is strictly lower than its own.
- **Elemental cells** (optional). One to three cells carry an element; a card of that element plays every side at +1 there, a card of another element at -1 (never below 1).
- **Chain** (optional). Each card a placement captured then captures its own weaker opponent neighbours, one step and no further.
- **Score.** Owned grid cards plus unplayed hand cards; more wins, equal draws.
- **Stakes** (optional). The winner takes one card of the loser's deck: picked with the pad, or the strongest for an AI winner. In a pack the card moves between collections ([The card economy](#the-card-economy)).

The novice AI plays a random legal move; the regular maximises immediate captures while not exposing weak sides; the expert also subtracts the opponent's best reply.

A duel with no table in the world draws as its 2D board: the grid on a gold-rimmed felt, each hand fanned down its side with the selected card slid out toward the board, the score plates, the turn line and the selected card's panel (its full name, rarity, the four strengths, and what the cursor's elemental cell does to it). A duel with a table is played in the world ([Grid Duel at a table](#grid-duel-at-a-table)).

### The card page

Every card the duel shows, on the 2D board and on the table alike, is a textured quad sampling one 15-bit texture page the world writes into VRAM while a duel is seated (`legaia_engine_modes::cards::table_art`, written by `World::step_duel_card_page` inside the field VRAM step both hosts run and re-upload after):

| Slot | Texture |
|---|---|
| `0..=4` | Seat 0's five faces |
| `5..=9` | Seat 1's five faces |
| `10` | The card back: an indigo lattice under a gold sun emblem |

A face is `52 x 72` texels: a bevelled frame in its rarity's metal (bronze, silver, blue steel, gold), the monster's portrait box-filtered into a window over a vignette in its element's colour, an element gem, the four strengths on a cross under the art, and rarity pips. The body under the strengths is transparent (texel `0`): the drawing puts a quad in the owner's colour under the face, so a captured card changes colour with no second texture. The art is built from the session's cards and the portraits the scene host rendered; no disc bytes are written.

The page sits at `(0, 256)`, `208 x 216` halfwords, in retail's display-buffer band, which the port never draws a frame into; the pack-map VRAM allocator keeps the rectangle out of its free list (`expansion_map::vram_alloc::RESERVED`). The words it covers are kept on the first write and put back when the duel closes, and a rebuild of the scene's VRAM under a running duel is noticed (a probe row of the back) and written again.

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

## Party night

`party::PartyNight` chains short rounds of the other modes into one
points-based night: a round-intro card, a scoreboard after every round, a
podium and prizes. It plays no round itself. A host ticks it once a frame;
when it cues a round, the host builds the round from `party::round_section`
(the base instance cut short by the round's knobs, widened to seat
everyone), plays it, and reports the placements back. Two machines that
build the night from the same definition, players and seed and report the
same results hold the same scoreboard, which `scoreboard_hash` digests
(timers excluded, so a copy that only runs its clock down still agrees).

A night is a `[[party_nights]]` table in `modes.toml` (format in the
[expansion pack reference](../formats/expansion-pack.md#modestoml)); the
built-in set carries **Practice Night** (a golf hole, two poker hands with
a wager, one lap, a Grid Duel ladder), so a night is playable with no pack.

| Rule | How it scores |
|---|---|
| Placement | `points[place - 1]`, first place first; tied seats share a place and its points; a place past the table scores 0. |
| Double points | A round marked `double`, and the final round when `double_final` is set (the default). |
| Wager | Every player still in stakes `wager` percent of their points (rounded down, never below zero); the round's winners split the pot, the remainder to the earliest seated. Nobody placing first returns the stakes. |
| Last place picks | With `last_place_picks`, before every round after the first the player in last place picks the next round from those left; an AI rival picks at once, and a pick not made in ten seconds takes the highlighted round. |
| Grid Duel | King of the hill: the two lowest on the board duel first, the winner holds the hill against the next, the leader defends last; a draw keeps the king. Placement: the last king first, then the others in reverse order of falling. |
| Void | A round that cannot finish scores nothing and takes no stakes; the night goes on. |
| Leaving | A player who leaves keeps their total, scores nothing more and wins no prize. |
| Prizes | `[[party_nights.prizes]]` by final place (tied players share it): gold, coins, boosters and cards. A night ended early, or one whose every round was void, grants none. |

The round knobs are `hands` and `stack` (poker), `holes` (golf, 1-based),
`laps` (race) and `rounds` (prop hunt). A party Grid Duel bets no card.

### Played alone

`World::enter_pack_mode` with a night's id (a map trigger's `mode` event, a
dialogue choice's `mode`) opens the night against its AI rivals
(`players` seats, named from `rivals`, playing at `ai_tier`): the world's
`LocalNight` (`engine-expansion` `party`, `engine-core`
`world/expansion_party.rs`) directs it under `SceneMode::Expansion`.
Between rounds the seat is empty and the mode screen draws the party card;
a round plays on the seat like any instance, settles nothing of its own (a
poker round's chips are only points), and its results stand three seconds
before the night scores them. A Grid Duel match between two rivals plays
out at once, AI against AI. Start on a party card ends the night early;
any button closes the finished podium. Online, the room's host directs one
copy per player ([netplay.md](netplay.md#party-night)).

The party card is `engine-ui` `expansion_modes::party`, projected for a
night alone by `engine-screens` and for a room by `engine-netplay`
`screens`, so both hosts draw one composition: the intro card slides in
with the round's kind, title, detail and tags (double points, the wager,
the duel match-up) over a strip of the standings; the scoreboard's rows
step in with each player's last-round delta and the next round (or the
pick); the podium raises 2nd / 1st / 3rd blocks with a figure in each
player's colour, confetti and sparkles over a completed night.

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
| Grid Duel | The 3x3 grid on its board with elemental cells, both hands fanned down the sides (the selected card slid out), the score plates, the turn line and the selected card's panel. Cards are the card page's faces |
| Golf | The hole top-down from its tiles (surfaces, height shading, slope arrows), cup and flag, every ball, the aim line, the power meter and the scorecard |
| Racing | A top-down track map with boost segments and the start line, every racer with its heading, and the lap / place / speed / boost / order panel. A functional view |
| Prop hunt | A top-down arena with blockers, decoys, hunters and hiders, and the round / timer / role / score panel. A hidden hider draws exactly like a decoy while the player hunts. A functional view |
| Results | Placements with each mode's score, and what was won |

## In-world golf and racing

A golf course or a race that names a pack map (`map = "<id>"`, schema in the expansion-pack format page's `modes.toml` section) is played in the 3D field scene the map builds ([`expansion-pack-maps.md`](../formats/expansion-pack-maps.md)) instead of on a top-down board. The session is unchanged and stays the only authority: the map only lays the course or track out, and what is drawn is read off the session's view each frame and never fed back. The pieces:

| Piece | Where |
|---|---|
| Course and track from a map | `golf::course::Course::from_map`, `racing::Track::from_map`, built by `ActiveMode::build_with_maps` |
| Mode units on the field | `map_frame::MapFrame` (origin and scale), `ActiveMode::map_frame` |
| The stage (leader, balls, aim, bodies, trails, camera shot, banner) | `legaia_engine_expansion::mode_stage::WorldStage`, kept on `PackModeSeat` |
| Moving the leader | `World::stage_pack_mode` (`world::expansion_stage`), after the seat's tick |
| Entering the map | `SceneHost::sync_pack_mode_map`, off `World::pack_mode_wants_map` |
| Camera | `legaia_engine_world_views::pack_mode_world::stage_frame`, a `FieldCameraFrame::Venue` arm of `resolve_field_camera` |
| World pieces | `pack_mode_world::stage_prims` through `HostView::pack_mode_world_prims` |
| HUD | `ModeScreen::InWorld` (`engine-ui` `golf::hud`, `race::hud`) |

### Laying a course out on a map

A map hole is the map's tile grid inside the hole's `area`, one golf tile per map tile: a tile with any wall quarter is a wall, a void tile and everything outside the area is out of bounds, and an open tile plays as its legend character's surface. Heights are the rank of each tier's floor height among the tiers the hole uses, doubled, because the session's slope is a central difference that halves a one-step change. The tee and the cup sit on their tiles' centres.

The session runs in its own mode units, exactly as for a tile-string course, and `MapFrame` places them on the field: mode `(0, 0)` at the area's corner tile, mode `+x` / `+y` on world `+X` / `+Z`, so a 12-bit mode angle keeps its meaning on the ground. The scale is per mode. Golf runs at four world units a mode unit, which makes a golf tile one 128-unit map tile and keeps a hole the length the ball physics was tuned for; racing runs at two, where the racers' top speed is about the field player's run. A race's centerline is its `waypoints`' tile centres in those units; the track edge is still the session's half-width, so a map's corridor should be drawn to match it.

### The stage

`WorldStage::step` turns the session's view into a `StageFrame` once a frame, while the session's map is the one the player is in:

- **Golf.** The party leader stands beside the player's ball facing the aim, plays the swing for 24 frames after a strike, and runs to the ball's new lie once it rests; holed, it waits by the cup. The other seats stand at their own balls as bodies, on the far side from where the leader would stand. Every ball not yet holed draws, the rolling one at the session's rolling position. The aim arrow lies on the ground from the ball to play, lengthening with the power meter and warming from yellow to red as it fills. The camera sits behind the ball to play looking down its aim, follows a rolling ball along the shot's line, and settles on the cup between holes; it eases toward each target. Banners name the hole and par at its start and the score as a ball drops ("Birdie!", "Hole in one!").
- **Racing.** The leader is the player's racer: its position, its heading, the run clip at speed and the walk below it. The other racers are bodies. A boosting racer leaves a fading ribbon of its last positions. The camera chases from behind on the heading, a little ahead of the racer and pulled back as it speeds up. Banners carry the countdown, "Go!", each new lap and the finish place.

The party bank has no swing clip, so the swing plays the landing clip (clip base `7`, the hop phase machine's landing), whose crouch and rise reads as a putt. The leader's walk to its ball, the camera ease and the banner timers are the stage's own memory; nothing the stage keeps reaches the session.

`World::stage_pack_mode` applies the leader: world position, the floor height under it, the field heading (`render_26`, `(1024 - angle) & 0xFFF` from a mode angle), the clip through the strided-clip binder when it changes, and one step of the player animation. When the seat closes the player returns to the pose it had when the seat opened - on the map's spawn when the mode entered its map.

### Bodies, the leader seat and online play

The leader is the session's local seat (`ActiveMode::local_seat`, seat `0` offline), so a networked peer stands its own golfer or racer and sees the others as bodies. An instance's `bodies` names map NPCs (`[[npcs]]` of its map) for the other seats, in seat order with the leader's skipped: `World::stage_pack_mode` moves each named NPC to its body's pose - its anchor in `World::npcs.positions`, its heading - cues its `walk_clip` while the body moves and its idle clip at rest, and both hosts draw it through the field NPC path, so the bodies are real meshes (a party member placement, model byte `0xF0` and up, draws on any map).

The field's NPC clips keep advancing under the stage (`World::field_npc_clips_advance`). When the seat closes each NPC returns to its tile and idle clip. A networked session builds with the active pack's maps (`NetMode::build` takes them), so map instances play online.

With action movement on (`WorldToggles::action_movement`), an in-world race reads the action dash button (`ACTION_DASH_BUTTON`, R2) as the boost (R1) too.

### Entering the map

`World::enter_pack_mode` and the picker build a map instance with the active pack's maps. The world cannot load a scene, so `World::pack_mode_wants_map` names the map while the player is elsewhere, and `SceneHost::tick` hands it to `sync_pack_mode_map`: the seat is lifted off the world, the map is entered at the instance's `spawn`, and the seat is put back under `SceneMode::Expansion`, set to return to the field of the map. A map that fails to enter is logged once and the instance plays as its top-down screen. An instance with no map is always the top-down screen.

### Drawing it

Both hosts draw the field under the seat as they draw any field scene, the leader with the field's actors and its drop shadow included. The camera is the shared resolver's: under an in-world stage `resolve_field_camera` returns `FieldCameraFrame::Venue` built from the stage's shot (the field follow camera's pitch, `H` and eye depth, the shot's yaw and pull). The stage's own pieces are screen primitives projected through that frame, every corner carrying its scene depth so each host depth-tests them against the field it drew:

| Piece | Shape |
|---|---|
| Ball | A shaded disc facing the camera with a band of the seat's colour, over a blob shadow |
| Cup | A dark disc with a white rim, a flagpole and a red pennant |
| Aim arrow | A ground strip and head, translucent |
| Body | A capsule billboard in the seat's colour with a head, over a blob shadow |
| Boost trail | A translucent cyan ribbon |

They are built in code from no disc bytes. `screen_layers::compose_screen_prims` puts them, with the drop shadows, under the seat's HUD, which is `ModeScreen::InWorld`: the hole, par and whose stroke, a compact scorecard, the power meter; or the lap and place, the running order and the boost gauge; and the stage's banner. The native window fills `HostScreenPrims::pack_mode_world` from `HostView::pack_mode_world_prims`, and the browser play page from the same call (`play_field_fx.rs`).

### Running it

```bash
legaia-engine play-window --disc "Legend of Legaia (USA).bin" \
    --expansion crates/engine-modes/tests/fixtures/links \
    --map links_green --mode links-golf
```

`--mode links-race` with `--map links_ring` runs the race. The test pack's maps draw their walls as boxes (`wall = { kind = "box" }`) and seat Noa and Gala as map NPCs, the AI golfer and racers. Entered from anywhere else (the picker, `--mode` without `--map`), the scene host takes the player to the instance's map first. A headless capture (`--screenshot-every`) presses the pad through `--pad-script`, e.g. `--pad-script 120:Cross,150:Cross` to start the meter and strike.

### Gaps

- **Stand-in figures.** A seat the instance names no `bodies` NPC for draws as a capsule billboard in its seat's colour.
- **Racer collision is the session's.** Racers keep to the track by its half-width, not by the map's walls, so a map draws its corridor to the track.
- **The browser host enters no pack map** yet ([`expansion-pack-maps.md`](../formats/expansion-pack-maps.md#gaps)); its draw path for the stage is in place for when it does.

## Grid Duel at a table

A duel that names a pack map and a table (`map`, `[card_duels.table]`, schema in the expansion-pack format page's `modes.toml` section) is played on a board laid on a table in the 3D field scene: the opponent seated across it, the party leader standing at the player's side, spectators around, and the camera taking its shots. As for golf and racing the session is unchanged and the only authority; the table reads its view each frame and feeds nothing back.

| Piece | Where |
|---|---|
| The table on the map | `cards::table::DuelTableFrame`, built by `ActiveMode::build_with_maps` (`ActiveMode::duel_table`) |
| The stage (card poses, flips, bursts, NPC moods, cues, the shot) | `legaia_engine_expansion::duel_stage::DuelStage`, inside `WorldStage` |
| NPCs and sound cues | `World::stage_pack_mode` (`world::expansion_stage`) |
| The card page | `World::step_duel_card_page` |
| Drawing | `legaia_engine_world_views::pack_mode_world` (its `duel` module) |
| HUD | `ModeScreen::InWorld` (`engine-ui` `duel::hud`) |

### The layout

The table is placed in its own axes: `across` from the player's seat toward the opponent's (the table's `facing`), `right` the player's right hand. Row 0 of the grid is the far row and column 0 the player's left, as the 2D board draws it. The layout is made from the **local** seat: each networked peer sits in the near seat and sees row 0 on the far side, so the D-pad moves the cursor the way it reads on that peer's screen and each player sees the table from their own chair.

- **Cards.** A card is a quad `0.86` of a cell wide at the face's aspect. Each hand is fanned along its seat's edge of the board, tilted up toward its holder; the selected card rises and reaches toward the board, and a held card hovers face up over the cursor's cell. Grid cards lie flat, reading from the near seat.
- **Faces and backs.** Whether a card shows its face or its back is the projected quad's winding: the near hand turns its faces to the camera, the far hand its backs, and a capture flip shows the back at its half turn.
- **Seats.** The opponent NPC stands on its seat, `seat_distance` past the board's far edge; the leader stands at the player's left, turned to the board; spectators keep their own tiles and turn to the board.

### The timeline

The stage keeps its own frame clock; every tween is a pure function of it and of the views it has seen, so the timeline is deterministic and a peer that draws nothing agrees with one that draws everything.

| Beat | What plays |
|---|---|
| Deal | The ten cards fly from the dealer's pile at the board's right into the hands, alternating seats, `6` frames apart, `16` frames each, with a deal cue apiece |
| Place | A placed card slides from the hand (or its hover) in an arc and drops onto its cell over `22` frames; it lands with a dust ring and the place cue |
| Capture | When the card lands, each card it captured turns over about its long axis in `20` frames, lifting off the board; the owner's colour changes at the half turn with the flip cue and a burst of sparks in the capturer's colour |
| Chain | A capture that is not a neighbour of the placed card is a chain capture: it waits `12` frames more, bursts larger, and raises "Chain!" |
| End | The last flips play out; the result cue and banner ("You win!", "You lose", "Draw"); the opponent cheers or slumps and the spectators the other way |

A move that takes two or more cards raises a banner ("2 captured!") and sets the NPCs' moods: the spectators cheer for the player and slump against them, the opponent the reverse, each for `90` frames; the opponent plays its `think` clip while it is choosing. A mood plays the NPC's `cheer` / `slump` clip, and an NPC without one keeps its idle. Sound cues are retail cue ids pushed on the field's SFX ring (`World::push_sfx_cue`), the table's `sfx` or the menu cues.

### The shots

| Shot | When | Framing |
|---|---|---|
| Establishing | The first `120` frames, while the cards are dealt | Wide and high over the room, turning slowly in toward the player's side, then easing into the close shot |
| Close | Play | Over the board from behind the player's seat (pitch `470`, `0.22` of the follow camera's eye depth), drifting slowly side to side |
| Push in | The player is choosing a cell | Toward the cursor's cell, closer and steeper |
| Opponent's turn | The rival is choosing | The focus moves toward the far seat |
| Pull back | A capture or a chain | Wider and steeper while the flips play |
| Reaction | The end, once the flips settle | Low across the board on the opponent's seat, for `150` frames |

A table shot frames the table itself: it ignores the follow camera's distance preset. The shots work alike in the retail-faithful and the clean rasterisation; the card art is a 15-bit page, so PSX mode snaps and dithers it like the scene.

The seat holds its results card until the reaction shot is over (`WorldStage::holds_results`; Cross does nothing meanwhile), and then draws it over the table rather than over a backdrop. Through the results stage the stage keeps stepping off the last view it saw (`WorldStage::step_after`), so the last flips and the reaction shot play under it.

### Drawing it

Everything is screen primitives projected through the stage's frame with per-corner scene depth, so an NPC or a wall in front of the table hides it.

Their order is explicit ordering-table bands, all deeper than the HUD's: the table's legs, apron and top (a wooden top on four legs, lit brightest under the board; drawn only when `draw_table`), the board (a gold rim, a dark inlay and the nine felt cells, or the base scene's `surface` texture), the elemental cells' pulsing rings and the cursor's frame, the cards' shadows (subtractive, spreading as a card rises), the cards nearest last (owner quad, then the face or back subdivided `2 x 2` so its affine mapping bends with the card, then a raised card's rim light), and the sparks (additive). None carry disc bytes but the card page's portraits and a named `surface`.

The HUD is the score plates, the turn prompt and the selected card's panel, clean of the table; it steps aside for the reaction shot, leaving the stage's banner.

### Running it

```bash
legaia-engine play-window --disc "Legend of Legaia (USA).bin" \
    --expansion packs/rebirth --map rebirth_tower_5f --mode wardens-duel
```

The Rebirth pack plays the Warden's Duel at the card room's table on its 5F floor; the `duel_table` test pack (`crates/engine-modes/tests/fixtures/duel_table`, `--map duel_room --mode table-duel`) is the minimal case.

## Tests

- **Rules.** Each module tests its rules directly: hand categories, kickers, ties, the wheel and the exhaustive five-card census; blinds, the big blind's option, minimum raises, short all-ins, side pots and odd chips; booster guarantees, reward chances, trade atomicity and binder order; captures, chains and elemental cells; golf surfaces, walls, hazards, slopes and turn order; checkpoint order, edge sliding and boost; inspect, misses, taunts and role rotation.
- **Determinism.** `tests/determinism.rs` runs each mode from the Rebirth pack twice with the same seed and the same scripted pad stream and asserts identical `state_hash` sequences and results, then asserts a different seed diverges so the hash cannot be vacuous.
- **Soak.** `tests/soak.rs` drives every mode with random pads over many seeds (all-human poker tables included) and asserts no panic, termination within a frame bound, chip conservation, a full duel grid, a complete golf scorecard within the stroke cap, and no avatar inside a wall.
- **Pack.** `tests/rebirth_pack.rs` parses the Rebirth `cards.toml` / `modes.toml`, converts every instance into a session config, and builds every round of Tower Game Night for a night alone and a full room.
- **Party night.** `engine-modes` `party` tests the placement table with ties, double points, the wager pot, the duel ladder, voiding, leaving, the last-place pick (and an AI rival's), seed-determined shuffles, serde round trips and the prize rules; `engine-ui` `expansion_modes::party` checks the intro, scoreboard and podium labels and the confetti's motion; `engine-core` `tests/expansion_party_world.rs` plays the built-in night alone through `World::tick` to its prize, and ends one early.
- **Seat.** `engine-modes` `active` builds every built-in instance and rolls a won mode's reward rules; `engine-expansion` `modes` drives the picker, the quit prompt and the results card.
- **World.** `engine-core` `tests/expansion_modes_world.rs` enters each built-in instance from the field, drives it to its end through `World::tick` with scripted pads, and checks the coin bank, the pending card rewards and the restored field.
- **In the world.** `engine-modes` `tests/map_modes.rs` lays the `links` test pack's course and track out on their maps (surfaces, walls, heights, tee and cup in their tiles, a putt holing inside the cup's tile). `engine-core` `tests/expansion_stage_world.rs` stands the map's field image under the session and plays a scripted golf round and a scripted race to their ends through `World::tick` (the leader at the ball and on the floor, the swing, the camera following the rolling ball, the chase camera), and runs both with and without the map, asserting the session's state hash matches frame for frame. The disc-gated `engine-scene-host` `expansion_stage_disc` enters both through the scene host over the retail `tower`.
- **The duel table.** `engine-modes` `cards::table` and `cards::table_art` test the table's axes and seats and the card page's layout, transparency and portrait window. `engine-expansion` `duel_stage::tests` runs whole AI duels through the stage: the session's hashes match with the stage and without it, the same seed gives the same frames, the deal starts on the pile and ends in the hands, a full grid's cards sit on their cells, a capture's colour changes mid-flip lifted off the board, the far seat sees the table turned round, and the reaction shot holds the results.
- **The duel table in the world.** `engine-core` `tests/expansion_duel_table_world.rs` plays the `duel_table` pack's duel to its end through `World::tick` on its map (the opponent seated, the leader on the near side, bursts and cues, the held results card, the NPC put back), checks the session's hashes against the 2D screen frame for frame, and checks the card page is written and put back. The disc-gated `engine-scene-host` `expansion_duel_table_disc` enters it through the scene host over the retail `tower` with its portraits built.
- **Screens.** `engine-ui` `expansion_modes::tests` checks each screen's key labels and that every panel lies on the stage; `expansion_modes::binder` checks the card screens' labels, the portrait runs and the toast placement.
- **Card economy.** `engine-modes` `cards::economy`, `cards::screen` and `cards::trade` test buying, opening (the same saved stream opens the same pack), selling at a dealer's rate, the starter, the duel deck, every screen stage driven by pad, and the trade table's revisions, atomicity and codec. Further:
  - `engine-expansion` tests the grant drain, the battle sources, stakes, the toasts, and the save block's card record (round trip, and no record without card state).
  - `engine-core` `world::tests::expansion_cards` checks that drops leave the retail spoils and RNG word untouched while crediting the collection, that a no-reward fight drops nothing, the grant drain, stakes both ways, a dealer purchase through `World::tick`, the duel deck and the save block.
  - The disc-gated `engine-scene-host` `expansion_cards_disc` checks every Rebirth card's art against a real monster record, renders every portrait, and builds the cache through the scene host.
