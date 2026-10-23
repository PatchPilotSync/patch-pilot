//! Plug-in, DAW and vendor-app scanners.
//!
//! Metadata only: never load a plug-in binary in-process (CLAUDE.md rule 1).

mod au;
mod enumerate;
mod error;
mod pe;
mod plist_info;
mod roots;
mod vst3;

pub use au::{AuComponent, AuInfo, read_au};
pub use enumerate::{Candidate, Enumeration, WalkWarning, enumerate};
pub use error::ReadError;
pub use pe::{PeVersion, read_pe_version};
pub use plist_info::{BundleInfo, read_bundle_info};
pub use roots::{PluginRoot, SystemDirs, default_roots, path_map};
pub use vst3::{Vst3Class, Vst3Info, Vst3Source, read_vst3};
