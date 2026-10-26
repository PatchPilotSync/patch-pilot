//! Just enough of Git's object and wire formats to push a commit over smart
//! HTTP (decision #73): blob, tree and commit objects, a pack of them
//! (whole objects, no deltas) and the `git-receive-pack` request and its
//! status report. GitHub's REST API makes one request per file, which its
//! rate limits stop after a few hundred; a pack carries thousands in one.

use std::collections::BTreeMap;
use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Commit = 1,
    Tree = 2,
    Blob = 3,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Commit => "commit",
            Kind::Tree => "tree",
            Kind::Blob => "blob",
        }
    }
}

/// A Git object's ID (hex SHA-1).
pub fn object_id(kind: Kind, body: &[u8]) -> String {
    let mut h = sha1_smol::Sha1::new();
    h.update(format!("{} {}\0", kind.name(), body.len()).as_bytes());
    h.update(body);
    h.digest().to_string()
}

fn hex_bytes(id: &str) -> Result<[u8; 20], String> {
    let mut out = [0u8; 20];
    if id.len() != 40 {
        return Err(format!("not an object ID: {id}"));
    }
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&id[i * 2..i * 2 + 2], 16)
            .map_err(|_| format!("not an object ID: {id}"))?;
    }
    Ok(out)
}

enum Node {
    File(String),
    Dir(BTreeMap<String, Node>),
}

/// Objects as (ID, body).
pub type Objects = Vec<(String, Vec<u8>)>;

/// The tree objects for `files` (`/`-separated path to blob ID): the root
/// tree's ID, and every tree.
pub fn trees(files: &BTreeMap<String, String>) -> Result<(String, Objects), String> {
    let mut root = BTreeMap::new();
    for (path, id) in files {
        let parts: Vec<&str> = path.split('/').collect();
        let (name, dirs) = parts.split_last().ok_or("empty path")?;
        let mut dir = &mut root;
        for d in dirs {
            let node = dir
                .entry((*d).to_owned())
                .or_insert_with(|| Node::Dir(BTreeMap::new()));
            dir = match node {
                Node::Dir(children) => children,
                Node::File(_) => return Err(format!("{path}: a file is in the way")),
            };
        }
        dir.insert((*name).to_owned(), Node::File(id.clone()));
    }
    let mut out = Vec::new();
    let id = write_tree(&root, &mut out)?;
    Ok((id, out))
}

