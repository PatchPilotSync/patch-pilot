//! Advanced: sync with GitHub (decision #73). Instead of a cloud folder, the
//! backup and keep-in-sync folder (decisions #67 and #70) is a copy in
//! app-data that Patch Pilot pushes to and pulls from a private GitHub repo
//! the user picks. Backup, restore and keeping files in sync work on that
//! copy exactly as on a cloud folder; only how files travel changes. Every
//! change becomes a commit, so the repo's history is a second, exact record.
//! Big files for Bring over keep going through a cloud folder (decision #64).
//!
//! The repo holds what `Patch Pilot Backup` holds: one folder per machine.
//! Each machine changes only its own folder, so two machines never write the
//! same file and a push that lost a race just goes again on the newer commit.
//! Everything else in the copy is whatever the repo has.
//!
//! No network here: the app talks to GitHub through [`Remote`].

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backup::{FOLDER_NAME, below, own_folder, remove_empty_parents, safe_rel, write_atomic};

/// [`crate::backup::BackupSettings::service`] while syncing with GitHub.
pub const SERVICE: &str = "github";
const SETTINGS_FILE: &str = "github.json";
const TOKEN_FILE: &str = "github-token";
const INDEX_FILE: &str = "github-index.json";
const MIRROR: &str = "GitHub";
/// GitHub refuses files over 100 MB; backups stop at 64 MB anyway.
pub const MAX_FILE_BYTES: u64 = 95 * 1024 * 1024;
/// The name offered for a new repo.
pub const DEFAULT_REPO: &str = "patch-pilot-backup";

/// Which repo this machine syncs with, kept in app-data. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubSettings {
    /// Who signed in, for the UI.
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub repo: Option<RepoRef>,
    /// The commit the copy was last brought up to.
    #[serde(default)]
    pub head: Option<String>,
    #[serde(default)]
    pub synced_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
    pub branch: String,
}

