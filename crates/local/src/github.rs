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
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backup::{FOLDER_NAME, below, own_folder, remove_empty_parents, safe_rel, write_atomic};
use crate::gitpack;

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

impl FileIndex {
    /// Record that the file at `path` (`rel` in the copy) was just written
    /// with blob ID `id`, so the next look doesn't read it again.
    pub(crate) fn note(&mut self, rel: &str, path: &Path, id: &str) {
        if let Ok(meta) = std::fs::metadata(path) {
            self.0
                .insert(rel.to_owned(), (meta.len(), mtime(&meta), id.to_owned()));
        }
    }
}

fn mtime(meta: &std::fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos())
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

/// One entry of a tree as GitHub lists it, its path from that tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: String,
    pub id: String,
    /// A folder; otherwise a file (links and submodules are left out).
    pub is_tree: bool,
}

/// A tree's entries, and whether GitHub cut the list short.
pub type Listing = (Vec<TreeEntry>, bool);

/// Every file under `tree`. `list` lists a tree, all the way down when
/// asked, and says when GitHub cut that listing short (it stops at about
/// 100,000 entries or 7 MB). A folder whose whole listing is too big is
/// listed one level, and each folder in it on its own, so any size of repo
/// can be read.
pub fn walk_tree(
    list: &dyn Fn(&str, bool) -> Result<Listing, String>,
    tree: &str,
) -> Result<Vec<RemoteFile>, String> {
    let mut out = Vec::new();
    let mut todo = vec![(tree.to_owned(), String::new())];
    while let Some((tree, prefix)) = todo.pop() {
        let (entries, cut) = list(&tree, true)?;
        let (entries, recursive) = if cut {
            (list(&tree, false)?.0, false)
        } else {
            (entries, true)
        };
        for e in entries {
            let path = format!("{prefix}{}", e.path);
            if !e.is_tree {
                out.push(RemoteFile { path, id: e.id });
            } else if !recursive {
                todo.push((e.id, format!("{path}/")));
            }
        }
    }
    Ok(out)
}

/// The repo, as the app reaches it: reads through GitHub's REST API, a big
/// first pull as one archive, and pushes over Git's own smart HTTP protocol,
/// which takes thousands of files in one request.
pub trait Remote {
    /// The branch's latest commit (making a first one in an empty repo).
    fn head(&self) -> Result<String, String>;
    /// Every file at `commit`.
    fn files(&self, commit: &str) -> Result<Vec<RemoteFile>, String>;
    fn download(&self, id: &str) -> Result<Vec<u8>, String>;
    /// The whole repo at `commit` as a `.tar.gz` (one folder at its top).
    fn archive(&self, commit: &str) -> Result<Box<dyn Read + '_>, String>;
    /// Send a `git-receive-pack` request ([`gitpack::receive_pack_request`]);
    /// the server's response.
    fn receive_pack(&self, body: Vec<u8>) -> Result<Vec<u8>, String>;
}

/// How far a long sync has got, for "Uploading 3,200 of 15,456 files".
/// Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Progress {
    /// Sending this machine's files; otherwise bringing others' in.
    pub uploading: bool,
    pub done: usize,
    pub total: usize,
}

/// More files than this to bring in come as one archive, not one by one.
const ARCHIVE_OVER: usize = if cfg!(test) { 5 } else { 50 };
/// One push carries at most this much: a big first upload goes in parts,
/// each a commit, so a dropped connection loses one part at most.
const PUSH_BYTES: u64 = 96 * 1024 * 1024;
const PUSH_FILES: usize = if cfg!(test) { 4 } else { 4000 };

/// What one sync did. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubReport {
    /// Files from other machines brought into the copy, or taken out of it.
    pub pulled: usize,
    /// This machine's files sent (added, changed or removed).
    pub pushed: usize,
    /// Files too big for GitHub, left out.
    pub too_big: Vec<String>,
    /// Paths that can't be a file on this OS, or can't go in a Git repo
    /// (inside a `.git` folder), left out.
    pub unsafe_paths: Vec<String>,
}

