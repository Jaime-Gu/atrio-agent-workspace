//! Preserve Unix same-directory rename followed by directory synchronization.
use std::{io, path::Path};

pub(crate) fn commit_replace(temp: &Path, target: &Path) -> io::Result<()> {
    std::fs::rename(temp, target)?;
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "replacement has no parent"))?;
    std::fs::File::open(parent)?.sync_all()
}
