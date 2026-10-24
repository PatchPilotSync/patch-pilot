//! Linking a new machine to a rig with a [`LinkCode`].
//!
//! 1. The linked machine makes a code, shows it, and calls [`start`].
//! 2. The new machine is given the code and calls [`start`] too.
//! 3. Each posts its message to the code's mailbox on the relay and calls
//!    [`Linking::finish`] with the other's. Both now share a key, if and only
//!    if they used the same code (SPAKE2; the relay learns nothing usable).
//! 4. The linked machine sends [`LinkSession::offer`]; the new machine opens it
//!    with [`LinkSession::accept`] and answers with [`LinkSession::confirm`],
//!    which the linked machine checks with [`LinkSession::check_confirm`].
//!
//! A wrong code shows up as [`Error::WrongCode`] on both sides.

use spake2::{Ed25519Group, Identity, Password, Spake2};

use crate::{Error, LinkCode, Result, RigKey, envelope};

const IDENTITY: &[u8] = b"Patch Pilot link v1";
const KEY_CONTEXT: &str = "Patch Pilot 2026-10-24 link key v1";
const OFFER_AAD: &[u8] = b"Patch Pilot link offer v1";
const CONFIRM_AAD: &[u8] = b"Patch Pilot link confirm v1";
const CONFIRM_TEXT: &[u8] = b"linked";

/// One side of a handshake in progress. Single use.
pub struct Linking(Spake2<Ed25519Group>);

/// Begin linking with `code`. Returns the message to post to the mailbox.
pub fn start(code: &LinkCode) -> (Linking, Vec<u8>) {
    let (state, msg) = Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(code.as_str().as_bytes()),
        &Identity::new(IDENTITY),
    );
    (Linking(state), msg)
}

impl Linking {
    /// Finish with the other machine's message.
    pub fn finish(self, theirs: &[u8]) -> Result<LinkSession> {
        let shared = self.0.finish(theirs).map_err(|_| Error::Handshake)?;
        Ok(LinkSession(blake3::derive_key(KEY_CONTEXT, &shared)))
    }
}

/// The key both machines share for the rest of the handshake.
pub struct LinkSession([u8; 32]);

impl LinkSession {
    /// Linked machine: hand the rig key to the new machine.
    pub fn offer(&self, rig: &RigKey) -> Vec<u8> {
        envelope::seal(&self.0, OFFER_AAD, rig.as_bytes())
    }

    /// New machine: take the rig key.
    pub fn accept(&self, offer: &[u8]) -> Result<RigKey> {
        let key = envelope::open(&self.0, OFFER_AAD, offer).map_err(wrong_code)?;
        let key: [u8; 32] = key.try_into().map_err(|_| Error::Handshake)?;
        Ok(RigKey::from_bytes(key))
    }

    /// New machine: tell the linked machine it worked.
    pub fn confirm(&self) -> Vec<u8> {
        envelope::seal(&self.0, CONFIRM_AAD, CONFIRM_TEXT)
    }

    /// Linked machine: did the new machine get the key?
    pub fn check_confirm(&self, confirm: &[u8]) -> Result<()> {
        match envelope::open(&self.0, CONFIRM_AAD, confirm).map_err(wrong_code)? {
            t if t == CONFIRM_TEXT => Ok(()),
            _ => Err(Error::Handshake),
        }
    }
}

fn wrong_code(e: Error) -> Error {
    match e {
        Error::Decrypt => Error::WrongCode,
        e => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handshake(a: &LinkCode, b: &LinkCode) -> (LinkSession, LinkSession) {
        let (pc, pc_msg) = start(a);
        let (mac, mac_msg) = start(b);
        (pc.finish(&mac_msg).unwrap(), mac.finish(&pc_msg).unwrap())
    }

    #[test]
    fn same_code_hands_over_the_rig_key() {
        let rig = RigKey::generate();
        let code = LinkCode::generate();
        let typed: LinkCode = code.to_string().to_lowercase().parse().unwrap();
        let (pc, mac) = handshake(&code, &typed);

        let got = mac.accept(&pc.offer(&rig)).unwrap();
        assert_eq!(got, rig);
        pc.check_confirm(&mac.confirm()).unwrap();
    }

    #[test]
    fn wrong_code_fails_on_both_sides() {
        let code = LinkCode::generate();
        let (pc, mac) = handshake(&code, &LinkCode::generate());
        assert!(matches!(
            mac.accept(&pc.offer(&RigKey::generate())),
            Err(Error::WrongCode)
        ));
        assert!(matches!(
            pc.check_confirm(&mac.confirm()),
            Err(Error::WrongCode)
        ));
    }

    #[test]
    fn offer_and_confirm_cannot_be_swapped() {
        let code = LinkCode::generate();
        let (pc, mac) = handshake(&code, &code);
        assert!(pc.check_confirm(&pc.offer(&RigKey::generate())).is_err());
        assert!(mac.accept(&mac.confirm()).is_err());
    }

    #[test]
    fn garbage_messages_are_refused() {
        let (pc, _) = start(&LinkCode::generate());
        assert!(matches!(pc.finish(b"nope"), Err(Error::Handshake)));
    }
}
