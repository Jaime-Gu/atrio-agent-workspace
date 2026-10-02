//! Windows process, IPC, physical identity, lock and storage implementations.
//! Shared Host logic owns approval, revision and policy semantics.

pub(crate) mod ipc;
pub(crate) mod locks;
pub(crate) mod paths;
pub(crate) mod process;
pub(crate) mod storage;