/// Pull everything other machines changed into the copy at `dir`, then
/// push what this machine (`this`) changed in its own folder, in parts of
/// at most [`PUSH_FILES`] files. `progress` hears how a long one goes.
#[allow(clippy::too_many_arguments)]
pub fn sync(
    remote: &dyn Remote,
    dir: &Path,
    this: &str,
    machine_name: &str,
    settings: &mut GitHubSettings,
    index: &mut FileIndex,
    now: i64,
    progress: &mut dyn FnMut(Progress),
) -> Result<GitHubReport, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let branch = settings
        .repo
        .as_ref()
        .map_or_else(|| "main".to_owned(), |r| r.branch.clone());
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
    let mut moved = 0;
    // Files to send when the first part went: the total progress counts.
    let mut upload_total: Option<usize> = None;
    loop {
        let head = remote.head()?;
        let theirs: BTreeMap<String, String> = remote
            .files(&head)?
            .into_iter()
            .map(|f| (f.path, f.id))
            .collect();
        let here = local_files(dir, index)?;

        // Pull: outside this machine's folder the copy follows the repo.
        let mut wanted: BTreeMap<String, String> = BTreeMap::new();
        for (path, id) in &theirs {
            if in_mine(path) || here.get(path) == Some(id) {
                continue;
            }
            if safe_rel(path) {
                wanted.insert(path.clone(), id.clone());
            } else if !report.unsafe_paths.contains(path) {
                report.unsafe_paths.push(path.clone());
            }
        }
        let total = wanted.len();
        if total > 0 {
            let mut done = 0;
            progress(Progress {
                uploading: false,
                done,
                total,
            });
            if total > ARCHIVE_OVER {
                let archive = remote.archive(&head)?;
                extract(archive, dir, &mut wanted, &mut || {
                    done += 1;
                    if done % 100 == 0 {
                        progress(Progress {
                            uploading: false,
                            done,
                            total,
                        });
                    }
                })?;
            }
            // What the archive didn't have (or a small pull), file by file.
            for (path, id) in &wanted {
                let bytes = remote.download(id)?;
                if blob_id(&bytes) != *id {
                    return Err(format!("GitHub sent a different copy of {path}."));
                }
                write_atomic(&below(dir, path), &bytes)?;
                done += 1;
                if done % 20 == 0 {
                    progress(Progress {
                        uploading: false,
                        done,
                        total,
                    });
                }
            }
            report.pulled += total;
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
        let mut files = theirs.clone();
        let mut removed = 0;
        for path in theirs.keys().filter(|p| in_mine(p)) {
            if !here.contains_key(path) {
                files.remove(path);
                removed += 1;
            }
        }
        report.too_big.clear();
        let mut sends: Vec<(&String, &String)> = Vec::new();
        for (path, id) in here.iter().filter(|(p, _)| in_mine(p)) {
            if theirs.get(path) == Some(id) {
                continue;
            }
            if !git_can_hold(path) {
                if !report.unsafe_paths.contains(path) {
                    report.unsafe_paths.push(path.clone());
                }
                continue;
            }
            let size = std::fs::metadata(below(dir, path)).map_or(0, |m| m.len());
            if size > MAX_FILE_BYTES {
                report.too_big.push(path.clone());
            } else {
                sends.push((path, id));
            }
        }
        // A backup's list of files goes last, after its signature, so
        // another machine sees it only once the files it names are there.
        if let Some(m) = &mine {
            let lists = format!("{m}/Backups/");
            sends.sort_by_key(|(p, _)| {
                let list = p.starts_with(&lists);
                (list, list && p.ends_with(".json"))
            });
        }
        if sends.is_empty() && removed == 0 {
            settings.head = Some(head);
            settings.synced_at = Some(now);
            return Ok(report);
        }
        let total = *upload_total.get_or_insert(sends.len() + removed);

        // This part: blobs the repo lacks (a copy moved into Versions is
        // already there), every tree, and the commit.
        let known: HashSet<&String> = theirs.values().collect();
        let mut blobs: Vec<Vec<u8>> = Vec::new();
        let mut packed: HashSet<String> = HashSet::new();
        let (mut bytes, mut sent) = (0u64, 0usize);
        for (path, id) in &sends {
            if sent >= PUSH_FILES || bytes >= PUSH_BYTES {
                break;
            }
            sent += 1;
            let mut id = (*id).clone();
            if !known.contains(&id) {
                let file = below(dir, path);
                let body = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
                // Changed since it was listed: send what's there now.
                id = blob_id(&body);
                if packed.insert(id.clone()) {
                    bytes += body.len() as u64;
                    blobs.push(body);
                }
            }
            files.insert((*path).clone(), id);
        }
        let left = sends.len() - sent;
        let (tree, trees) = gitpack::trees(&files)?;
        let n = sent + removed;
        let message = format!(
            "{machine_name}: {n} file{} changed{}",
            if n == 1 { "" } else { "s" },
            if left > 0 { ", more to come" } else { "" }
        );
        let commit = gitpack::commit(&tree, Some(&head), &message, now);
        let commit_id = gitpack::object_id(gitpack::Kind::Commit, &commit);
        let mut objects: Vec<(gitpack::Kind, &[u8])> = blobs
            .iter()
            .map(|b| (gitpack::Kind::Blob, b.as_slice()))
            .collect();
        objects.extend(
            trees
                .iter()
                .map(|(_, b)| (gitpack::Kind::Tree, b.as_slice())),
        );
        objects.push((gitpack::Kind::Commit, &commit));
        let body =
            gitpack::receive_pack_request(&head, &commit_id, &branch, &gitpack::pack(&objects));
        drop(objects);
        drop(blobs);
        let done = total.saturating_sub(sends.len() + removed);
        progress(Progress {
            uploading: true,
            done,
            total,
        });
        match gitpack::push_status(&remote.receive_pack(body)?) {
            gitpack::PushStatus::Ok => {
                report.pushed += n;
                settings.head = Some(commit_id);
                progress(Progress {
                    uploading: true,
                    done: done + n,
                    total,
                });
                if left == 0 {
                    settings.synced_at = Some(now);
                    return Ok(report);
                }
            }
            // Git says "failed to update ref" when the branch moved; GitHub
            // may word it otherwise. Either way, a moved branch goes again.
            gitpack::PushStatus::Rejected(why) if remote.head()? == head => {
                return Err(format!("GitHub turned down the upload: {why}."));
            }
            gitpack::PushStatus::Moved | gitpack::PushStatus::Rejected(_) => {
                moved += 1;
                if moved > 4 {
                    return Err(
                        "GitHub kept changing while syncing. Patch Pilot tries again on the next change."
                            .into(),
                    );
                }
            }
        }
    }
}

