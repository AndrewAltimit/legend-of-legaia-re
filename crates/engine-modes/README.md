# legaia-engine-modes

Rules engines for the port's own game modes - content retail never had:
poker, golf, racing, trading-card battles and prop hunt. Each is a
deterministic, `World`-free session ticked once per frame from pad input,
the contract `engine-minigames` keeps for the retail minigames, so a mode
replays bit-identically and can run in lockstep between networked players.
Rules, the session contract and the lockstep rationale are in
[`expansion-modes.md`](../../docs/subsystems/expansion-modes.md).

## What lives here

| Module | Covers |
|---|---|
| `session` | The `ModeSession` trait, seats and AI tiers, results and rewards, pad press edges, the session RNG and the state hasher. |
| `fixed` | 20.12 fixed point, 12-bit angles, table sine and cosine, integer `atan2` and square root. |
| `poker` | Texas Hold'em: table session, hand evaluator (`hand`), side pots (`pots`), AI tiers (`ai`). |
| `cards` | Card book, collection, boosters, reward rolls, selling and trades; `cards::duel` is the Grid Duel card game. |
| `golf` | Mini-golf: tile courses and ball physics (`course`), stroke and turn session. |
| `racing` | Checkpoint racing on a closed centerline track. |
| `prop_hunt` | Prop-hunt round rules over a minimal 2D arena. |

Every mode builds its config from the matching expansion-pack instance
(`PokerConfig::from_table`, `DuelConfig::from_duel`,
`GolfConfig::from_course`, `RaceConfig::from_def`,
`PropHuntConfig::from_arena`), parsed by `legaia-expansion-format`.

No host draws these modes yet; each exposes a read-only `view()` snapshot
for one to draw from.
