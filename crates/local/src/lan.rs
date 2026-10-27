//! Sync over the home network (decision #94). Instead of a cloud folder or
//! GitHub, the backup and keep-in-sync folder (decisions #67 and #70) is a
//! copy in app-data, and linked machines on the same network copy each
//! other's folders straight from one another: no cloud, no server, nothing
//! leaves the network. Backup, restore and keeping files in sync work on the
//! copy exactly as on a cloud folder; only how files travel changes.
//!
//! How machines find each other: every few seconds each one broadcasts a
//! small beacon on UDP [`PORT`], sealed with the rig key, saying who it is,
//! which TCP port it answers on and a short [`state`] of its copy. A machine
//! that hears a linked machine's beacon with a state it hasn't pulled asks
//! that machine for any folder it holds a newer backup of, over TCP, every
//! request and answer sealed with the rig key too. A stranger on the
//! network can't read a beacon, ask for a file, or read an answer.
//!
//! Each machine changes only its own folder, so a folder's newest backup
//! stamp says which copy is newer: a machine pulls a folder only from a copy
//! with a newer backup, and then makes its own copy exactly that one. A
//! backup's index is written last, so a backup never shows before its files.
//!
//! No threads or sockets kept here: the app runs the beacons and the server.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use patchpilot_sync::RigKey;
use serde::{Deserialize, Serialize};

use crate::backup::{
    FOLDER_NAME, below, own_folder, remove_empty_parents, safe_rel, stamps, subdirs, write_atomic,
};
use crate::github::{FileIndex, blob_id, local_files};

/// [`crate::backup::BackupSettings::service`] while syncing over the home network.
pub const SERVICE: &str = "lan";
const MIRROR: &str = "Home network";
const INDEX_FILE: &str = "lan-index.json";
/// Beacons go to this UDP port, and the TCP server takes it when it's free.
pub const PORT: u16 = 47219;
const MAGIC: &[u8] = b"PPLAN1";
pub const MAX_BEACON: usize = 2048;
const MAX_REQUEST: u32 = 64 * 1024;
/// Backups hold files up to 64 MB; room for that, sealed.
const MAX_ANSWER: u32 = 128 * 1024 * 1024;
/// How long a quiet connection waits before giving up.
pub const TIMEOUT: Duration = Duration::from_secs(30);
const BACKUPS: &str = "Backups";

/// The folder to set as the backup folder: `Patch Pilot Backup` inside it is
/// this machine's copy.
pub fn backup_folder(app_dir: &Path) -> PathBuf {
    app_dir.join(MIRROR)
}

/// The copy: what [`crate::backup::backup_dir`] gives for [`backup_folder`].
pub fn mirror_dir(app_dir: &Path) -> PathBuf {
    backup_folder(app_dir).join(FOLDER_NAME)
}

