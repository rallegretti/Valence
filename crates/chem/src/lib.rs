//! Engine-independent chemistry core for Valence.
//!
//! - `elements`: periodic table data
//! - `lab`: the play space, bonding rules and bond-order solver
//! - `layout`: VSEPR-style 3D relaxation
//! - `naming`: common + IUPAC names (curated table, then generated)

// No-crash policy: library code must never panic on any input. Tests may.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::todo,
        clippy::unimplemented
    )
)]

pub mod canon;
pub mod compounds;
pub mod elements;
pub mod energy;
pub mod graph;
pub mod lab;
pub mod layout;
pub mod naming;
pub mod organic;
pub mod reactions;
pub mod smiles;

pub use glam::Vec3;
pub use graph::{BondKind, MolGraph};
pub use lab::{
    AtomData, AtomId, AttachMode, AttachPlan, BondData, BondId, ConnectPlan, Lab, LabData,
    MAX_ATOMS, Reject,
};
pub use naming::{Namer, Names};
