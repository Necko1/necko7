//! Channel-owned automation. User code only runs on bounded blocking workers.
pub mod api;
pub mod history;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod documentation_tests;
#[cfg(test)]
mod access_tests;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod match_tests;
#[cfg(test)]
mod message_filter_tests;
#[cfg(test)]
mod filter_window_tests;
pub mod matches;
pub mod runtime;
pub mod service;
#[cfg(test)]
mod tests;
pub mod worker;
