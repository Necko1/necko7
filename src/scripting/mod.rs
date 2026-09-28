//! Channel-owned automation. User code only runs on bounded blocking workers.
pub mod api;
#[cfg(test)]
mod match_tests;
pub mod matches;
pub mod runtime;
pub mod service;
#[cfg(test)]
mod tests;
pub mod worker;
