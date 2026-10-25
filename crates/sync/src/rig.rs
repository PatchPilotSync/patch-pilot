//! The rig key: one 32-byte secret shared by every linked machine. Everything
//! else is derived from it with BLAKE3, each use under its own context.

use std::fmt;
use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use patchpilot_core::{Bundle, ContentHash};
use rand_core::{OsRng, RngCore};

use crate::{Error, Result, envelope};

const ID_CONTEXT: &str = "Patch Pilot 2026-10-24 rig id v1";
const TOKEN_CONTEXT: &str = "Patch Pilot 2026-10-24 relay token v1";
const ENCRYPT_CONTEXT: &str = "Patch Pilot 2026-10-24 rig encryption v1";
const FILE_CONTEXT: &str = "Patch Pilot 2026-10-24 file name v1";
const STORED_PREFIX: &str = "pprig1:";
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
/// Unpacked size cap, so a corrupt object can't fill memory.
const MAX_BUNDLE_JSON: u64 = 256 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct RigKey([u8; 32]);

impl RigKey {
    /// A new rig, made by the first machine that turns sync on.
    pub fn generate() -> Self {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        Self(key)
    }

    pub(crate) fn from_bytes(key: [u8; 32]) -> Self {
        Self(key)
    }

    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// What the relay files this rig's objects under. Derived one way from the
    /// key, so it reveals nothing about it.
    pub fn rig_id(&self) -> String {
        hex(&blake3::derive_key(ID_CONTEXT, &self.0)[..16])
    }

    /// Sent to the relay to prove this machine belongs to the rig. The relay
    /// keeps only a hash of it; it can't decrypt anything with it.
    pub fn relay_token(&self) -> String {
        hex(&blake3::derive_key(TOKEN_CONTEXT, &self.0))
    }

    /// The text form kept in the app-data directory.
    pub fn to_stored(&self) -> String {
        format!("{STORED_PREFIX}{}", hex(&self.0))
    }

    pub fn from_stored(s: &str) -> Result<Self> {
        let h = s.trim().strip_prefix(STORED_PREFIX).ok_or(Error::BadKey)?;
        if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::BadKey);
        }
        let mut key = [0u8; 32];
        for (i, b) in key.iter_mut().enumerate() {
            *b = u8::from_str_radix(&h[2 * i..2 * i + 2], 16).map_err(|_| Error::BadKey)?;
        }
        Ok(Self(key))
    }

    /// Encrypt a bundle for storing under `object` on the relay. The JSON is
    /// gzipped first: a rig with thousands of ReaPack scripts is several MB of
    /// JSON, over the relay's 2 MB object limit, and compresses about 10x.
    pub fn seal_bundle(&self, object: &str, bundle: &Bundle) -> Result<Vec<u8>> {
        check_object(object)?;
        let json = serde_json::to_vec(bundle).expect("bundle serializes");
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(&json).expect("writing to memory cannot fail");
        let packed = gz.finish().expect("writing to memory cannot fail");
        Ok(envelope::seal(
            &self.encryption_key(),
            &self.aad(object),
            &packed,
        ))
    }

    /// Decrypt and parse what the relay holds under `object`. Refuses data
    /// from another rig, data moved from another name, and newer formats.
    pub fn open_bundle(&self, object: &str, sealed: &[u8]) -> Result<Bundle> {
        check_object(object)?;
        let plain = envelope::open(&self.encryption_key(), &self.aad(object), sealed)?;
        // Builds before 0.3.1 sealed plain JSON.
        let json = if plain.starts_with(&GZIP_MAGIC) {
            let mut json = Vec::new();
            GzDecoder::new(&plain[..])
                .take(MAX_BUNDLE_JSON)
                .read_to_end(&mut json)
                .map_err(|_| Error::Decrypt)?;
            json
        } else {
            plain
        };
        let json = std::str::from_utf8(&json).map_err(|_| Error::Decrypt)?;
        Ok(Bundle::from_json(json)?)
    }

    /// The relay object holding one user-owned file (a template, an FX
    /// chain), by its content hash: `file/<keyed hash>`. Keyed with the rig,
    /// so the relay can't match the name against files it knows.
    pub fn file_object(&self, hash: &ContentHash) -> String {
        let key = blake3::derive_key(FILE_CONTEXT, &self.0);
        let name = blake3::keyed_hash(&key, hash.as_str().as_bytes());
        format!("file/{}", hex(&name.as_bytes()[..20]))
    }

    /// Encrypt a file's bytes for storing under [`Self::file_object`].
    pub fn seal_file(&self, hash: &ContentHash, bytes: &[u8]) -> Vec<u8> {
        let object = self.file_object(hash);
        envelope::seal(&self.encryption_key(), &self.aad(&object), bytes)
    }

    /// Decrypt a file and check it is the one `hash` names.
    pub fn open_file(&self, hash: &ContentHash, sealed: &[u8]) -> Result<Vec<u8>> {
        let object = self.file_object(hash);
        let bytes = envelope::open(&self.encryption_key(), &self.aad(&object), sealed)?;
        if content_hash(&bytes) != *hash {
            return Err(Error::Decrypt);
        }
        Ok(bytes)
    }

    fn encryption_key(&self) -> [u8; 32] {
        blake3::derive_key(ENCRYPT_CONTEXT, &self.0)
    }

    fn aad(&self, object: &str) -> Vec<u8> {
        format!("{}/{object}", self.rig_id()).into_bytes()
    }
}

