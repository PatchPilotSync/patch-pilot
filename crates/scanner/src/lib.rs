//! Plug-in, DAW and vendor-app scanners.
//!
//! Metadata only: never load a plug-in binary in-process (CLAUDE.md rule 1).

mod au;
mod enumerate;
mod error;
mod names;
mod pe;
mod plist_info;
mod roots;
mod vendor_apps;
mod vst2_aax;
mod vst3;

pub use au::{AuComponent, AuInfo, read_au};
pub use enumerate::{Candidate, Enumeration, WalkWarning, enumerate};
pub use error::ReadError;
pub use names::{Channels, NormalizedName, normalize_name};
pub use pe::{PeVersion, read_pe_version};
pub use plist_info::{BundleInfo, read_bundle_info};
pub use roots::{PluginRoot, SystemDirs, default_roots, path_map};
#[cfg(windows)]
pub use vendor_apps::read_uninstall_entries;
pub use vendor_apps::{
    ADAPTERS, Detection, UninstallEntry, VendorAdapter, VendorAppStatus, detect_macos,
    detect_windows,
};
pub use vst2_aax::{MetaSource, PluginMeta, read_aax, read_vst2};
pub use vst3::{Vst3Class, Vst3Info, Vst3Source, read_vst3};
