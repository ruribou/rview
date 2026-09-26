//! rview: organize git diffs into review-oriented categories.
//!
//! The pipeline is `git` → [`diff`] (parse) → [`category`] (classify) →
//! [`concern`] (detect signals) → [`report`] (aggregate and render).

pub mod category;
pub mod concern;
pub mod diff;
pub mod git;
pub mod report;