/// Whether GitHub takes a file at `path`: it refuses anything in a `.git`
/// folder (a script folder that's a Git checkout) and checks every
/// `.gitmodules`, so neither goes up.
fn git_can_hold(path: &str) -> bool {
    !path
        .split('/')
        .any(|c| c.eq_ignore_ascii_case(".git") || c.eq_ignore_ascii_case(".gitmodules"))
}

/// Write the files of a `.tar.gz` of the repo that are in `wanted` (and are
/// the copy it names) into `dir`, taking each off `wanted`.
fn extract(
    archive: Box<dyn Read + '_>,
    dir: &Path,
    wanted: &mut BTreeMap<String, String>,
    tick: &mut dyn FnMut(),
) -> Result<(), String> {
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    let broken = |e: std::io::Error| format!("The download from GitHub broke off: {e}");
    for entry in tar.entries().map_err(broken)? {
        let mut entry = entry.map_err(broken)?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        // GitHub puts everything in one `owner-repo-commit/` folder.
        let path = entry.path().map_err(broken)?;
        let rel = path
            .components()
            .skip(1)
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let Some(id) = wanted.get(&rel) else {
            continue;
        };
        let mut body = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut body).map_err(broken)?;
        if blob_id(&body) == *id {
            write_atomic(&below(dir, &rel), &body)?;
            wanted.remove(&rel);
            tick();
        }
    }
    Ok(())
}