/// Never print the key.
impl fmt::Debug for RigKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RigKey({})", self.rig_id())
    }
}

/// The relay object holding a machine's latest scan, Golden versions and
/// profiles: `machine/<machine id>`.
pub fn machine_object(machine_id: &str) -> Result<String> {
    let name = format!("machine/{machine_id}");
    check_object(&name)?;
    Ok(name)
}

/// Lowercase letters, digits, `-` and `/`-separated segments only, so object
/// names are safe as relay keys and URL paths on every platform.
fn check_object(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && name.split('/').all(|seg| {
            !seg.is_empty()
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
    if ok {
        Ok(())
    } else {
        Err(Error::BadObject(name.to_owned()))
    }
}

/// The hash a manifest records for a user-owned file's bytes.
pub fn content_hash(bytes: &[u8]) -> ContentHash {
    ContentHash::try_from(format!("blake3:{}", blake3::hash(bytes).to_hex()))
        .expect("a BLAKE3 hex digest is a valid content hash")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::{BundleMachine, Manifest};

    fn bundle() -> Bundle {
        let json = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/parity/score/laptop.json"
        ))
        .unwrap();
        let mut b = Bundle::new(1_760_000_000, Some("Laptop".into()));
        b.machines.push(BundleMachine {
            manifest: Manifest::from_json(&json).unwrap(),
            profile_id: None,
        });
        b
    }

    #[test]
    fn a_bundle_round_trips_through_the_relay_form() {
        let rig = RigKey::generate();
        let name = machine_object("3f2a9c1e5b7d4e8a").unwrap();
        let sealed = rig.seal_bundle(&name, &bundle()).unwrap();
        assert_eq!(rig.open_bundle(&name, &sealed).unwrap(), bundle());
        // Encrypted: no plug-in names in what the relay sees.
        let laptop = String::from_utf8_lossy(&sealed).into_owned();
        assert!(!laptop.contains("Laptop"));
    }

    #[test]
    fn thousands_of_reapack_scripts_fit_the_relay_limit() {
        let mut b = bundle();
        let m = &mut b.machines[0].manifest;
        let template = m.items[0].clone();
        for n in 0..9000 {
            let id = patchpilot_core::ProductId(format!(
                "reapack/reateam-scripts/various/script-{n}-do-something-useful-lua"
            ));
            let mut item = template.clone();
            item.product_id = Some(id.clone());
            item.path = patchpilot_core::PortablePath::logical(
                "REAPACK",
                format!("ReaTeam Scripts/Various/Script {n}: do something useful.lua"),
            );
            m.items.push(item);
            let mut p = m.products[0].clone();
            p.canonical_id = id;
            p.name = format!("Script {n}: do something useful");
            m.products.push(p);
        }
        let json = serde_json::to_vec(&b).unwrap().len();
        let rig = RigKey::generate();
        let sealed = rig.seal_bundle("machine/a1", &b).unwrap();
        assert!(json > 2_000_000, "{json}");
        assert!(sealed.len() < 2_000_000 / 4, "{}", sealed.len());
        assert_eq!(rig.open_bundle("machine/a1", &sealed).unwrap(), b);
    }

    #[test]
    fn objects_from_older_builds_still_open() {
        let rig = RigKey::generate();
        let json = serde_json::to_vec(&bundle()).unwrap();
        let old = envelope::seal(&rig.encryption_key(), &rig.aad("machine/a1"), &json);
        assert_eq!(rig.open_bundle("machine/a1", &old).unwrap(), bundle());
    }

    #[test]
    fn another_rig_or_a_moved_object_cannot_open_it() {
        let rig = RigKey::generate();
        let sealed = rig.seal_bundle("machine/a1", &bundle()).unwrap();
        assert!(matches!(
            RigKey::generate().open_bundle("machine/a1", &sealed),
            Err(Error::Decrypt)
        ));
        assert!(matches!(
            rig.open_bundle("machine/b2", &sealed),
            Err(Error::Decrypt)
        ));
    }

    #[test]
    fn newer_bundles_are_refused_after_decrypting() {
        let rig = RigKey::generate();
        let mut b = bundle();
        b.bundle_version = patchpilot_core::BUNDLE_VERSION + 1;
        let sealed = rig.seal_bundle("machine/a1", &b).unwrap();
        assert!(matches!(
            rig.open_bundle("machine/a1", &sealed),
            Err(Error::Bundle(_))
        ));
    }

    #[test]
    fn derived_names_are_stable_and_distinct() {
        let rig = RigKey::from_bytes([1; 32]);
        assert_eq!(rig.rig_id(), RigKey::from_bytes([1; 32]).rig_id());
        assert_eq!(rig.rig_id().len(), 32);
        assert_eq!(rig.relay_token().len(), 64);
        assert!(!rig.relay_token().starts_with(&rig.rig_id()));
        assert_ne!(rig.rig_id(), RigKey::from_bytes([2; 32]).rig_id());
        assert!(!rig.to_stored().contains(&rig.relay_token()));
    }

    #[test]
    fn stored_form_round_trips_and_rejects_junk() {
        let rig = RigKey::generate();
        assert_eq!(RigKey::from_stored(&rig.to_stored()).unwrap(), rig);
        assert_eq!(
            RigKey::from_stored(&format!("{}\n", rig.to_stored())).unwrap(),
            rig
        );
        for bad in ["", "pprig1:", "pprig1:zz", &rig.to_stored()[1..]] {
            assert!(RigKey::from_stored(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn debug_never_shows_the_key() {
        let rig = RigKey::from_bytes([0xab; 32]);
        assert!(!format!("{rig:?}").contains("abab"));
    }

    #[test]
    fn files_round_trip_by_hash_and_names_hide_the_hash() {
        let rig = RigKey::generate();
        let bytes = b"<TRACK\n<FXCHAIN\n>\n>\n";
        let hash = content_hash(bytes);
        let object = rig.file_object(&hash);
        assert!(check_object(&object).is_ok());
        assert!(!object.contains(&hash.as_str()["blake3:".len()..][..16]));
        assert_ne!(object, RigKey::generate().file_object(&hash));
        let sealed = rig.seal_file(&hash, bytes);
        assert_eq!(rig.open_file(&hash, &sealed).unwrap(), bytes);
        assert!(!String::from_utf8_lossy(&sealed).contains("FXCHAIN"));
        // Another rig, or a file stored under another hash, fails.
        assert!(RigKey::generate().open_file(&hash, &sealed).is_err());
        let other = content_hash(b"other");
        assert!(rig.open_file(&other, &sealed).is_err());
    }

    #[test]
    fn object_names_are_checked() {
        assert!(machine_object("3f2a9c1e5b7d4e8a").is_ok());
        for bad in ["", "../x", "A", "a b", "a/"] {
            assert!(machine_object(bad).is_err(), "{bad}");
        }
    }
}
