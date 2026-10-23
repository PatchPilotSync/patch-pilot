//! Plug-in, DAW and vendor-app scanners.
//!
//! Metadata only: never load a plug-in binary in-process (CLAUDE.md rule 1).

mod enumerate;
mod roots;

pub use enumerate::{Candidate, Enumeration, WalkWarning, enumerate};
pub use roots::{PluginRoot, SystemDirs, default_roots, path_map};