/// Every file in the copy, by `/`-separated path, with its blob ID. Files
/// still being written are left out.
pub(crate) fn local_files(
    dir: &Path,
    index: &mut FileIndex,
) -> Result<BTreeMap<String, String>, String> {
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
        let mtime = mtime(&meta);
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
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::io::Write;

    use gitpack::Kind;

    /// A repo in memory, made of real Git objects, that takes pushes the way
    /// GitHub's `git-receive-pack` does.
    struct FakeRepo {
        objects: RefCell<HashMap<String, (Kind, Vec<u8>)>>,
        head: RefCell<String>,
        /// Another machine's commit lands just before the next push.
        race: RefCell<Option<(String, String)>>,
        /// Blobs that came in pushes.
        received: Cell<usize>,
        pushes: Cell<usize>,
        /// The connection drops on this push (counting from 1).
        drop_push: Cell<Option<usize>>,
        archives: Cell<usize>,
        /// A file the archive leaves out.
        archive_skips: RefCell<Option<String>>,
        downloads: Cell<usize>,
    }

    impl FakeRepo {
        fn new() -> Self {
            let r = Self {
                objects: RefCell::default(),
                head: RefCell::default(),
                race: RefCell::default(),
                received: Cell::new(0),
                pushes: Cell::new(0),
                drop_push: Cell::new(None),
                archives: Cell::new(0),
                archive_skips: RefCell::default(),
                downloads: Cell::new(0),
            };
            r.commit_files(&BTreeMap::new(), "Start");
            r
        }
        fn store(&self, kind: Kind, body: &[u8]) -> String {
            let id = gitpack::object_id(kind, body);
            self.objects
                .borrow_mut()
                .insert(id.clone(), (kind, body.to_vec()));
            id
        }
        fn object(&self, id: &str, kind: Kind) -> Vec<u8> {
            let objects = self.objects.borrow();
            let (k, body) = objects.get(id).unwrap_or_else(|| panic!("missing {id}"));
            assert_eq!(*k, kind);
            body.clone()
        }
        fn commit_files(&self, files: &BTreeMap<String, String>, message: &str) {
            let (tree, trees) = gitpack::trees(files).unwrap();
            for (_, body) in &trees {
                self.store(Kind::Tree, body);
            }
            let parent = self.head.borrow().clone();
            let parent = (!parent.is_empty()).then_some(parent.as_str());
            let commit = gitpack::commit(&tree, parent, message, 0);
            let id = self.store(Kind::Commit, &commit);
            *self.head.borrow_mut() = id;
        }
        /// Every file at `commit`; panics if an object is missing.
        fn files_at(&self, commit: &str) -> BTreeMap<String, String> {
            let body = self.object(commit, Kind::Commit);
            let text = String::from_utf8(body).unwrap();
            let tree = text.lines().next().unwrap().strip_prefix("tree ").unwrap();
            let mut out = BTreeMap::new();
            self.walk(tree, "", &mut out);
            out
        }
        fn walk(&self, tree: &str, prefix: &str, out: &mut BTreeMap<String, String>) {
            let body = self.object(tree, Kind::Tree);
            let mut rest = body.as_slice();
            while !rest.is_empty() {
                let nul = rest.iter().position(|&b| b == 0).unwrap();
                let entry = std::str::from_utf8(&rest[..nul]).unwrap();
                let (mode, name) = entry.split_once(' ').unwrap();
                let id: String = rest[nul + 1..nul + 21]
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                rest = &rest[nul + 21..];
                let path = format!("{prefix}{name}");
                if mode == "40000" {
                    self.walk(&id, &format!("{path}/"), out);
                } else {
                    self.object(&id, Kind::Blob);
                    out.insert(path, id);
                }
            }
        }
        fn latest(&self) -> BTreeMap<String, String> {
            self.files_at(&self.head.borrow().clone())
        }
        fn put(&self, path: &str, body: &str) {
            let id = self.store(Kind::Blob, body.as_bytes());
            let mut files = self.latest();
            files.insert(path.into(), id);
            self.commit_files(&files, "Another machine");
        }
        fn body(&self, path: &str) -> Option<String> {
            let id = self.latest().get(path)?.clone();
            Some(String::from_utf8(self.object(&id, Kind::Blob)).unwrap())
        }
        fn message(&self) -> String {
            let body = self.object(&self.head.borrow().clone(), Kind::Commit);
            let text = String::from_utf8(body).unwrap();
            text.split_once("\n\n").unwrap().1.trim().to_owned()
        }
    }

    fn report(lines: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for l in lines {
            out.extend_from_slice(format!("{:04x}{l}\n", l.len() + 5).as_bytes());
        }
        out.extend_from_slice(b"0000");
        out
    }

    impl Remote for FakeRepo {
        fn head(&self) -> Result<String, String> {
            Ok(self.head.borrow().clone())
        }
        fn files(&self, commit: &str) -> Result<Vec<RemoteFile>, String> {
            Ok(self
                .files_at(commit)
                .into_iter()
                .map(|(path, id)| RemoteFile { path, id })
                .collect())
        }
        fn download(&self, id: &str) -> Result<Vec<u8>, String> {
            self.downloads.set(self.downloads.get() + 1);
            Ok(self.object(id, Kind::Blob))
        }
        fn archive(&self, commit: &str) -> Result<Box<dyn Read + '_>, String> {
            self.archives.set(self.archives.get() + 1);
            let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            let mut tar = tar::Builder::new(gz);
            let top = format!("josh-backup-{}", &commit[..7]);
            let skip = self.archive_skips.borrow().clone();
            for (path, id) in self.files_at(commit) {
                if skip.as_deref() == Some(path.as_str()) {
                    continue;
                }
                let body = self.object(&id, Kind::Blob);
                let mut header = tar::Header::new_gnu();
                header.set_size(body.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append_data(&mut header, format!("{top}/{path}"), body.as_slice())
                    .unwrap();
            }
            let bytes = tar.into_inner().unwrap().finish().unwrap();
            Ok(Box::new(std::io::Cursor::new(bytes)))
        }
        fn receive_pack(&self, body: Vec<u8>) -> Result<Vec<u8>, String> {
            self.pushes.set(self.pushes.get() + 1);
            if self.drop_push.get() == Some(self.pushes.get()) {
                return Err("The connection to GitHub dropped.".into());
            }
            let len = usize::from_str_radix(std::str::from_utf8(&body[..4]).unwrap(), 16).unwrap();
            let line = std::str::from_utf8(&body[4..len]).unwrap();
            let (command, caps) = line.split_once('\0').unwrap();
            assert!(caps.contains("report-status"));
            let mut parts = command.split(' ');
            let (old, new, branch) = (
                parts.next().unwrap(),
                parts.next().unwrap(),
                parts.next().unwrap(),
            );
            assert_eq!(branch, "refs/heads/main");
            assert_eq!(&body[len..len + 4], b"0000");
            if let Some((path, text)) = self.race.borrow_mut().take() {
                self.put(&path, &text);
            }
            for (kind, object) in gitpack::unpack(&body[len + 4..]) {
                if kind == Kind::Blob {
                    self.received.set(self.received.get() + 1);
                }
                self.store(kind, &object);
            }
            if old != *self.head.borrow() {
                return Ok(report(&[
                    "unpack ok",
                    "ng refs/heads/main failed to update ref",
                ]));
            }
            // Every object the new commit needs is here.
            self.files_at(new);
            *self.head.borrow_mut() = new.to_owned();
            Ok(report(&["unpack ok", "ok refs/heads/main"]))
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
                        disks: Vec::new(),
                    },
                    0,
                ),
            })
            .to_string(),
        );
    }

    /// A machine syncing with the repo.
    struct Machine {
        dir: PathBuf,
        id: &'static str,
        name: &'static str,
        settings: GitHubSettings,
        index: FileIndex,
        heard: Vec<Progress>,
    }

    impl Machine {
        fn new(root: &Path, id: &'static str, name: &'static str) -> Self {
            Self {
                dir: root.join(id),
                id,
                name,
                settings: GitHubSettings::default(),
                index: FileIndex::default(),
                heard: Vec::new(),
            }
        }
        fn sync(&mut self, repo: &dyn Remote, now: i64) -> Result<GitHubReport, String> {
            self.heard.clear();
            let heard = &mut self.heard;
            sync(
                repo,
                &self.dir,
                self.id,
                self.name,
                &mut self.settings,
                &mut self.index,
                now,
                &mut |p| heard.push(p),
            )
        }
    }

    /// A real Git repo, reached through Git's own commands the way the app
    /// reaches GitHub. Pushes are checked as strictly as GitHub checks them.
    struct GitRepo(PathBuf);

    impl GitRepo {
        /// `None` where Git isn't installed.
        fn new(dir: &Path) -> Option<Self> {
            std::process::Command::new("git")
                .arg("--version")
                .output()
                .ok()?;
            let repo = Self(dir.join("repo.git"));
            std::fs::create_dir_all(&repo.0).unwrap();
            repo.git(&["init", "-q", "--bare"], None);
            repo.git(&["config", "receive.fsckObjects", "true"], None);
            repo.git(&["config", "transfer.unpackLimit", "1"], None);
            // GitHub starts a repo with a commit (Remote::head).
            let tree = repo.git(&["mktree"], Some(b""));
            let tree = String::from_utf8(tree).unwrap();
            let commit = repo.git(&["commit-tree", tree.trim(), "-m", "Start"], None);
            let commit = String::from_utf8(commit).unwrap();
            repo.git(&["update-ref", "refs/heads/main", commit.trim()], None);
            Some(repo)
        }

        fn git(&self, args: &[&str], input: Option<&[u8]>) -> Vec<u8> {
            use std::process::{Command, Stdio};
            let mut child = Command::new("git")
                .arg("--git-dir")
                .arg(&self.0)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.com")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.com")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(input.unwrap_or_default()).unwrap();
            drop(stdin);
            let out = child.wait_with_output().unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            out.stdout
        }
    }

    impl Remote for GitRepo {
        fn head(&self) -> Result<String, String> {
            let out = self.git(&["rev-parse", "refs/heads/main"], None);
            Ok(String::from_utf8(out).unwrap().trim().to_owned())
        }
        fn files(&self, commit: &str) -> Result<Vec<RemoteFile>, String> {
            let out = self.git(&["ls-tree", "-r", "-z", commit], None);
            Ok(out
                .split(|&b| b == 0)
                .filter(|e| !e.is_empty())
                .map(|e| {
                    let e = std::str::from_utf8(e).unwrap();
                    let (meta, path) = e.split_once('\t').unwrap();
                    RemoteFile {
                        path: path.to_owned(),
                        id: meta.rsplit(' ').next().unwrap().to_owned(),
                    }
                })
                .collect())
        }
        fn download(&self, id: &str) -> Result<Vec<u8>, String> {
            Ok(self.git(&["cat-file", "blob", id], None))
        }
        fn archive(&self, commit: &str) -> Result<Box<dyn Read + '_>, String> {
            let tgz = self.git(
                &[
                    "archive",
                    "--format=tar.gz",
                    "--prefix=josh-sync-abc/",
                    commit,
                ],
                None,
            );
            Ok(Box::new(std::io::Cursor::new(tgz)))
        }
        fn receive_pack(&self, body: Vec<u8>) -> Result<Vec<u8>, String> {
            let repo = self.0.to_string_lossy().into_owned();
            Ok(self.git(&["receive-pack", "--stateless-rpc", &repo], Some(&body)))
        }
    }

    #[test]
    fn syncs_through_real_git() {
        let tmp = tempfile::tempdir().unwrap();
        let Some(repo) = GitRepo::new(tmp.path()) else {
            return;
        };
        let mut pc = Machine::new(tmp.path(), "pc", "Studio PC");
        backed_up(&pc.dir, "Studio PC", "pc");
        // The same files in two folders make the same tree twice, and the
        // upload goes in parts.
        for folder in ["Reaper/FXChains", "Versions/s/Reaper/FXChains"] {
            for i in 0..5 {
                put(
                    &pc.dir,
                    &format!("Studio PC/Files/{folder}/{i}.RfxChain"),
                    &format!("chain {i}"),
                );
            }
        }
        // A script folder that's a Git checkout of its own stays here.
        put(
            &pc.dir,
            "Studio PC/Files/Reaper/Scripts/mine/.git/config",
            "x",
        );
        let r = pc.sync(&repo, 1).unwrap();
        assert_eq!(r.pushed, 11);
        assert_eq!(
            r.unsafe_paths,
            vec!["Studio PC/Files/Reaper/Scripts/mine/.git/config".to_owned()]
        );
        assert_eq!(pc.heard.last().map(|p| (p.done, p.total)), Some((11, 11)));
        repo.git(&["fsck", "--strict"], None);

        // A new machine brings it all in as one archive.
        let mut mac = Machine::new(tmp.path(), "mac", "MacBook");
        let r = mac.sync(&repo, 2).unwrap();
        assert_eq!(r.pulled, 11);
        assert_eq!(
            std::fs::read_to_string(mac.dir.join("Studio PC/Files/Reaper/FXChains/3.RfxChain"))
                .unwrap(),
            "chain 3"
        );
        // And a change goes back the other way.
        backed_up(&mac.dir, "MacBook", "mac");
        put(&mac.dir, "MacBook/Files/new.txt", "new");
        assert_eq!(mac.sync(&repo, 3).unwrap().pushed, 2);
        pc.sync(&repo, 4).unwrap();
        assert_eq!(
            std::fs::read_to_string(pc.dir.join("MacBook/Files/new.txt")).unwrap(),
            "new"
        );
    }

    #[test]
    fn reads_a_repo_too_big_to_list_at_once() {
        // A tree: "a/x", "a/b/y", "a/b/z", "c".
        let trees: HashMap<&str, Vec<TreeEntry>> = HashMap::from([
            ("root", vec![dir("a", "ta"), file("c", "3")]),
            ("ta", vec![file("x", "1"), dir("b", "tb")]),
            ("tb", vec![file("y", "2"), file("z", "4")]),
        ]);
        fn file(path: &str, id: &str) -> TreeEntry {
            TreeEntry {
                path: path.into(),
                id: id.into(),
                is_tree: false,
            }
        }
        fn dir(path: &str, id: &str) -> TreeEntry {
            TreeEntry {
                path: path.into(),
                id: id.into(),
                is_tree: true,
            }
        }
        fn all(
            trees: &HashMap<&str, Vec<TreeEntry>>,
            t: &str,
            prefix: &str,
            out: &mut Vec<TreeEntry>,
        ) {
            for e in &trees[t] {
                let path = format!("{prefix}{}", e.path);
                out.push(TreeEntry {
                    path: path.clone(),
                    ..e.clone()
                });
                if e.is_tree {
                    all(trees, &e.id, &format!("{path}/"), out);
                }
            }
        }
        // GitHub cuts a whole listing short past `limit` entries.
        for limit in [100, 3, 1] {
            let asked = RefCell::new(0);
            let list = |t: &str, recursive: bool| {
                *asked.borrow_mut() += 1;
                if !recursive {
                    return Ok((trees[t].clone(), false));
                }
                let mut out = Vec::new();
                all(&trees, t, "", &mut out);
                let cut = out.len() > limit;
                out.truncate(limit);
                Ok((out, cut))
            };
            let mut files: Vec<(String, String)> = walk_tree(&list, "root")
                .unwrap()
                .into_iter()
                .map(|f| (f.path, f.id))
                .collect();
            files.sort();
            let want = [("a/b/y", "2"), ("a/b/z", "4"), ("a/x", "1"), ("c", "3")];
            assert_eq!(
                files,
                want.map(|(p, i)| (p.to_owned(), i.to_owned())),
                "limit {limit}"
            );
            if limit == 100 {
                assert_eq!(*asked.borrow(), 1, "one request when it fits");
            }
        }
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
        let repo = FakeRepo::new();
        repo.put("README.md", "Patch Pilot backups");
        let mut mac = Machine::new(tmp.path(), "mac", "MacBook");
        let mut pc = Machine::new(tmp.path(), "pc", "Studio PC");

        backed_up(&mac.dir, "MacBook", "mac");
        let v1 = "MacBook/Files/Reaper/FXChains/Vocal.RfxChain";
        put(&mac.dir, v1, "vocal v1");
        let r = mac.sync(&repo, 10).unwrap();
        assert_eq!((r.pulled, r.pushed), (1, 2));
        assert_eq!(repo.body(v1).as_deref(), Some("vocal v1"));
        assert_eq!(repo.message(), "MacBook: 2 files changed");
        assert_eq!(mac.settings.head, Some(repo.head().unwrap()));
        assert_eq!(mac.settings.synced_at, Some(10));

        // The PC gets the Mac's folder and pushes its own.
        backed_up(&pc.dir, "Studio PC", "pc");
        let r = pc.sync(&repo, 20).unwrap();
        assert_eq!((r.pulled, r.pushed), (3, 1));
        assert_eq!(
            std::fs::read_to_string(below(&pc.dir, v1)).unwrap(),
            "vocal v1"
        );

        // A change on the Mac, moving the old copy to Versions: the old
        // content isn't sent again, and the PC follows.
        let old = "MacBook/Versions/s/Reaper/FXChains/Vocal.RfxChain";
        std::fs::create_dir_all(below(&mac.dir, old).parent().unwrap()).unwrap();
        std::fs::rename(below(&mac.dir, v1), below(&mac.dir, old)).unwrap();
        put(&mac.dir, v1, "vocal v2");
        let before = repo.received.get();
        let r = mac.sync(&repo, 30).unwrap();
        assert_eq!(r.pushed, 2);
        assert_eq!(repo.received.get() - before, 1);
        let r = pc.sync(&repo, 40).unwrap();
        assert_eq!((r.pulled, r.pushed), (2, 0));
        assert_eq!(
            std::fs::read_to_string(below(&pc.dir, v1)).unwrap(),
            "vocal v2"
        );

        // A file gone from the Mac's folder goes from the repo, then the PC.
        std::fs::remove_file(below(&mac.dir, v1)).unwrap();
        let r = mac.sync(&repo, 50).unwrap();
        assert_eq!(r.pushed, 1);
        assert!(repo.body(v1).is_none());
        pc.sync(&repo, 60).unwrap();
        assert!(!below(&pc.dir, v1).exists());
        assert!(below(&pc.dir, old).is_file());
        // Nothing to do: nothing sent.
        let pushes = repo.pushes.get();
        let r = pc.sync(&repo, 70).unwrap();
        assert_eq!((r.pulled, r.pushed), (0, 0));
        assert_eq!(repo.pushes.get(), pushes);
        assert!(pc.heard.is_empty());
    }

    #[test]
    fn never_overwrites_this_machines_folder_and_retries_a_lost_race() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = FakeRepo::new();
        let mut pc = Machine::new(tmp.path(), "pc", "Studio PC");
        // The repo's copy of the PC's folder is older than the PC's own.
        repo.put("Studio PC/Files/a.txt", "old");
        backed_up(&pc.dir, "Studio PC", "pc");
        put(&pc.dir, "Studio PC/Files/a.txt", "new");
        *repo.race.borrow_mut() = Some(("MacBook/Files/b.txt".into(), "mac".into()));
        let r = pc.sync(&repo, 1).unwrap();
        assert_eq!(repo.body("Studio PC/Files/a.txt").as_deref(), Some("new"));
        assert_eq!(repo.body("MacBook/Files/b.txt").as_deref(), Some("mac"));
        assert_eq!(
            std::fs::read_to_string(pc.dir.join("MacBook/Files/b.txt")).unwrap(),
            "mac"
        );
        assert_eq!(r.pushed, 2);
        assert_eq!(repo.pushes.get(), 2);
    }

    #[test]
    fn a_big_first_upload_goes_in_parts_and_picks_up_where_it_stopped() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = FakeRepo::new();
        let mut pc = Machine::new(tmp.path(), "pc", "Studio PC");
        backed_up(&pc.dir, "Studio PC", "pc");
        for i in 0..10 {
            put(
                &pc.dir,
                &format!("Studio PC/Files/Presets/{i}.fxp"),
                &format!("preset {i}"),
            );
        }
        // 11 files go 4 at a time; the line drops on the second part.
        repo.drop_push.set(Some(2));
        let e = pc.sync(&repo, 1).unwrap_err();
        assert!(e.contains("dropped"), "{e}");
        assert_eq!(repo.latest().len(), 4);
        // The backup's list of files goes last.
        assert!(
            repo.latest()
                .keys()
                .all(|p| p.starts_with("Studio PC/Files/"))
        );
        assert_eq!(repo.message(), "Studio PC: 4 files changed, more to come");
        assert_eq!(pc.settings.head, Some(repo.head().unwrap()));
        let p = |done, total| Progress {
            uploading: true,
            done,
            total,
        };
        assert_eq!(pc.heard, vec![p(0, 11), p(4, 11), p(4, 11)]);

        // The next try sends only what's left.
        let r = pc.sync(&repo, 2).unwrap();
        assert_eq!(r.pushed, 7);
        assert_eq!(repo.latest().len(), 11);
        assert_eq!(repo.received.get(), 11);
        assert_eq!(pc.heard, vec![p(0, 7), p(4, 7), p(4, 7), p(7, 7)]);
        assert_eq!(repo.message(), "Studio PC: 3 files changed");
        assert_eq!(pc.settings.synced_at, Some(2));
    }

    #[test]
    fn a_big_pull_comes_as_one_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = FakeRepo::new();
        for i in 0..10 {
            repo.put(
                &format!("Studio PC/Files/Presets/{i}.fxp"),
                &format!("preset {i}"),
            );
        }
        *repo.archive_skips.borrow_mut() = Some("Studio PC/Files/Presets/3.fxp".into());
        let mut mac = Machine::new(tmp.path(), "mac", "MacBook");
        let r = mac.sync(&repo, 1).unwrap();
        assert_eq!(r.pulled, 10);
        assert_eq!((repo.archives.get(), repo.downloads.get()), (1, 1));
        for i in 0..10 {
            let f = mac.dir.join(format!("Studio PC/Files/Presets/{i}.fxp"));
            assert_eq!(std::fs::read_to_string(f).unwrap(), format!("preset {i}"));
        }
        assert_eq!(
            mac.heard.first(),
            Some(&Progress {
                uploading: false,
                done: 0,
                total: 10
            })
        );

        // A few changes come one by one.
        repo.put("Studio PC/Files/Presets/3.fxp", "preset 3b");
        mac.sync(&repo, 2).unwrap();
        assert_eq!((repo.archives.get(), repo.downloads.get()), (1, 2));
    }

    #[test]
    fn a_new_copy_gets_this_machines_folder_back() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = FakeRepo::new();
        let mut pc = Machine::new(tmp.path(), "pc", "Studio PC");
        backed_up(&pc.dir, "Studio PC", "pc");
        put(&pc.dir, "Studio PC/Files/a.txt", "a");
        pc.sync(&repo, 1).unwrap();

        // The app-data copy is gone (a reinstall): nothing is removed from
        // the repo, and the copy is whole again.
        let mut fresh = Machine::new(&tmp.path().join("fresh"), "pc", "Studio PC");
        let r = fresh.sync(&repo, 2).unwrap();
        assert_eq!(r.pushed, 0);
        assert_eq!(
            std::fs::read_to_string(fresh.dir.join("Studio PC/Files/a.txt")).unwrap(),
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
        let r = Machine::new(tmp.path(), "pc", "PC").sync(&repo, 1).unwrap();
        assert_eq!(
            r.unsafe_paths,
            vec!["MacBook/Files/bad:name.txt".to_owned()]
        );
        assert!(tmp.path().join("pc/MacBook/Files/ok.txt").is_file());
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
