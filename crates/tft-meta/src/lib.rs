//! tft-meta — Lobby → Accept → Loading → Running meta-FSM.
//!
//! This crate implements the meta-game loop that automates:
//! - Creating a lobby and starting matchmaking (via LCU REST)
//! - Accepting ready-check (via LCU REST polling)
//! - Waiting for game to load (via 2999 in-game API)
//! - Initializing the game window and handing off to the in-game loop
//!
//! When LCU is unavailable, the `Manual` mode skips lobby automation
//! and starts from window/game detection.

pub mod config;
pub mod fsm;
pub mod ingame_api;
pub mod lcu_client;
pub mod lobby;
pub mod ready_check;
pub mod rule_shop;

pub use config::MetaConfig;
pub use fsm::{MetaFsm, MetaState};
