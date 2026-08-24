//! Planning domain modules.
//!
//! The orchestration remains in planner.rs for now. Pure deterministic
//! planning rules live in pure.rs so they can be tested without an AI service
//! or filesystem state.

pub(crate) mod constraints;
pub(crate) mod parsing;
pub(crate) mod pure;
