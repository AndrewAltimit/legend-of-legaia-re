//! Netplay in the play hosts: the game modes over the network, shared-world
//! roaming, and the per-player room controller both hosts drive.
//!
//! [`legaia_netplay`] is the protocol, the session cores and the relay, and
//! names no engine crate; this crate joins it to a running `World`:
//!
//! - [`mode_net`] - which model each mode kind runs under, building one
//!   instance identically on every peer, the lockstep adapter and the
//!   host-authoritative view stream's payloads;
//! - [`view_stream`] - the view stream's host and guest ends;
//! - [`field_hunt`] - prop hunt in the field scene the room stands in;
//! - [`roam`] - avatar reports and snapshots for players walking their own
//!   worlds;
//! - [`room`] - [`room::NetPlay`], one player's connection, phases and room
//!   screen, called once per frame by the native play window and the
//!   browser play page alike;
//! - [`connect`] - the Multiplayer screen before a room exists (host or
//!   join);
//! - [`screens`] - the room screen and roaming panel off a controller, as
//!   `legaia_engine_ui` draw lists;
//! - [`party`] - party night: a playlist of short rounds with one
//!   scoreboard every peer computes and agrees on;
//! - [`trade`] - card trades between two players, settled by the host;
//! - [`avatar_pose`] - the pose a remote avatar is drawn in.
//!
//! See `docs/subsystems/netplay.md` ("In the hosts") for the flow and its
//! limits.

pub mod avatar_pose;
pub mod connect;
pub mod field_hunt;
pub mod mode_net;
pub mod party;
pub mod roam;
pub mod room;
pub mod screens;
pub mod trade;
pub mod view_stream;