fn write_tree(dir: &BTreeMap<String, Node>, out: &mut Objects) -> Result<String, String> {
    // Git sorts a tree's entries by name, a folder's name as if it ended in "/".
    let mut entries: Vec<(Vec<u8>, &str, String)> = Vec::new();
    for (name, node) in dir {
        match node {
            Node::File(id) => entries.push((name.as_bytes().to_vec(), "100644", id.clone())),
            Node::Dir(children) => {
                let id = write_tree(children, out)?;
                let mut key = name.as_bytes().to_vec();
                key.push(b'/');
                entries.push((key, "40000", id));
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut body = Vec::new();
    for (key, mode, id) in &entries {
        let name = key.strip_suffix(b"/").unwrap_or(key);
        body.extend_from_slice(mode.as_bytes());
        body.push(b' ');
        body.extend_from_slice(name);
        body.push(0);
        body.extend_from_slice(&hex_bytes(id)?);
    }
    let id = object_id(Kind::Tree, &body);
    out.push((id.clone(), body));
    Ok(id)
}

/// A commit object's body, by Patch Pilot, at `time` (Unix seconds, UTC).
pub fn commit(tree: &str, parent: Option<&str>, message: &str, time: i64) -> Vec<u8> {
    let who = format!("Patch Pilot <patchpilot@users.noreply.github.com> {time} +0000");
    let mut s = format!("tree {tree}\n");
    if let Some(p) = parent {
        s.push_str(&format!("parent {p}\n"));
    }
    s.push_str(&format!("author {who}\ncommitter {who}\n\n{message}\n"));
    s.into_bytes()
}

/// A pack of whole objects.
pub fn pack(objects: &[(Kind, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"PACK");
    out.extend_from_slice(&2u32.to_be_bytes());
    out.extend_from_slice(&(objects.len() as u32).to_be_bytes());
    for (kind, body) in objects {
        let mut size = body.len();
        let mut byte = ((*kind as u8) << 4) | (size & 0x0f) as u8;
        size >>= 4;
        while size > 0 {
            out.push(byte | 0x80);
            byte = (size & 0x7f) as u8;
            size >>= 7;
        }
        out.push(byte);
        let mut z = ZlibEncoder::new(Vec::new(), Compression::fast());
        z.write_all(body).expect("writing to memory");
        out.extend_from_slice(&z.finish().expect("writing to memory"));
    }
    let mut h = sha1_smol::Sha1::new();
    h.update(&out);
    out.extend_from_slice(&h.digest().bytes());
    out
}

fn pkt_line(s: &str) -> Vec<u8> {
    let mut out = format!("{:04x}", s.len() + 4).into_bytes();
    out.extend_from_slice(s.as_bytes());
    out
}

/// The body of a `git-receive-pack` request moving `branch` from `old` to
/// `new`, carrying `pack`.
pub fn receive_pack_request(old: &str, new: &str, branch: &str, pack: &[u8]) -> Vec<u8> {
    let mut out = pkt_line(&format!(
        "{old} {new} refs/heads/{branch}\0report-status agent=patch-pilot\n"
    ));
    out.extend_from_slice(b"0000");
    out.extend_from_slice(pack);
    out
}

/// What the server said about a push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushStatus {
    Ok,
    /// The branch moved meanwhile.
    Moved,
    Rejected(String),
}

/// Read a `report-status` response.
pub fn push_status(response: &[u8]) -> PushStatus {
    let mut lines = Vec::new();
    let mut rest = response;
    while rest.len() >= 4 {
        let Some(len) = std::str::from_utf8(&rest[..4])
            .ok()
            .and_then(|h| usize::from_str_radix(h, 16).ok())
        else {
            break;
        };
        if len == 0 {
            rest = &rest[4..];
            continue;
        }
        if len < 4 || len > rest.len() {
            break;
        }
        lines.push(String::from_utf8_lossy(&rest[4..len]).trim_end().to_owned());
        rest = &rest[len..];
    }
    if let Some(unpack) = lines.iter().find(|l| l.starts_with("unpack ")) {
        if unpack != "unpack ok" {
            return PushStatus::Rejected(unpack.clone());
        }
    } else {
        let text = String::from_utf8_lossy(response).trim().to_owned();
        return PushStatus::Rejected(if text.is_empty() {
            "GitHub didn't say whether it took the files.".into()
        } else {
            text
        });
    }
    match lines.iter().find(|l| l.starts_with("ng ")) {
        None if lines.iter().any(|l| l.starts_with("ok ")) => PushStatus::Ok,
        None => PushStatus::Rejected("GitHub didn't update the branch.".into()),
        Some(ng) => {
            let why = ng.splitn(3, ' ').nth(2).unwrap_or_default().to_owned();
            if ["fast-forward", "fetch first", "stale info", "lock"]
                .iter()
                .any(|w| why.contains(w))
            {
                PushStatus::Moved
            } else {
                PushStatus::Rejected(why)
            }
        }
    }
}

/// Read a pack of whole objects (what [`pack`] writes); for tests.
#[cfg(test)]
pub fn unpack(pack: &[u8]) -> Vec<(Kind, Vec<u8>)> {
    use std::io::Read;
    assert_eq!(&pack[..4], b"PACK");
    let count = u32::from_be_bytes(pack[8..12].try_into().unwrap());
    let mut pos = 12;
    let mut out = Vec::new();
    for _ in 0..count {
        let first = pack[pos];
        let kind = match (first >> 4) & 7 {
            1 => Kind::Commit,
            2 => Kind::Tree,
            3 => Kind::Blob,
            k => panic!("object type {k}"),
        };
        let mut size = (first & 0x0f) as usize;
        let mut shift = 4;
        let mut byte = first;
        pos += 1;
        while byte & 0x80 != 0 {
            byte = pack[pos];
            size |= ((byte & 0x7f) as usize) << shift;
            shift += 7;
            pos += 1;
        }
        let mut z = flate2::read::ZlibDecoder::new(&pack[pos..]);
        let mut body = Vec::new();
        z.read_to_end(&mut body).unwrap();
        assert_eq!(body.len(), size);
        pos += z.total_in() as usize;
        out.push((kind, body));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_git() {
        // `printf 'hello\n' | git hash-object --stdin`
        assert_eq!(
            object_id(Kind::Blob, b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
        // `git mktree < /dev/null`
        let (empty, _) = trees(&BTreeMap::new()).unwrap();
        assert_eq!(empty, "4b825dc642cb6eb9a060e54bf8d69288fbee4904");
    }

    #[test]
    fn trees_match_git() {
        // mkdir a "a b"; printf 'hello\n' > a/x; printf 'hello\n' > a.txt;
        // printf x > "a b/c"; git add .; git write-tree
        let hello = object_id(Kind::Blob, b"hello\n");
        let mut files = BTreeMap::from([
            ("a/x".to_owned(), hello.clone()),
            ("a.txt".to_owned(), hello.clone()),
        ]);
        assert_eq!(
            trees(&files).unwrap().0,
            "a9bae932899dbd616007c2ec028e2f137cef0266"
        );
        files.insert("a b/c".into(), object_id(Kind::Blob, b"x"));
        assert_eq!(
            trees(&files).unwrap().0,
            "550ab1aecc2489fcdd3c491b92ebed26c180f298"
        );
    }

    #[test]
    fn packs_round_trip() {
        let big = vec![7u8; 100_000];
        let p = pack(&[(Kind::Blob, b"hello\n"), (Kind::Blob, &big)]);
        let back = unpack(&p);
        assert_eq!(back[0], (Kind::Blob, b"hello\n".to_vec()));
        assert_eq!(back[1], (Kind::Blob, big));
    }

    #[test]
    fn reads_push_status() {
        let lines = |ls: &[&str]| {
            let mut v: Vec<u8> = ls
                .iter()
                .flat_map(|l| pkt_line(&format!("{l}\n")))
                .collect();
            v.extend_from_slice(b"0000");
            v
        };
        assert_eq!(
            push_status(&lines(&["unpack ok", "ok refs/heads/main"])),
            PushStatus::Ok
        );
        assert_eq!(
            push_status(&lines(&[
                "unpack ok",
                "ng refs/heads/main non-fast-forward"
            ])),
            PushStatus::Moved
        );
        assert_eq!(
            push_status(&lines(&[
                "unpack ok",
                "ng refs/heads/main protected branch hook declined"
            ])),
            PushStatus::Rejected("protected branch hook declined".into())
        );
        assert!(matches!(
            push_status(&lines(&["unpack index-pack failed"])),
            PushStatus::Rejected(_)
        ));
    }
    /// Real Git takes what Patch Pilot pushes. Skipped where Git isn't installed.
    #[test]
    fn real_git_takes_the_push() {
        use std::process::{Command, Stdio};
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("r.git");
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("--git-dir")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap()
        };
        assert!(
            Command::new("git")
                .args(["init", "-q", "--bare"])
                .arg(&repo)
                .status()
                .unwrap()
                .success()
        );
        let push = |old: &str, files: &[(&str, &[u8])], parent: Option<&str>| {
            let map: BTreeMap<String, String> = files
                .iter()
                .map(|(p, b)| ((*p).to_owned(), object_id(Kind::Blob, b)))
                .collect();
            let (tree, trees) = trees(&map).unwrap();
            let c = commit(&tree, parent, "Studio PC: 2 files changed", 1_761_000_000);
            let id = object_id(Kind::Commit, &c);
            let mut objects: Vec<(Kind, &[u8])> =
                files.iter().map(|(_, b)| (Kind::Blob, *b)).collect();
            objects.extend(trees.iter().map(|(_, b)| (Kind::Tree, b.as_slice())));
            objects.push((Kind::Commit, &c));
            let body = receive_pack_request(old, &id, "main", &pack(&objects));
            let mut child = Command::new("git")
                .args(["receive-pack", "--stateless-rpc"])
                .arg(&repo)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(&body).unwrap();
            let out = child.wait_with_output().unwrap();
            (id, push_status(&out.stdout))
        };
        let zero = "0".repeat(40);
        let (first, status) = push(
            &zero,
            &[("Studio PC/a b.txt", b"hello\n"), ("Studio PC/a/x", b"x")],
            None,
        );
        assert_eq!(status, PushStatus::Ok);
        assert_eq!(git(&["rev-parse", "main"]).trim(), first);
        let (second, status) = push(&first, &[("Studio PC/a b.txt", b"bye\n")], Some(&first));
        assert_eq!(status, PushStatus::Ok);
        assert_eq!(
            git(&["ls-tree", "-r", "--name-only", "main"]),
            "Studio PC/a b.txt\n"
        );
        git(&["fsck", "--strict"]);
        // A push on a commit that's no longer the latest: Git says "failed
        // to update ref", so sync looks at the branch again.
        let (_, status) = push(&first, &[("Studio PC/c", b"c")], Some(&first));
        assert_ne!(status, PushStatus::Ok);
        assert_eq!(git(&["rev-parse", "main"]).trim(), second);
    }
}
