//! Engine-independent chemistry core for Valence.
//!
//! - `elements`: periodic table data
//! - `lab`: the play space, bonding rules and bond-order solver
//! - `layout`: VSEPR-style 3D relaxation
//! - `naming`: common + IUPAC names (curated table, then generated)

pub mod canon;
pub mod compounds;
pub mod elements;
pub mod graph;
pub mod lab;
pub mod layout;
pub mod naming;
pub mod organic;
pub mod smiles;

pub use glam::Vec3;
pub use graph::{BondKind, MolGraph};
pub use lab::{AtomId, AttachMode, AttachPlan, BondId, Lab, Reject};
pub use naming::{Namer, Names};
