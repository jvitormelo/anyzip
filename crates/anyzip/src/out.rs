//! Where engines deliver entries: one place decides what is skipped and why.

use std::ops::ControlFlow;

use crate::path;
use crate::{Entry, ListedEntry, SkipReason, Skipped};

/// What an engine found in a header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    File,
    Dir,
    Symlink,
    Encrypted,
    /// Hard link, device, fifo, socket, file-copy reference.
    Other,
}

pub(crate) struct Out<'a> {
    list_only: bool,
    pub(crate) listed: Vec<ListedEntry>,
    pub(crate) skipped: Vec<Skipped>,
    pub(crate) recovered: bool,
    pub(crate) emitted: usize,
    on_entry: &'a mut dyn FnMut(Entry) -> ControlFlow<()>,
}

impl<'a> Out<'a> {
    pub(crate) fn new(
        list_only: bool,
        on_entry: &'a mut dyn FnMut(Entry) -> ControlFlow<()>,
    ) -> Self {
        Self {
            list_only,
            listed: Vec::new(),
            skipped: Vec::new(),
            recovered: false,
            emitted: 0,
            on_entry,
        }
    }

    pub(crate) fn list_only(&self) -> bool {
        self.list_only
    }

    /// Decides an entry from its header. `Some(path)` means: decompress it now and call
    /// [`Out::entry`]. Directories vanish; everything else not extracted is recorded as skipped.
    pub(crate) fn admit(&mut self, name: String, kind: Kind, declared: u64) -> Option<String> {
        let reason = match kind {
            Kind::Dir => return None,
            Kind::Symlink => SkipReason::Symlink,
            Kind::Encrypted => SkipReason::Encrypted,
            Kind::Other => SkipReason::NotAFile,
            Kind::File => match path::normalize(&name) {
                None => SkipReason::UnsafePath,
                Some(path) if self.list_only => {
                    self.listed.push(ListedEntry {
                        path,
                        size: declared,
                    });
                    return None;
                }
                Some(path) => return Some(path),
            },
        };
        self.skipped.push(Skipped { path: name, reason });
        None
    }

    pub(crate) fn entry(&mut self, path: String, data: Vec<u8>) -> ControlFlow<()> {
        self.emitted += 1;
        (self.on_entry)(Entry { path, data })
    }
}
