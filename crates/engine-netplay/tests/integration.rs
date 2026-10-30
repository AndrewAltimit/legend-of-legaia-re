//! Netplay through in-process worlds: networked poker, golf and prop hunt,
//! shared-world roaming, desync detection, and a real-socket smoke test.
//!
//! Synthetic worlds with no pack (the built-in practice modes), no disc
//! gate, no Sony bytes.

mod field_hunt;
mod harness;
mod modes;
mod party;
mod roaming;
mod sockets;
mod trades;
