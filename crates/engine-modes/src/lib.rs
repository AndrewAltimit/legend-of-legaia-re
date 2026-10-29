//! Rules engines for the port's own game modes - content retail never had
//! (poker, golf, racing, trading-card battles, prop hunt). Each is a
//! deterministic, `World`-free session ticked once per frame from pad
//! input, the same contract `engine-minigames` keeps for the retail
//! minigames, so a mode replays bit-identically and can run in lockstep
//! across networked players.

#![forbid(unsafe_code)]
