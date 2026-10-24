//! Live sync core (decision #38): everything a machine needs to share its
//! scans through the relay without the relay being able to read them.
//!
//! - [`RigKey`]: the secret every linked machine holds. Names the rig on the
//!   relay, proves membership to it and encrypts every object.
//! - [`LinkCode`]: the short code a linked machine shows so a new one can join.
//! - [`link`]: the handshake that turns a link code into the rig key on the new
//!   machine (SPAKE2, so the relay never learns the key and a wrong guess
//!   costs an attacker a round trip, not an offline search).
//!
//! No network here: the app and the relay move the bytes.

mod code;
mod envelope;
pub mod link;
mod rig;

pub use code::LinkCode;
pub use rig::{RigKey, machine_object};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a link code: {0}")]
    BadCode(String),
    #[error("that link code doesn't match, or the other machine stopped linking")]
    WrongCode,
    #[error("couldn't decrypt: the data was changed or belongs to another rig")]
    Decrypt,
    #[error("sync data version {0} is newer than this Patch Pilot; update it")]
    UnsupportedEnvelope(u8),
    #[error("bad link message from the other machine")]
    Handshake,
    #[error("not a rig key")]
    BadKey,
    #[error("not a valid object name: {0}")]
    BadObject(String),
    #[error(transparent)]
    Bundle(#[from] patchpilot_core::ManifestError),
}

pub type Result<T> = std::result::Result<T, Error>;