pub fn load_index(app_dir: &Path) -> FileIndex {
    std::fs::read(app_dir.join(INDEX_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_index(app_dir: &Path, index: &FileIndex) -> Result<(), String> {
    let json = serde_json::to_vec(index).expect("index serializes");
    write_atomic(&app_dir.join(INDEX_FILE), &json)
}

/// What a machine broadcasts. Mirrored nowhere: it never reaches the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Beacon {
    pub machine_id: String,
    pub machine_name: String,
    /// The TCP port it answers on.
    pub port: u16,
    /// Its copy's [`state`].
    pub state: String,
}

pub fn beacon_bytes(key: &RigKey, beacon: &Beacon) -> Vec<u8> {
    let json = serde_json::to_vec(beacon).expect("beacon serializes");
    let mut out = MAGIC.to_vec();
    out.extend(key.seal_lan("beacon", &json));
    out
}

/// A linked machine's beacon; `None` for anything else on the port.
pub fn read_beacon(key: &RigKey, bytes: &[u8]) -> Option<Beacon> {
    let sealed = bytes.strip_prefix(MAGIC)?;
    serde_json::from_slice(&key.open_lan("beacon", sealed).ok()?).ok()
}

/// The copy in a few characters: it changes whenever any folder gets a new
/// backup. Reads folder names only, so it's cheap to work out every beacon.
pub fn state(dir: &Path) -> String {
    let mut h = blake3::Hasher::new();
    for f in folder_stamps(dir) {
        h.update(f.0.as_bytes());
        h.update(b"\0");
        h.update(f.1.as_bytes());
        h.update(b"\n");
    }
    h.finalize().to_hex()[..16].to_owned()
}

/// Each folder in the copy with its newest backup's stamp.
fn folder_stamps(dir: &Path) -> Vec<(String, String)> {
    subdirs(dir)
        .into_iter()
        .filter_map(|f| {
            let latest = stamps(&f).pop()?;
            Some((f.file_name()?.to_string_lossy().into_owned(), latest))
        })
        .collect()
}

/// One machine's folder in a copy, and its newest backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderState {
    pub folder: String,
    pub machine_id: String,
    pub machine_name: String,
    /// The newest backup's stamp; later stamps sort later.
    pub latest: String,
}

/// Every folder in the copy that holds a backup.
pub fn folder_states(dir: &Path) -> Vec<FolderState> {
    folder_stamps(dir)
        .into_iter()
        .filter_map(|(folder, latest)| {
            let head = head(&dir.join(&folder), &latest)?;
            Some(FolderState {
                folder,
                machine_id: head.machine_id,
                machine_name: head.machine_name,
                latest,
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct Head {
    machine_id: String,
    machine_name: String,
}

fn head(folder: &Path, stamp: &str) -> Option<Head> {
    let bytes = std::fs::read(folder.join(BACKUPS).join(format!("{stamp}.json"))).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "ask", rename_all = "snake_case")]
enum Ask {
    Folders,
    /// Every file in one folder, with its blob ID.
    Files {
        folder: String,
    },
    /// One file's bytes, by its path in the copy.
    File {
        path: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Request {
    /// Binds the answer to this request.
    id: String,
    #[serde(flatten)]
    ask: Ask,
}

const OK: u8 = 0;
const REFUSED: u8 = 1;

fn read_frame(stream: &mut impl Read, max: u32) -> Result<Option<Vec<u8>>, String> {
    let mut len = [0u8; 4];
    match stream.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.to_string()),
    }
    let len = u32::from_be_bytes(len);
    if len > max {
        return Err("message too big".into());
    }
    let mut body = vec![0u8; len as usize];
    stream.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok(Some(body))
}

fn write_frame(stream: &mut impl Write, body: &[u8]) -> Result<(), String> {
    let len = u32::try_from(body.len()).map_err(|_| "message too big")?;
    stream
        .write_all(&len.to_be_bytes())
        .and_then(|()| stream.write_all(body))
        .and_then(|()| stream.flush())
        .map_err(|e| e.to_string())
}

/// Answer a linked machine's requests on `stream` until it hangs up, from
/// the copy at `dir`. Anything that isn't a request sealed with `key` ends
/// the connection unanswered. Only reads: nothing a request says changes
/// this machine.
pub fn answer(
    stream: &mut (impl Read + Write),
    key: &RigKey,
    dir: &Path,
    index: &Mutex<FileIndex>,
) -> Result<(), String> {
    while let Some(sealed) = read_frame(stream, MAX_REQUEST)? {
        let plain = key
            .open_lan("request", &sealed)
            .map_err(|_| "not from a linked machine")?;
        let req: Request = serde_json::from_slice(&plain).map_err(|e| e.to_string())?;
        let mut out = vec![OK];
        match reply(&req.ask, dir, index) {
            Ok(body) => out.extend(body),
            Err(e) => {
                out[0] = REFUSED;
                out.extend(e.into_bytes());
            }
        }
        write_frame(stream, &key.seal_lan(&format!("answer/{}", req.id), &out))?;
    }
    Ok(())
}

fn reply(ask: &Ask, dir: &Path, index: &Mutex<FileIndex>) -> Result<Vec<u8>, String> {
    match ask {
        Ask::Folders => Ok(serde_json::to_vec(&folder_states(dir)).expect("serializes")),
        Ask::Files { folder } => {
            if !safe_rel(folder) || folder.contains('/') {
                return Err("not a folder in the backup".into());
            }
            // Read without holding the index, so a pull here can go on.
            let mut idx = index.lock().map_err(|e| e.to_string())?.clone();
            let all = local_files(dir, &mut idx)?;
            if let Ok(mut shared) = index.lock() {
                *shared = idx;
            }
            let prefix = format!("{folder}/");
            let files: BTreeMap<&String, &String> =
                all.iter().filter(|(p, _)| p.starts_with(&prefix)).collect();
            Ok(serde_json::to_vec(&files).expect("serializes"))
        }
        Ask::File { path } => {
            if !safe_rel(path) || path.ends_with(".patchpilot-partial") {
                return Err("not a file in the backup".into());
            }
            let file = below(dir, path);
            let meta = std::fs::metadata(&file).map_err(|_| format!("{path} isn't there now"))?;
            if !meta.is_file() || meta.len() > u64::from(MAX_ANSWER) - 1024 {
                return Err(format!("{path} can't be sent"));
            }
            std::fs::read(&file).map_err(|e| format!("{path}: {e}"))
        }
    }
}

/// A copy to pull from: a linked machine on the network ([`Connection`]),
/// or anything that answers the same questions.
pub trait Source {
    fn folders(&mut self) -> Result<Vec<FolderState>, String>;
    /// Every file in `folder`, by path in the copy, with its blob ID.
    fn files(&mut self, folder: &str) -> Result<BTreeMap<String, String>, String>;
    fn file(&mut self, path: &str) -> Result<Vec<u8>, String>;
}

/// One connection to a linked machine, for as many requests as a pull takes.
pub struct Connection {
    stream: TcpStream,
    key: RigKey,
}

impl Connection {
    pub fn open(addr: SocketAddr, key: &RigKey) -> Result<Self, String> {
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
            .map_err(|e| format!("Couldn't reach it on the network ({e})."))?;
        stream
            .set_read_timeout(Some(TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(TIMEOUT)))
            .and_then(|()| stream.set_nodelay(true))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            stream,
            key: key.clone(),
        })
    }

    fn ask(&mut self, ask: Ask) -> Result<Vec<u8>, String> {
        // A random ID: a fresh nonce, hashed.
        let id = blake3::hash(&self.key.seal_lan("id", b""))
            .to_hex()
            .to_string();
        let req = serde_json::to_vec(&Request {
            id: id.clone(),
            ask,
        })
        .expect("request serializes");
        let lost = |e: String| format!("The connection broke off ({e}).");
        write_frame(&mut self.stream, &self.key.seal_lan("request", &req)).map_err(lost)?;
        let sealed = read_frame(&mut self.stream, MAX_ANSWER)
            .map_err(lost)?
            .ok_or_else(|| lost("closed".into()))?;
        let mut plain = self
            .key
            .open_lan(&format!("answer/{id}"), &sealed)
            .map_err(|_| "The answer didn't come from a linked machine.".to_owned())?;
        match plain.first() {
            Some(&OK) => {
                plain.remove(0);
                Ok(plain)
            }
            Some(_) => Err(String::from_utf8_lossy(&plain[1..]).into_owned()),
            None => Err("An empty answer came back.".into()),
        }
    }
}

impl Source for Connection {
    fn folders(&mut self) -> Result<Vec<FolderState>, String> {
        serde_json::from_slice(&self.ask(Ask::Folders)?).map_err(|e| e.to_string())
    }

    fn files(&mut self, folder: &str) -> Result<BTreeMap<String, String>, String> {
        let body = self.ask(Ask::Files {
            folder: folder.to_owned(),
        })?;
        serde_json::from_slice(&body).map_err(|e| e.to_string())
    }

    fn file(&mut self, path: &str) -> Result<Vec<u8>, String> {
        self.ask(Ask::File {
            path: path.to_owned(),
        })
    }
}

/// What a pull brought. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PullReport {
    /// Machines whose folders came over, by name.
    pub machines: Vec<String>,
    pub copied: usize,
    /// Files gone from a folder there (moved to its `Versions`), gone here too.
    pub removed: usize,
}

/// How far a pull has got. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub from: String,
    pub done: usize,
    pub total: usize,
}

/// Bring every folder `source` has a newer backup of into the copy at
/// `dir`, each made exactly like `source`'s. This machine's own folder
/// (`this`) is never taken, unless the copy doesn't have it at all (a new
/// copy gets this machine's backups back). A folder here with the same name
/// but another machine's backups is left alone.
pub fn pull(
    source: &mut dyn Source,
    dir: &Path,
    this: &str,
    index: &mut FileIndex,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<PullReport, String> {
    let mut report = PullReport::default();
    let mine = own_folder(dir, this);
    let wanted: Vec<FolderState> = source
        .folders()?
        .into_iter()
        .filter(|f| safe_rel(&f.folder) && !f.folder.contains('/'))
        .filter(|f| f.machine_id != this || mine.is_none())
        .filter(|f| {
            let here = dir.join(&f.folder);
            match stamps(&here).pop() {
                None => true,
                Some(latest) => {
                    latest < f.latest
                        && head(&here, &latest).is_some_and(|h| h.machine_id == f.machine_id)
                }
            }
        })
        .collect();
    if wanted.is_empty() {
        return Ok(report);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let here = local_files(dir, index)?;
    for f in wanted {
        let prefix = format!("{}/", f.folder);
        let theirs: BTreeMap<String, String> = source
            .files(&f.folder)?
            .into_iter()
            .filter(|(p, _)| p.starts_with(&prefix) && safe_rel(p))
            .collect();
        // A backup's index last, so it never shows before its files.
        let backups = format!("{prefix}{BACKUPS}/");
        let mut need: Vec<(&String, &String)> = theirs
            .iter()
            .filter(|(p, id)| here.get(*p) != Some(id))
            .collect();
        need.sort_by_key(|(p, _)| (p.starts_with(&backups), p.as_str()));
        let total = need.len();
        progress(0, total);
        for (n, (path, id)) in need.into_iter().enumerate() {
            let body = source.file(path)?;
            if blob_id(&body) != *id {
                return Err(format!("{path} changed on the way; trying again later."));
            }
            let file = below(dir, path);
            write_atomic(&file, &body)?;
            index.note(path, &file, id);
            report.copied += 1;
            progress(n + 1, total);
        }
        for path in here.keys().filter(|p| p.starts_with(&prefix)) {
            if !theirs.contains_key(path) {
                let file = below(dir, path);
                if std::fs::remove_file(&file).is_ok() {
                    report.removed += 1;
                    remove_empty_parents(&file, dir);
                }
            }
        }
        report.machines.push(f.machine_name);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn put(dir: &Path, rel: &str, body: &str) {
        let p = below(dir, rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    /// A backup's index, as `backup::back_up` writes it (only what's read here).
    fn backed_up(dir: &Path, folder: &str, id: &str, stamp: &str) {
        put(
            dir,
            &format!("{folder}/Backups/{stamp}.json"),
            &format!(r#"{{"machine_id":"{id}","machine_name":"{folder}"}}"#),
        );
    }

    /// Another machine's copy, answering as it would over the network.
    struct Copy {
        dir: PathBuf,
        index: Mutex<FileIndex>,
        asked: Vec<String>,
    }

    impl Copy {
        fn new(dir: &Path) -> Self {
            Self {
                dir: dir.to_owned(),
                index: Mutex::default(),
                asked: Vec::new(),
            }
        }

        fn ask(&mut self, ask: Ask) -> Result<Vec<u8>, String> {
            if let Ask::File { path } = &ask {
                self.asked.push(path.clone());
            }
            reply(&ask, &self.dir, &self.index)
        }
    }

    impl Source for Copy {
        fn folders(&mut self) -> Result<Vec<FolderState>, String> {
            Ok(serde_json::from_slice(&self.ask(Ask::Folders)?).unwrap())
        }
        fn files(&mut self, folder: &str) -> Result<BTreeMap<String, String>, String> {
            let folder = folder.to_owned();
            Ok(serde_json::from_slice(&self.ask(Ask::Files { folder })?).unwrap())
        }
        fn file(&mut self, path: &str) -> Result<Vec<u8>, String> {
            let path = path.to_owned();
            self.ask(Ask::File { path })
        }
    }

    fn pull_from(source: &mut dyn Source, dir: &Path, this: &str) -> PullReport {
        pull(source, dir, this, &mut FileIndex::default(), &mut |_, _| {}).unwrap()
    }

    #[test]
    fn takes_newer_folders_and_never_this_machines_own() {
        let tmp = tempfile::tempdir().unwrap();
        let (pc, mac) = (tmp.path().join("pc"), tmp.path().join("mac"));
        // The PC's copy: its own folder, and an old copy of the Mac's.
        put(
            &pc,
            "Studio PC/Files/Reaper/FXChains/Vocal.RfxChain",
            "vocal",
        );
        backed_up(&pc, "Studio PC", "pc", "2026-10-27 10.00.00");
        put(&pc, "MacBook/Files/old.txt", "old");
        backed_up(&pc, "MacBook", "mac", "2026-10-26 10.00.00");
        // The Mac's copy: its own newer folder.
        put(&mac, "MacBook/Files/new.txt", "new");
        backed_up(&mac, "MacBook", "mac", "2026-10-27 09.00.00");

        let r = pull_from(&mut Copy::new(&pc), &mac, "mac");
        assert_eq!(r.machines, vec!["Studio PC".to_owned()]);
        assert_eq!(
            std::fs::read_to_string(mac.join("Studio PC/Files/Reaper/FXChains/Vocal.RfxChain"))
                .unwrap(),
            "vocal"
        );
        // The Mac's own folder wasn't touched by the PC's older copy.
        assert!(mac.join("MacBook/Files/new.txt").is_file());
        assert!(!mac.join("MacBook/Files/old.txt").exists());

        // The PC takes the Mac's newer folder and ends up exactly like it.
        let r = pull_from(&mut Copy::new(&mac), &pc, "pc");
        assert_eq!(r.machines, vec!["MacBook".to_owned()]);
        assert_eq!(r.removed, 2, "old.txt and the old backup's index");
        assert!(pc.join("MacBook/Files/new.txt").is_file());
        assert!(!pc.join("MacBook/Files/old.txt").exists());
        assert_eq!(state(&pc), state(&mac));

        // In step: nothing more to pull either way.
        assert_eq!(
            pull_from(&mut Copy::new(&mac), &pc, "pc"),
            PullReport::default()
        );
        assert_eq!(
            pull_from(&mut Copy::new(&pc), &mac, "mac"),
            PullReport::default()
        );
    }

    #[test]
    fn copies_only_changed_files_and_the_index_last() {
        let tmp = tempfile::tempdir().unwrap();
        let (pc, mac) = (tmp.path().join("pc"), tmp.path().join("mac"));
        put(&pc, "Studio PC/Files/a.txt", "a");
        put(&pc, "Studio PC/Files/b.txt", "b");
        backed_up(&pc, "Studio PC", "pc", "2026-10-27 10.00.00");
        pull_from(&mut Copy::new(&pc), &mac, "mac");

        put(&pc, "Studio PC/Files/b.txt", "b2");
        backed_up(&pc, "Studio PC", "pc", "2026-10-27 11.00.00");
        let mut copy = Copy::new(&pc);
        let r = pull_from(&mut copy, &mac, "mac");
        assert_eq!(r.copied, 2);
        assert_eq!(
            copy.asked,
            vec![
                "Studio PC/Files/b.txt".to_owned(),
                "Studio PC/Backups/2026-10-27 11.00.00.json".to_owned()
            ]
        );
        assert_eq!(
            std::fs::read_to_string(mac.join("Studio PC/Files/b.txt")).unwrap(),
            "b2"
        );
    }

    #[test]
    fn a_new_copy_gets_this_machines_folder_back_and_a_name_clash_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let (pc, mac) = (tmp.path().join("pc"), tmp.path().join("mac"));
        put(&pc, "MacBook/Files/mine.txt", "mine");
        backed_up(&pc, "MacBook", "mac", "2026-10-27 10.00.00");
        put(&pc, "Studio/Files/x.txt", "x");
        backed_up(&pc, "Studio", "pc", "2026-10-27 10.00.00");
        // The Mac's copy already has a different machine's "Studio".
        put(&mac, "Studio/Files/y.txt", "y");
        backed_up(&mac, "Studio", "other", "2026-10-26 10.00.00");

        let r = pull_from(&mut Copy::new(&pc), &mac, "mac");
        assert_eq!(r.machines, vec!["MacBook".to_owned()]);
        assert!(mac.join("MacBook/Files/mine.txt").is_file());
        assert!(!mac.join("Studio/Files/x.txt").exists());
    }

    #[test]
    fn requests_outside_the_copy_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        put(tmp.path(), "secret.txt", "no");
        let dir = tmp.path().join("copy");
        put(&dir, "PC/Files/a.txt", "a");
        let index = Mutex::default();
        for path in [
            "../secret.txt",
            "/etc/passwd",
            "PC/../../secret.txt",
            "C:/x",
        ] {
            let ask = Ask::File { path: path.into() };
            assert!(reply(&ask, &dir, &index).is_err(), "{path}");
        }
        let ask = Ask::Files {
            folder: "..".into(),
        };
        assert!(reply(&ask, &dir, &index).is_err());
    }

    #[test]
    fn beacons_open_only_for_the_rig() {
        let rig = RigKey::generate();
        let b = Beacon {
            machine_id: "pc".into(),
            machine_name: "Studio PC".into(),
            port: PORT,
            state: "abc".into(),
        };
        let bytes = beacon_bytes(&rig, &b);
        assert!(bytes.len() < MAX_BEACON);
        assert_eq!(read_beacon(&rig, &bytes), Some(b));
        assert_eq!(read_beacon(&RigKey::generate(), &bytes), None);
        assert_eq!(read_beacon(&rig, b"PPLAN1junk"), None);
    }

    #[test]
    fn pulls_over_a_real_connection_and_strangers_get_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let (pc, mac) = (tmp.path().join("pc"), tmp.path().join("mac"));
        put(&pc, "Studio PC/Files/Reaper/Scripts/a.lua", "print(1)");
        backed_up(&pc, "Studio PC", "pc", "2026-10-27 10.00.00");
        let rig = RigKey::generate();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (key, dir) = (rig.clone(), pc.clone());
        let server = std::thread::spawn(move || {
            let index = Mutex::default();
            let mut results = Vec::new();
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                results.push(answer(&mut stream, &key, &dir, &index));
            }
            results
        });

        let mut conn = Connection::open(addr, &rig).unwrap();
        let r = pull(
            &mut conn,
            &mac,
            "mac",
            &mut FileIndex::default(),
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(r.copied, 2);
        assert_eq!(
            std::fs::read_to_string(mac.join("Studio PC/Files/Reaper/Scripts/a.lua")).unwrap(),
            "print(1)"
        );
        drop(conn);

        let mut stranger = Connection::open(addr, &RigKey::generate()).unwrap();
        assert!(stranger.folders().is_err());
        let results = server.join().unwrap();
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
    }
}
