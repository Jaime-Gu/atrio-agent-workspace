//! Operating-system adapters used by the shared Host.
//!
//! Shared policy, approval, revision, and persistence code calls narrow
//! platform surfaces; platform modules must not silently substitute mock
//! implementations.

#[cfg(windows)]
pub(crate) mod windows;

#[cfg(unix)]
pub(crate) mod macos;

#[cfg(windows)]
pub(crate) use windows::storage::commit_replace;

#[cfg(unix)]
pub(crate) use macos::storage::commit_replace;
