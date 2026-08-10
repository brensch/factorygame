//! OVERFLOW game rules, engine-free.
//!
//! This crate is the game: the deterministic tick simulation, the machine and
//! item definitions, the blueprint-card deck, and the round/run structure.
//! It has zero dependencies, does no I/O, and never touches a clock — so it
//! compiles unchanged to wasm32 (browser UI), embeds unchanged in Bevy
//! (desktop/mobile), and runs thousands of headless games per second in the
//! lab harness for balance work.
//!
//! Anything an engine needs (positions to draw, moves to animate, legal
//! actions to offer) is exposed as plain data. Nothing in here may ever import
//! an engine type.

pub mod boards;
pub mod deck;
pub mod defs;
pub mod rng;
pub mod run;
pub mod sim;