impl RepoRef {
    /// `owner/name`, as GitHub shows it.
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

pub fn load_settings(app_dir: &Path) -> GitHubSettings {
    std::fs::read(app_dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_settings(app_dir: &Path, settings: &GitHubSettings) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(settings).expect("settings serialize");
    write_atomic(&app_dir.join(SETTINGS_FILE), &json)
}

/// Blob IDs of the copy's files by size and modified time, so a sync reads
/// only files that changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIndex(BTreeMap<String, (u64, u128, String)>);

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

/// The sign-in token, kept in app-data beside the rig key (decision #38),
/// readable only by this user.
pub fn load_token(app_dir: &Path) -> Option<String> {
    std::fs::read_to_string(app_dir.join(TOKEN_FILE))
        .ok()
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
}

pub fn save_token(app_dir: &Path, token: &str) -> Result<(), String> {
    let path = app_dir.join(TOKEN_FILE);
    write_atomic(&path, token.trim().as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// Sign out: forget the token and the repo. The copy in app-data stays.
pub fn forget(app_dir: &Path) -> Result<(), String> {
    for name in [TOKEN_FILE, SETTINGS_FILE, INDEX_FILE] {
        match std::fs::remove_file(app_dir.join(name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
            _ => {}
        }
    }
    Ok(())
}

/// The folder to set as the backup folder: `Patch Pilot Backup` inside it is
/// the repo's copy.
pub fn backup_folder(app_dir: &Path) -> PathBuf {
    app_dir.join(MIRROR)
}

/// The repo's copy: what [`crate::backup::backup_dir`] gives for
/// [`backup_folder`].
pub fn mirror_dir(app_dir: &Path) -> PathBuf {
    backup_folder(app_dir).join(FOLDER_NAME)
}

/// `owner/name`, `name` (on `login`'s account) or a github.com link, as a
/// pair; `None` if it isn't a repo name.
pub fn parse_repo(input: &str, login: &str) -> Option<(String, String)> {
    let s = input.trim().trim_end_matches('/');
    let s = s
        .strip_prefix("https://github.com/")
        .or_else(|| s.strip_prefix("github.com/"))
        .unwrap_or(s);
    let s = s.strip_suffix(".git").unwrap_or(s);
    let (owner, name) = match s.split_once('/') {
        Some((o, n)) => (o, n),
        None => (login, s),
    };
    let ok = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (ok(owner) && ok(name)).then(|| (owner.to_owned(), name.to_owned()))
}

/// The Git blob ID of `bytes`, as GitHub names a file's content.
pub fn blob_id(bytes: &[u8]) -> String {
    let mut h = sha1_smol::Sha1::new();
    h.update(format!("blob {}\0", bytes.len()).as_bytes());
    h.update(bytes);
    h.digest().to_string()
}

/// A file in the repo at a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    /// `/`-separated, from the repo's root.
    pub path: String,
    /// Its [`blob_id`].
    pub id: String,
}

/// A file to add or replace (`Some(blob id)`) or remove (`None`) in a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushError {
    /// Another machine pushed first: go again on the newer commit.
    Moved,
    Failed(String),
}

/// The repo, as the app reaches it over GitHub's API.
pub trait Remote {
    /// The branch's latest commit (making a first one in an empty repo).
    fn head(&self) -> Result<String, String>;
    /// Every file at `commit`.
    fn files(&self, commit: &str) -> Result<Vec<RemoteFile>, String>;
    fn download(&self, id: &str) -> Result<Vec<u8>, String>;
    /// Store `bytes`, giving its blob ID.
    fn upload(&self, bytes: &[u8]) -> Result<String, String>;
    /// A commit of `changes` on `parent`, moving the branch to it only if
    /// the branch is still at `parent`.
    fn commit(&self, parent: &str, changes: &[Change], message: &str) -> Result<String, PushError>;
}

/// What one sync did. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubReport {
    /// Files from other machines brought into the copy, or taken out of it.
    pub pulled: usize,
    /// This machine's files sent (added, changed or removed).
    pub pushed: usize,
    /// Files too big for GitHub, left out.
    pub too_big: Vec<String>,
    /// Paths that can't be a file on this OS, left out.
    pub unsafe_paths: Vec<String>,
}

/// Pull everything other machines changed into the copy at `dir`, then
/// push what this machine (`this`) changed in its own folder.
pub fn sync(
    remote: &dyn Remote,
    dir: &Path,
    this: &str,
    machine_name: &str,
    settings: &mut GitHubSettings,
    index: &mut FileIndex,
    now: i64,
) -> Result<GitHubReport, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // This machine's folder as it was before pulling: a machine whose copy
    // is new gets its own folder back from the repo like any other.
    let mine = own_folder(dir, this);
    let in_mine = |p: &str| {
        mine.as_ref().is_some_and(|m| {
            p.strip_prefix(m.as_str())
                .is_some_and(|r| r.starts_with('/'))
        })
    };
    let mut report = GitHubReport::default();
    for _ in 0..4 {
        let head = remote.head()?;
        let theirs: BTreeMap<String, String> = remote
            .files(&head)?
            .into_iter()
            .map(|f| (f.path, f.id))
            .collect();
        let here = local_files(dir, index)?;

        // Pull: outside this machine's folder the copy follows the repo.
        for (path, id) in &theirs {
            if in_mine(path) || here.get(path) == Some(id) {
                continue;
            }
            if !safe_rel(path) {
                if !report.unsafe_paths.contains(path) {
                    report.unsafe_paths.push(path.clone());
                }
                continue;
            }
            let bytes = remote.download(id)?;
            if blob_id(&bytes) != *id {
                return Err(format!("GitHub sent a different copy of {path}."));
            }
            write_atomic(&below(dir, path), &bytes)?;
            report.pulled += 1;
        }
        for path in here.keys() {
            if !in_mine(path) && !theirs.contains_key(path) {
                let target = below(dir, path);
                std::fs::remove_file(&target).map_err(|e| format!("{}: {e}", target.display()))?;
                remove_empty_parents(&target, dir);
                report.pulled += 1;
            }
        }

        // Push: in this machine's folder the repo follows the copy.
        let mut changes = Vec::new();
        let mut uploads = Vec::new();
        report.too_big.clear();
        for (path, id) in here.iter().filter(|(p, _)| in_mine(p)) {
            if theirs.get(path) == Some(id) {
                continue;
            }
            let file = below(dir, path);
            let size = std::fs::metadata(&file).map_or(0, |m| m.len());
            if size > MAX_FILE_BYTES {
                report.too_big.push(path.clone());
                continue;
            }
            changes.push(Change {
                path: path.clone(),
                id: Some(id.clone()),
            });
            uploads.push((file, id));
        }
        for path in theirs.keys().filter(|p| in_mine(p)) {
            if !here.contains_key(path) {
                changes.push(Change {
                    path: path.clone(),
                    id: None,
                });
            }
        }
        if changes.is_empty() {
            settings.head = Some(head);
            settings.synced_at = Some(now);
            return Ok(report);
        }
        // A file moved to Versions is already there under its old path.
        let known: HashSet<&String> = theirs.values().collect();
        for (file, id) in uploads {
            if known.contains(id) {
                continue;
            }
            let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let sent = remote.upload(&bytes)?;
            if sent != *id {
                return Err(format!(
                    "GitHub stored a different copy of {}.",
                    file.display()
                ));
            }
        }
        let n = changes.len();
        let message = format!(
            "{machine_name}: {n} file{} changed",
            if n == 1 { "" } else { "s" }
        );
        match remote.commit(&head, &changes, &message) {
            Ok(commit) => {
                report.pushed = n;
                settings.head = Some(commit);
                settings.synced_at = Some(now);
                return Ok(report);
            }
            Err(PushError::Moved) => continue,
            Err(PushError::Failed(e)) => return Err(e),
        }
    }
    Err("GitHub kept changing while syncing. Patch Pilot tries again on the next change.".into())
}

/// Every file in the copy, by `/`-separated path, with its blob ID. Files
/// still being written are left out.
fn local_files(dir: &Path, index: &mut FileIndex) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut seen = BTreeMap::new();
    for entry in walkdir::WalkDir::new(dir).min_depth(1) {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if name.ends_with(".patchpilot-partial") {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(dir)
            .map_err(|e| e.to_string())?
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        let id = match index.0.get(&rel) {
            Some((size, t, id)) if *size == meta.len() && *t == mtime && mtime != 0 => id.clone(),
            _ => blob_id(
                &std::fs::read(entry.path())
                    .map_err(|e| format!("{}: {e}", entry.path().display()))?,
            ),
        };
        seen.insert(rel.clone(), (meta.len(), mtime, id.clone()));
        out.insert(rel, id);
    }
    index.0 = seen;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A repo in memory: commits as path -> blob ID maps.
    #[derive(Default)]
    struct FakeRepo {
        blobs: RefCell<HashMap<String, Vec<u8>>>,
        commits: RefCell<Vec<BTreeMap<String, String>>>,
        /// Another machine's commit lands just before the next push.
        race: RefCell<Option<(String, Vec<u8>)>>,
        uploads: RefCell<usize>,
    }

    impl FakeRepo {
        fn new() -> Self {
            let r = Self::default();
            r.commits.borrow_mut().push(BTreeMap::new());
            r
        }
        fn put(&self, path: &str, body: &str) {
            let id = blob_id(body.as_bytes());
            self.blobs.borrow_mut().insert(id.clone(), body.into());
            let mut next = self.commits.borrow().last().unwrap().clone();
            next.insert(path.into(), id);
            self.commits.borrow_mut().push(next);
        }
        fn latest(&self) -> BTreeMap<String, String> {
            self.commits.borrow().last().unwrap().clone()
        }
        fn body(&self, path: &str) -> Option<String> {
            let id = self.latest().get(path)?.clone();
            Some(String::from_utf8(self.blobs.borrow()[&id].clone()).unwrap())
        }
    }

    impl Remote for FakeRepo {
        fn head(&self) -> Result<String, String> {
            Ok((self.commits.borrow().len() - 1).to_string())
        }
        fn files(&self, commit: &str) -> Result<Vec<RemoteFile>, String> {
            let i: usize = commit.parse().unwrap();
            Ok(self.commits.borrow()[i]
                .iter()
                .map(|(p, id)| RemoteFile {
                    path: p.clone(),
                    id: id.clone(),
                })
                .collect())
        }
        fn download(&self, id: &str) -> Result<Vec<u8>, String> {
            self.blobs.borrow().get(id).cloned().ok_or("no blob".into())
        }
        fn upload(&self, bytes: &[u8]) -> Result<String, String> {
            *self.uploads.borrow_mut() += 1;
            let id = blob_id(bytes);
            self.blobs.borrow_mut().insert(id.clone(), bytes.to_vec());
            Ok(id)
        }
        fn commit(&self, parent: &str, changes: &[Change], _: &str) -> Result<String, PushError> {
            if let Some((path, body)) = self.race.borrow_mut().take() {
                self.put(&path, std::str::from_utf8(&body).unwrap());
            }
            if parent != self.head().unwrap() {
                return Err(PushError::Moved);
            }
            let mut next = self.latest();
            for c in changes {
                match &c.id {
                    Some(id) => {
                        assert!(self.blobs.borrow().contains_key(id), "blob not uploaded");
                        next.insert(c.path.clone(), id.clone());
                    }
                    None => {
                        assert!(next.remove(&c.path).is_some(), "removed a path not there");
                    }
                }
            }
            self.commits.borrow_mut().push(next);
            self.head().map_err(PushError::Failed)
        }
    }

    fn put(dir: &Path, rel: &str, body: &str) {
        let p = below(dir, rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    /// The bits of a backup [`own_folder`] reads.
    fn backed_up(dir: &Path, folder: &str, id: &str) {
        put(
            dir,
            &format!("{folder}/Backups/2026-10-26 02.48.38.json"),
            &serde_json::json!({
                "format": 1, "made_at": 0, "stamp": "2026-10-26 02.48.38",
                "machine_id": id, "machine_name": folder, "os": "macos", "files": [],
                "scan": patchpilot_core::Manifest::new(
                    patchpilot_core::MachineInfo {
                        id: id.into(),
                        name: folder.into(),
                        os: patchpilot_core::Os::Macos,
                        os_version: None,
                        arch: patchpilot_core::Arch::Aarch64,
                        path_map: patchpilot_core::PathMap::new(patchpilot_core::Os::Macos),
                    },
                    0,
                ),
            })
            .to_string(),
        );
    }

    #[test]
    fn blob_ids_match_git() {
        // `printf 'hello\n' | git hash-object --stdin`
        assert_eq!(
            blob_id(b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
        assert_eq!(blob_id(b""), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    }

    #[test]
    fn pushes_own_folder_and_pulls_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let mac = tmp.path().join("mac");
        let pc = tmp.path().join("pc");
        let repo = FakeRepo::new();
        repo.put("README.md", "Patch Pilot backups");
        let mut mac_s = GitHubSettings::default();
        let mut pc_s = GitHubSettings::default();
        let mut mac_i = FileIndex::default();

        backed_up(&mac, "MacBook", "mac");
        put(
            &mac,
            "MacBook/Files/Reaper/FXChains/Vocal.RfxChain",
            "vocal v1",
        );
        let r = sync(&repo, &mac, "mac", "MacBook", &mut mac_s, &mut mac_i, 10).unwrap();
        assert_eq!((r.pulled, r.pushed), (1, 2));
        assert_eq!(
            repo.body("MacBook/Files/Reaper/FXChains/Vocal.RfxChain")
                .as_deref(),
            Some("vocal v1")
        );
        assert_eq!(mac_s.head.as_deref(), Some("2"));
        assert_eq!(mac_s.synced_at, Some(10));

        // The PC gets the Mac's folder and pushes its own.
        backed_up(&pc, "Studio PC", "pc");
        let r = sync(
            &repo,
            &pc,
            "pc",
            "Studio PC",
            &mut pc_s,
            &mut FileIndex::default(),
            20,
        )
        .unwrap();
        assert_eq!((r.pulled, r.pushed), (3, 1));
        assert_eq!(
            std::fs::read_to_string(pc.join("MacBook/Files/Reaper/FXChains/Vocal.RfxChain"))
                .unwrap(),
            "vocal v1"
        );

        // A change on the Mac, moving the old copy to Versions: the old
        // content isn't uploaded again, and the PC follows.
        let v1 = "MacBook/Files/Reaper/FXChains/Vocal.RfxChain";
        std::fs::create_dir_all(mac.join("MacBook/Versions/s/Reaper/FXChains")).unwrap();
        std::fs::rename(
            below(&mac, v1),
            mac.join("MacBook/Versions/s/Reaper/FXChains/Vocal.RfxChain"),
        )
        .unwrap();
        put(&mac, v1, "vocal v2");
        let before = *repo.uploads.borrow();
        let r = sync(&repo, &mac, "mac", "MacBook", &mut mac_s, &mut mac_i, 30).unwrap();
        assert_eq!(r.pushed, 2);
        assert_eq!(*repo.uploads.borrow() - before, 1);
        let r = sync(
            &repo,
            &pc,
            "pc",
            "Studio PC",
            &mut pc_s,
            &mut FileIndex::default(),
            40,
        )
        .unwrap();
        assert_eq!((r.pulled, r.pushed), (2, 0));
        assert_eq!(std::fs::read_to_string(below(&pc, v1)).unwrap(), "vocal v2");

        // A file gone from the Mac's folder goes from the repo, then the PC.
        std::fs::remove_file(below(&mac, v1)).unwrap();
        let r = sync(&repo, &mac, "mac", "MacBook", &mut mac_s, &mut mac_i, 50).unwrap();
        assert_eq!(r.pushed, 1);
        assert!(repo.body(v1).is_none());
        sync(
            &repo,
            &pc,
            "pc",
            "Studio PC",
            &mut pc_s,
            &mut FileIndex::default(),
            60,
        )
        .unwrap();
        assert!(!below(&pc, v1).exists());
        assert!(
            pc.join("MacBook/Versions/s/Reaper/FXChains/Vocal.RfxChain")
                .is_file()
        );
        // Nothing to do: nothing sent.
        let r = sync(
            &repo,
            &pc,
            "pc",
            "Studio PC",
            &mut pc_s,
            &mut FileIndex::default(),
            70,
        )
        .unwrap();
        assert_eq!((r.pulled, r.pushed), (0, 0));
    }

    #[test]
    fn never_overwrites_this_machines_folder_and_retries_a_lost_race() {
        let tmp = tempfile::tempdir().unwrap();
        let pc = tmp.path().join("pc");
        let repo = FakeRepo::new();
        // The repo's copy of the PC's folder is older than the PC's own.
        repo.put("Studio PC/Files/a.txt", "old");
        backed_up(&pc, "Studio PC", "pc");
        put(&pc, "Studio PC/Files/a.txt", "new");
        *repo.race.borrow_mut() = Some(("MacBook/Files/b.txt".into(), b"mac".to_vec()));
        let mut s = GitHubSettings::default();
        let r = sync(
            &repo,
            &pc,
            "pc",
            "Studio PC",
            &mut s,
            &mut FileIndex::default(),
            1,
        )
        .unwrap();
        assert_eq!(repo.body("Studio PC/Files/a.txt").as_deref(), Some("new"));
        assert_eq!(repo.body("MacBook/Files/b.txt").as_deref(), Some("mac"));
        assert_eq!(
            std::fs::read_to_string(pc.join("MacBook/Files/b.txt")).unwrap(),
            "mac"
        );
        assert_eq!(r.pushed, 2);
    }

    #[test]
    fn a_new_copy_gets_this_machines_folder_back() {
        let tmp = tempfile::tempdir().unwrap();
        let pc = tmp.path().join("pc");
        let repo = FakeRepo::new();
        backed_up(&pc, "Studio PC", "pc");
        put(&pc, "Studio PC/Files/a.txt", "a");
        sync(
            &repo,
            &pc,
            "pc",
            "Studio PC",
            &mut GitHubSettings::default(),
            &mut FileIndex::default(),
            1,
        )
        .unwrap();

        // The app-data copy is gone (a reinstall): nothing is removed from
        // the repo, and the copy is whole again.
        let fresh = tmp.path().join("fresh");
        let r = sync(
            &repo,
            &fresh,
            "pc",
            "Studio PC",
            &mut GitHubSettings::default(),
            &mut FileIndex::default(),
            2,
        )
        .unwrap();
        assert_eq!(r.pushed, 0);
        assert_eq!(
            std::fs::read_to_string(fresh.join("Studio PC/Files/a.txt")).unwrap(),
            "a"
        );
        assert_eq!(repo.body("Studio PC/Files/a.txt").as_deref(), Some("a"));
    }

    #[test]
    fn leaves_out_paths_this_os_cant_hold() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = FakeRepo::new();
        repo.put("MacBook/Files/bad:name.txt", "x");
        repo.put("MacBook/Files/ok.txt", "y");
        let r = sync(
            &repo,
            tmp.path(),
            "pc",
            "PC",
            &mut GitHubSettings::default(),
            &mut FileIndex::default(),
            1,
        )
        .unwrap();
        assert_eq!(
            r.unsafe_paths,
            vec!["MacBook/Files/bad:name.txt".to_owned()]
        );
        assert!(tmp.path().join("MacBook/Files/ok.txt").is_file());
    }

    #[test]
    fn reads_repo_names() {
        let p = |s| parse_repo(s, "josh");
        assert_eq!(
            p("patch-pilot-backup"),
            Some(("josh".into(), "patch-pilot-backup".into()))
        );
        assert_eq!(p("team/rig"), Some(("team".into(), "rig".into())));
        assert_eq!(
            p("https://github.com/team/rig.git"),
            Some(("team".into(), "rig".into()))
        );
        assert_eq!(p("a/b/c"), None);
        assert_eq!(p(""), None);
        assert_eq!(p("../x"), None);
    }

    #[test]
    fn token_is_kept_and_forgotten() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load_token(tmp.path()), None);
        save_token(tmp.path(), " gho_abc\n").unwrap();
        assert_eq!(load_token(tmp.path()).as_deref(), Some("gho_abc"));
        let s = GitHubSettings {
            login: Some("josh".into()),
            ..GitHubSettings::default()
        };
        save_settings(tmp.path(), &s).unwrap();
        assert_eq!(load_settings(tmp.path()), s);
        forget(tmp.path()).unwrap();
        assert_eq!(load_token(tmp.path()), None);
        assert_eq!(load_settings(tmp.path()), GitHubSettings::default());
    }
}
