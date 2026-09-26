//! rview: organize git diffs into review-oriented categories.
//!
//! The pipeline is:
//!
//! 1. [`analysis::collect`] runs git and parses its output
//!    ([`diff`], [`patch`]), then compares manifests ([`deps`]) and the DB
//!    schema ([`schema`]) between both sides.
//! 2. [`report::Report::build`] classifies files ([`category`]) and
//!    detects concerns ([`concern`], [`migration`]).
//! 3. The report is rendered as text or JSON.

pub mod analysis;
pub mod category;
pub mod concern;
pub mod deps;
pub mod diff;
pub mod git;
pub mod migration;
pub mod patch;
pub mod report;
pub mod schema;
