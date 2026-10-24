//! Authenticated encryption for everything that leaves the machine:
//! `[version][24-byte nonce][XChaCha20-Poly1305 ciphertext]`.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand_core::{OsRng, RngCore};

use crate::{Error, Result};

const VERSION: u8 = 1;
const NONCE: usize = 24;

/// Encrypt `plaintext`, binding it to `aad` (where it may be stored), so a
/// blob moved to another name or rig fails to open.
pub(crate) fn seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let mut nonce = [0u8; NONCE];
    OsRng.fill_bytes(&mut nonce);
    let ct = XChaCha20Poly1305::new(key.into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("encryption of an in-memory buffer cannot fail");
    let mut out = Vec::with_capacity(1 + NONCE + ct.len());
    out.push(VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    out
}

pub(crate) fn open(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
    let (&version, rest) = sealed.split_first().ok_or(Error::Decrypt)?;
    if version > VERSION {
        return Err(Error::UnsupportedEnvelope(version));
    }
    if version != VERSION || rest.len() < NONCE {
        return Err(Error::Decrypt);
    }
    let (nonce, ct) = rest.split_at(NONCE);
    XChaCha20Poly1305::new(key.into())
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad })
        .map_err(|_| Error::Decrypt)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7; 32];

    #[test]
    fn round_trip_with_a_fresh_nonce_each_time() {
        let a = seal(&KEY, b"here", b"hello");
        let b = seal(&KEY, b"here", b"hello");
        assert_ne!(a, b);
        assert_eq!(open(&KEY, b"here", &a).unwrap(), b"hello");
    }

    #[test]
    fn tampering_wrong_place_or_wrong_key_fail() {
        let mut s = seal(&KEY, b"here", b"hello");
        assert!(matches!(open(&KEY, b"there", &s), Err(Error::Decrypt)));
        assert!(matches!(open(&[8; 32], b"here", &s), Err(Error::Decrypt)));
        *s.last_mut().unwrap() ^= 1;
        assert!(matches!(open(&KEY, b"here", &s), Err(Error::Decrypt)));
        assert!(matches!(open(&KEY, b"here", &[]), Err(Error::Decrypt)));
        assert!(matches!(open(&KEY, b"here", &[1, 2]), Err(Error::Decrypt)));
    }

    #[test]
    fn newer_versions_say_so() {
        let mut s = seal(&KEY, b"here", b"hello");
        s[0] = 2;
        assert!(matches!(
            open(&KEY, b"here", &s),
            Err(Error::UnsupportedEnvelope(2))
        ));
    }
}
