//! Link codes: `7K2M-QX4P-9TRA`, 12 Crockford base32 characters. The first
//! group names the relay mailbox the two machines meet at; the whole code is
//! the SPAKE2 password, so the 40 bits the relay never sees are the secret.
//! Each wrong guess costs a live round trip and the mailbox closes after one
//! attempt, which is why a short code is enough.

use std::fmt;
use std::str::FromStr;

use rand_core::{OsRng, RngCore};

use crate::Error;

/// Crockford base32: no I, L, O or U, so it reads aloud and types cleanly.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const LEN: usize = 12;
const GROUP: usize = 4;

#[derive(Clone, PartialEq, Eq)]
pub struct LinkCode([u8; LEN]);

impl LinkCode {
    pub fn generate() -> Self {
        let mut code = [0u8; LEN];
        for c in &mut code {
            *c = ALPHABET[(OsRng.next_u32() % 32) as usize];
        }
        Self(code)
    }

    /// Where the two machines meet on the relay. Not secret.
    pub fn mailbox(&self) -> &str {
        self.as_str().split_at(GROUP).0
    }

    pub(crate) fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("code is ASCII")
    }
}

impl fmt::Display for LinkCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.as_str();
        write!(f, "{}-{}-{}", &s[..4], &s[4..8], &s[8..])
    }
}

/// Never print the secret part in logs.
impl fmt::Debug for LinkCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LinkCode({}-****-****)", self.mailbox())
    }
}

impl FromStr for LinkCode {
    type Err = Error;

    /// Forgiving about what people type: case, spaces, dashes, and the
    /// letters Crockford maps to digits (O to 0, I and L to 1).
    fn from_str(s: &str) -> Result<Self, Error> {
        let mut code = [0u8; LEN];
        let mut n = 0;
        for ch in s.chars() {
            if ch == '-' || ch.is_whitespace() {
                continue;
            }
            let c = match ch.to_ascii_uppercase() {
                'O' => b'0',
                'I' | 'L' => b'1',
                c if c.is_ascii() && ALPHABET.contains(&(c as u8)) => c as u8,
                _ => return Err(Error::BadCode(format!("'{ch}' isn't used in link codes"))),
            };
            if n == LEN {
                return Err(Error::BadCode(format!("a link code has {LEN} characters")));
            }
            code[n] = c;
            n += 1;
        }
        if n != LEN {
            return Err(Error::BadCode(format!("a link code has {LEN} characters")));
        }
        Ok(Self(code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_codes_read_back() {
        let c = LinkCode::generate();
        let shown = c.to_string();
        assert_eq!(shown.len(), 14);
        assert_eq!(shown.parse::<LinkCode>().unwrap(), c);
        assert_eq!(c.mailbox(), &shown[..4]);
        assert_ne!(LinkCode::generate(), c);
    }

    #[test]
    fn typing_is_forgiving() {
        let c: LinkCode = " 7k2m qx4p-9tra ".parse().unwrap();
        assert_eq!(c.to_string(), "7K2M-QX4P-9TRA");
        let c: LinkCode = "OIL0-0000-0000".parse().unwrap();
        assert_eq!(c.to_string(), "0110-0000-0000");
    }

    #[test]
    fn bad_codes_are_refused() {
        assert!("7K2M-QX4P".parse::<LinkCode>().is_err());
        assert!("7K2M-QX4P-9TRA-1".parse::<LinkCode>().is_err());
        assert!("7K2M-QX4P-9TRU".parse::<LinkCode>().is_err());
        assert!("7K2M-QX4P-9TR!".parse::<LinkCode>().is_err());
    }

    #[test]
    fn debug_hides_the_secret() {
        let c: LinkCode = "7K2M-QX4P-9TRA".parse().unwrap();
        assert_eq!(format!("{c:?}"), "LinkCode(7K2M-****-****)");
    }
}
