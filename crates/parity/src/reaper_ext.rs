//! Reaper extensions (decision #104): the `reaper_*.dll` and `reaper_*.dylib`
//! files in `UserPlugins/` that add to Reaper's script API (SWS, ReaPack,
//! js_ReaScriptAPI, ReaImGui), which scripts call them, and whether each one
//! here can actually load.
//!
//! Extensions are software built for one OS and CPU, so they are never copied
//! between machines (CLAUDE.md rule 2): a missing one is a download from its
//! project, for this machine's build. Only file names and the first bytes of
//! each file (its Mach-O or PE header) are read; nothing is loaded (rule 1).

use std::collections::{BTreeMap, BTreeSet};

use patchpilot_core::{Arch, Confidence, Item, ItemKind, Os, PortablePath, Product, ProductId};

use crate::reaper::{REAPER_RESOURCES, slug};

/// Product IDs of extensions start with this: `reaper-ext/sws`.
pub const PREFIX: &str = "reaper-ext/";

/// The resource subfolder Reaper loads extensions from. Only files at its
/// top load; Reaper doesn't look in subfolders.
pub const USER_PLUGINS: &str = "UserPlugins";

/// How much of each file is read: enough for any Mach-O or PE header.
pub const HEAD_BYTES: usize = 64 * 1024;

/// The CPU a build is for, or Reaper runs as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cpu {
    /// 32-bit Intel.
    X86,
    X86_64,
    Arm64,
}

impl Cpu {
    /// What a build for it is called on `os`.
    fn label(self, os: Os) -> &'static str {
        match (self, os) {
            (Self::X86, _) => "32-bit",
            (Self::X86_64, Os::Macos) => "Intel",
            (Self::X86_64, Os::Windows) => "64-bit",
            (Self::Arm64, Os::Macos) => "Apple silicon",
            (Self::Arm64, Os::Windows) => "ARM",
        }
    }

    /// How Reaper runs when it runs as this.
    fn reaper_runs(self, os: Os) -> &'static str {
        match (self, os) {
            (Self::Arm64, Os::Macos) => "Reaper here runs natively on Apple silicon",
            (Self::X86_64, Os::Macos) => "Reaper here runs as an Intel app",
            (Self::X86_64, Os::Windows) => "Reaper here is 64-bit",
            (Self::X86, _) => "Reaper here is 32-bit",
            (Self::Arm64, Os::Windows) => "Reaper here runs on ARM",
        }
    }
}

/// An extension Patch Pilot knows by name.
#[derive(Debug)]
pub struct Known {
    /// The file name's stem without `reaper_` and the CPU suffix, lowercase.
    pub key: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    /// Prefixes of the API functions it adds, as scripts call them.
    api: &'static [&'static str],
    /// Where its project offers the downloads.
    pub page: &'static str,
    /// ReaPack's default ReaTeam Extensions repository has it, so ReaPack
    /// installs it from Browse packages.
    pub reapack: bool,
    /// The file each build is named, per OS and CPU.
    files: &'static [(Os, Cpu, &'static str)],
}

pub const KNOWN: &[Known] = &[
    Known {
        key: "sws",
        name: "SWS Extension",
        vendor: "SWS",
        api: &["BR_", "CF_", "FNG_", "NF_", "SNM_", "SN_", "ULT_", "Xen_"],
        page: "https://www.sws-extension.org/",
        reapack: false,
        files: &[
            (Os::Windows, Cpu::X86_64, "reaper_sws-x64.dll"),
            (Os::Windows, Cpu::X86, "reaper_sws-x86.dll"),
            (Os::Macos, Cpu::Arm64, "reaper_sws-arm64.dylib"),
            (Os::Macos, Cpu::X86_64, "reaper_sws-x86_64.dylib"),
        ],
    },
    Known {
        key: "reapack",
        name: "ReaPack",
        vendor: "cfillion",
        api: &["ReaPack_"],
        page: "https://reapack.com/",
        reapack: false,
        files: &[
            (Os::Windows, Cpu::X86_64, "reaper_reapack-x64.dll"),
            (Os::Windows, Cpu::X86, "reaper_reapack-x86.dll"),
            (Os::Macos, Cpu::Arm64, "reaper_reapack-arm64.dylib"),
            (Os::Macos, Cpu::X86_64, "reaper_reapack-x86_64.dylib"),
        ],
    },
    Known {
        key: "js_reascriptapi",
        name: "js_ReaScriptAPI",
        vendor: "juliansader",
        api: &["JS_"],
        page: "https://github.com/juliansader/ReaExtensions",
        reapack: true,
        files: &[
            (Os::Windows, Cpu::X86_64, "reaper_js_ReaScriptAPI64.dll"),
            (Os::Windows, Cpu::X86, "reaper_js_ReaScriptAPI32.dll"),
            (Os::Macos, Cpu::Arm64, "reaper_js_ReaScriptAPI64ARM.dylib"),
            (Os::Macos, Cpu::X86_64, "reaper_js_ReaScriptAPI64.dylib"),
        ],
    },
    Known {
        key: "imgui",
        name: "ReaImGui",
        vendor: "cfillion",
        api: &["ImGui_"],
        page: "https://github.com/cfillion/reaimgui",
        reapack: true,
        files: &[
            (Os::Windows, Cpu::X86_64, "reaper_imgui-x64.dll"),
            (Os::Windows, Cpu::X86, "reaper_imgui-x86.dll"),
            (Os::Macos, Cpu::Arm64, "reaper_imgui-arm64.dylib"),
            (Os::Macos, Cpu::X86_64, "reaper_imgui-x86_64.dylib"),
        ],
    },
];

impl Known {
    pub fn product_id(&self) -> ProductId {
        product_id(self.key)
    }

    /// The file this machine's build is named, e.g. `reaper_sws-arm64.dylib`.
    pub fn file_for(&self, os: Os, cpu: Cpu) -> Option<&'static str> {
        self.files
            .iter()
            .find(|(o, c, _)| *o == os && *c == cpu)
            .map(|(_, _, f)| *f)
    }
}

/// `reaper-ext/<key>`.
pub fn product_id(key: &str) -> ProductId {
    ProductId(format!("{PREFIX}{}", slug(key)))
}

pub fn is_extension(id: &ProductId) -> bool {
    id.0.starts_with(PREFIX)
}

/// The known extension with this product ID.
pub fn known(id: &ProductId) -> Option<&'static Known> {
    KNOWN.iter().find(|k| k.product_id() == *id)
}

/// What an extension file's name says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtName {
    /// Lowercase stem without `reaper_`, the CPU suffix or a copy mark:
    /// `sws`, `js_reascriptapi`.
    pub key: String,
    /// The OS its file type is for: `.dll` Windows, `.dylib` macOS.
    pub os: Os,
}

impl ExtName {
    pub fn product_id(&self) -> ProductId {
        product_id(&self.key)
    }

    /// The known extension's name, else the key as written.
    pub fn display(&self) -> String {
        KNOWN
            .iter()
            .find(|k| k.key == self.key)
            .map_or_else(|| self.key.clone(), |k| k.name.to_owned())
    }
}

/// CPU suffixes projects put on their builds' names, longest first so
/// `x86_64` isn't read as `64`.
const CPU_SUFFIXES: [&str; 13] = [
    "arm64ec", "aarch64", "x86_64", "x86-64", "64arm", "arm64", "amd64", "i386", "i686", "x64",
    "x86", "64", "32",
];

/// Parse an extension file name, e.g. `reaper_sws-arm64.dylib` or
/// `reaper_js_ReaScriptAPI64.dll`. `None` for anything Reaper wouldn't load
/// as an extension: names not starting `reaper_`, other file types.
pub fn parse_file_name(name: &str) -> Option<ExtName> {
    let lower = name.to_ascii_lowercase();
    let (stem, os) = match lower.strip_suffix(".dll") {
        Some(s) => (s, Os::Windows),
        None => (lower.strip_suffix(".dylib")?, Os::Macos),
    };
    let stem = stem.strip_prefix("reaper_")?;
    // A second download: `reaper_sws-arm64 (1).dylib`, `... copy.dylib`.
    let stem = match stem.rsplit_once(" (") {
        Some((s, n))
            if n.strip_suffix(')')
                .is_some_and(|n| n.parse::<u32>().is_ok()) =>
        {
            s
        }
        _ => stem,
    };
    let stem = stem
        .strip_suffix(" copy")
        .unwrap_or(stem)
        .trim_end_matches([' ', '-', '_']);
    let key = CPU_SUFFIXES
        .iter()
        .find_map(|s| stem.strip_suffix(s))
        .unwrap_or(stem)
        .trim_matches(['-', '_', ' ', '.']);
    (!key.is_empty()).then(|| ExtName {
        key: key.to_owned(),
        os,
    })
}

/// What a file's header says it is built for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binary {
    pub os: Os,
    /// Empty when the header names a CPU Reaper doesn't run on.
    pub cpus: Vec<Cpu>,
}

/// Read a Mach-O (thin or universal) or PE header from a file's first bytes.
/// `None` when it is neither: an incomplete download, a cloud placeholder,
/// a web page saved under the extension's name.
pub fn read_binary(head: &[u8]) -> Option<Binary> {
    let u32_le = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(head.get(at..at + 4)?.try_into().ok()?))
    };
    let u32_be = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(head.get(at..at + 4)?.try_into().ok()?))
    };
    let mach_cpu = |t: u32| match t {
        7 => Some(Cpu::X86),
        0x0100_0007 => Some(Cpu::X86_64),
        0x0100_000C => Some(Cpu::Arm64),
        _ => None,
    };
    match head.get(..4)? {
        [0xCF, 0xFA, 0xED, 0xFE] | [0xCE, 0xFA, 0xED, 0xFE] => Some(Binary {
            os: Os::Macos,
            cpus: mach_cpu(u32_le(4)?).into_iter().collect(),
        }),
        [0xCA, 0xFE, 0xBA, magic @ (0xBE | 0xBF)] => {
            let n = u32_be(4)? as usize;
            let size = if *magic == 0xBE { 20 } else { 32 };
            if n == 0 || n > 16 {
                return None;
            }
            let mut cpus: Vec<Cpu> = (0..n)
                .map(|i| u32_be(8 + i * size))
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .filter_map(mach_cpu)
                .collect();
            cpus.sort();
            cpus.dedup();
            Some(Binary {
                os: Os::Macos,
                cpus,
            })
        }
        [b'M', b'Z', ..] => {
            let pe = u32_le(0x3C)? as usize;
            if head.get(pe..pe + 4)? != b"PE\0\0" {
                return None;
            }
            let machine = u16::from_le_bytes(head.get(pe + 4..pe + 6)?.try_into().ok()?);
            let cpu = match machine {
                0x014C => Some(Cpu::X86),
                0x8664 => Some(Cpu::X86_64),
                0xAA64 => Some(Cpu::Arm64),
                _ => None,
            };
            Some(Binary {
                os: Os::Windows,
                cpus: cpu.into_iter().collect(),
            })
        }
        _ => None,
    }
}

/// The CPU Reaper runs as on this machine. On an Apple silicon Mac Reaper
/// can run under Rosetta as Intel; each build keeps its own plug-in list,
/// `reaper-vstplugins_arm64.ini` or `reaper-vstplugins64.ini`, so the one
/// written last (Unix seconds) says which Reaper ran last.
pub fn reaper_cpu(os: Os, arch: Arch, arm64_list: Option<i64>, intel_list: Option<i64>) -> Cpu {
    match (os, arch) {
        (Os::Macos, Arch::Aarch64 | Arch::Universal) => match (arm64_list, intel_list) {
            (None, Some(_)) => Cpu::X86_64,
            (Some(a), Some(i)) if i > a => Cpu::X86_64,
            _ => Cpu::Arm64,
        },
        // ARM Windows runs x64 builds too (arm64ec); 32-bit Reaper is rare.
        _ => Cpu::X86_64,
    }
}

/// The extensions a script calls, from its text: `reaper.BR_GetMouseCursorContext`
/// or `reaper.APIExists("CF_GetSWSVersion")` in Lua, `JS_Window_Find(...)` in
/// EEL or Python. Other files need none.
pub fn script_needs(rel: &str, bytes: &[u8]) -> Vec<ProductId> {
    let lower = rel.to_ascii_lowercase();
    let lua = lower.ends_with(".lua");
    if !(lua || lower.ends_with(".eel") || lower.ends_with(".py")) {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(bytes);
    let b = text.as_bytes();
    let is_ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut found: BTreeSet<&'static str> = BTreeSet::new();
    let mut i = 0;
    while i < b.len() {
        if !is_ident(b[i]) || (i > 0 && is_ident(b[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && is_ident(b[i]) {
            i += 1;
        }
        let word = &text[start..i];
        if word.len() < 4 || !word.contains('_') {
            continue;
        }
        let Some(k) = KNOWN.iter().find(|k| {
            k.api.iter().any(|p| {
                word.strip_prefix(p)
                    .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_uppercase()))
            })
        }) else {
            continue;
        };
        if found.contains(k.key) {
            continue;
        }
        let before = &text[..start];
        let quoted =
            start > 0 && matches!(b[start - 1], b'"' | b'\'') && b.get(i) == Some(&b[start - 1]);
        let called =
            lua && before.ends_with("reaper.") || !lua && text[i..].trim_start().starts_with('(');
        if called || quoted {
            found.insert(k.key);
        }
    }
    found.into_iter().map(product_id).collect()
}

/// One extension file, as a manifest item: a `vendor_app`-kind item (installed
/// software, never copied) at `REAPER_RESOURCES/UserPlugins/<name>`.
pub fn extension_item(
    name: &ExtName,
    file_name: &str,
    binary: &Binary,
    version: Option<String>,
    size: Option<u64>,
    mtime: Option<i64>,
) -> (Product, Item) {
    let id = name.product_id();
    let known = KNOWN.iter().find(|k| k.key == name.key);
    let product = Product {
        canonical_id: id.clone(),
        vendor: known.map_or("", |k| k.vendor).to_owned(),
        name: name.display(),
    };
    let arch = match binary.cpus[..] {
        [Cpu::X86_64] => Some(Arch::X86_64),
        [Cpu::Arm64] => Some(Arch::Aarch64),
        [_, _, ..] => Some(Arch::Universal),
        _ => None,
    };
    let item = Item {
        kind: ItemKind::VendorApp,
        product_id: Some(id),
        format: None,
        platform: binary.os,
        arch,
        version,
        path: PortablePath::logical(REAPER_RESOURCES, format!("{USER_PLUGINS}/{file_name}")),
        content_hash: None,
        size,
        mtime,
        platform_portable: false,
        confidence: if known.is_some() {
            Confidence::High
        } else {
            Confidence::Medium
        },
        requires: Vec::new(),
        needs_files: Vec::new(),
    };
    (product, item)
}

/// A file found in `UserPlugins/` or a subfolder of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundFile {
    /// `/`-separated below `UserPlugins/`: `reaper_sws-arm64.dylib`, or
    /// `sws-2.14/reaper_sws-arm64.dylib` in a subfolder.
    pub rel: String,
    /// `None` when its header isn't a Mach-O or PE one.
    pub binary: Option<Binary>,
    /// macOS put it in quarantine (downloaded) and nobody has allowed it.
    pub quarantined: bool,
    /// Last modification, Unix seconds.
    pub mtime: i64,
}

impl FoundFile {
    fn file_name(&self) -> &str {
        self.rel.rsplit('/').next().unwrap_or(&self.rel)
    }

    fn at_top(&self) -> bool {
        !self.rel.contains('/')
    }

    fn builds_for(&self, os: Os, cpu: Cpu) -> bool {
        self.binary
            .as_ref()
            .is_some_and(|b| b.os == os && b.cpus.contains(&cpu))
    }

    /// Reaper loads it: at the top of `UserPlugins/` and built for this OS
    /// and Reaper's CPU. A quarantined one is still refused by macOS.
    pub fn loads(&self, os: Os, cpu: Cpu) -> bool {
        self.at_top() && self.builds_for(os, cpu)
    }
}

/// Where one extension stands on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// macOS refuses to let Reaper load it until it's allowed.
    Blocked,
    /// In a subfolder of `UserPlugins/`, where Reaper doesn't look.
    InSubfolder,
    /// Built for another CPU than Reaper runs as.
    WrongCpu,
    /// The other OS's build (a `.dll` on a Mac).
    WrongOs,
    /// Not a complete extension file.
    Damaged,
    /// Scripts here call it and it isn't installed.
    Missing,
    /// Two copies load, and the extension complains.
    TwoCopies,
    Ok,
}

/// A one-click fix the app can make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixAction {
    /// Take the file out of macOS's quarantine.
    Unblock,
    /// Move the file to the top of `UserPlugins/`.
    MoveUp,
    /// Remove the older of two copies.
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Fix {
    pub action: FixAction,
    /// The file it acts on, below `UserPlugins/`.
    pub file: String,
}

/// Where to get this machine's build.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Download {
    pub page: String,
    /// The file to put in `UserPlugins/`, e.g. `reaper_sws-arm64.dylib`.
    pub file: Option<String>,
    /// ReaPack installs it (Extensions > ReaPack > Browse packages).
    pub reapack: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExtensionState {
    pub product_id: ProductId,
    pub name: String,
    pub status: Status,
    /// The file it is about, below `UserPlugins/`.
    pub file: Option<String>,
    /// What's wrong and what to do, in a sentence or two. Empty when Ok.
    pub detail: String,
    pub fix: Option<Fix>,
    pub download: Option<Download>,
    /// Scripts that call it, A to Z (paths below the resource folder).
    pub needed_by: Vec<String>,
}

/// Every extension in `files` and every one `needed` (product → scripts
/// that call it) on `os`, where Reaper runs as `cpu`: problems first, then
/// the ones that load, each A to Z. A missing extension ReaPack has is
/// installed from ReaPack when ReaPack loads here.
pub fn check(
    files: &[FoundFile],
    os: Os,
    cpu: Cpu,
    needed: &BTreeMap<ProductId, BTreeSet<String>>,
) -> Vec<ExtensionState> {
    let mut groups: BTreeMap<ProductId, (ExtName, Vec<&FoundFile>)> = BTreeMap::new();
    for f in files {
        if let Some(name) = parse_file_name(f.file_name()) {
            groups
                .entry(name.product_id())
                .or_insert_with(|| (name, Vec::new()))
                .1
                .push(f);
        }
    }
    let reapack_loads = groups
        .get(&product_id("reapack"))
        .is_some_and(|(_, fs)| fs.iter().any(|f| f.loads(os, cpu) && !f.quarantined));

    let mut out: Vec<ExtensionState> = Vec::new();
    for (id, (name, fs)) in &groups {
        let known = KNOWN.iter().find(|k| k.key == name.key);
        let want = known.and_then(|k| k.file_for(os, cpu));
        let download = known.map(|k| Download {
            page: k.page.to_owned(),
            file: want.map(str::to_owned),
            reapack: k.reapack,
        });
        let get = || match want {
            Some(w) => format!("Get {w} instead."),
            None => "Get the build for this machine from its website.".to_owned(),
        };
        let mut state = ExtensionState {
            product_id: id.clone(),
            name: name.display(),
            status: Status::Ok,
            file: None,
            detail: String::new(),
            fix: None,
            download: None,
            needed_by: needed
                .get(id)
                .map(|s| s.iter().cloned().collect())
                .unwrap_or_default(),
        };
        let mut loading: Vec<&FoundFile> =
            fs.iter().copied().filter(|f| f.loads(os, cpu)).collect();
        loading.sort_by_key(|f| (std::cmp::Reverse(f.mtime), f.rel.clone()));
        let allowed: Vec<&FoundFile> = loading.iter().copied().filter(|f| !f.quarantined).collect();
        if !loading.is_empty() && allowed.is_empty() {
            let f = loading[0];
            state.status = Status::Blocked;
            state.file = Some(f.rel.clone());
            state.detail = "macOS blocked it because it was downloaded, so Reaper can't load it. \
                 Unblock it, then quit and reopen Reaper."
                .to_owned();
            state.fix = Some(Fix {
                action: FixAction::Unblock,
                file: f.rel.clone(),
            });
        } else if loading.len() >= 2 {
            // The newest copy stays.
            let old = loading[loading.len() - 1];
            state.status = Status::TwoCopies;
            state.file = Some(old.rel.clone());
            state.detail = format!(
                "Reaper loads both {} and {}, and {} complains it's loaded twice. Remove the older one.",
                loading[0].rel, old.rel, state.name
            );
            state.fix = Some(Fix {
                action: FixAction::Remove,
                file: old.rel.clone(),
            });
        } else if loading.is_empty() {
            let top_names: BTreeSet<String> = fs
                .iter()
                .filter(|f| f.at_top())
                .map(|f| f.file_name().to_lowercase())
                .collect();
            let nested_ok = fs.iter().find(|f| !f.at_top() && f.builds_for(os, cpu));
            let wrong_cpu = |top: bool| {
                fs.iter()
                    .find(|f| f.at_top() == top && f.binary.as_ref().is_some_and(|b| b.os == os))
            };
            let wrong_os = |top: bool| {
                fs.iter()
                    .find(|f| f.at_top() == top && f.binary.as_ref().is_some_and(|b| b.os != os))
            };
            let damaged = fs.iter().find(|f| f.binary.is_none());
            if let Some(f) = nested_ok {
                let folder = f.rel.rsplit_once('/').map_or("", |(d, _)| d);
                state.status = Status::InSubfolder;
                state.detail = format!(
                    "It's in the {folder} folder inside UserPlugins, and Reaper only looks at the top. \
                     Move it up, then reopen Reaper."
                );
                // Never over another file of the same name.
                if !top_names.contains(&f.file_name().to_lowercase()) {
                    state.fix = Some(Fix {
                        action: FixAction::MoveUp,
                        file: f.rel.clone(),
                    });
                }
                state.file = Some(f.rel.clone());
            } else if let Some(f) = wrong_cpu(true).or_else(|| wrong_cpu(false)) {
                let built = f.binary.as_ref().and_then(|b| b.cpus.first()).map_or_else(
                    || "This build is for another kind of processor".to_owned(),
                    |c| format!("This is the {} build", c.label(os)),
                );
                state.status = Status::WrongCpu;
                state.detail = format!("{built}, but {}. {}", cpu.reaper_runs(os), get());
                state.file = Some(f.rel.clone());
                state.download = download.clone();
            } else if let Some(f) = wrong_os(true).or_else(|| wrong_os(false)) {
                let other = match os {
                    Os::Macos => "Windows",
                    Os::Windows => "Mac",
                };
                state.status = Status::WrongOs;
                state.detail = format!(
                    "This is the {other} build, which can't load here. {}",
                    get()
                );
                state.file = Some(f.rel.clone());
                state.download = download.clone();
            } else if let Some(f) = damaged {
                state.status = Status::Damaged;
                state.detail = format!(
                    "The file isn't a complete extension: still downloading, or only online in a cloud folder. \
                     Download it again. {}",
                    match want {
                        Some(w) => format!("This machine needs {w}."),
                        None => String::new(),
                    }
                )
                .trim_end()
                .to_owned();
                state.file = Some(f.rel.clone());
                state.download = download.clone();
            }
        }
        if state.status == Status::Ok {
            state.file = loading.first().map(|f| f.rel.clone());
        }
        out.push(state);
    }

    for (id, scripts) in needed {
        if groups.contains_key(id) {
            continue;
        }
        let Some(k) = known(id) else {
            continue;
        };
        let want = k.file_for(os, cpu);
        let how = if k.reapack && reapack_loads {
            format!(
                "In Reaper, choose Extensions > ReaPack > Browse packages and install {}.",
                k.name
            )
        } else {
            match want {
                Some(w) => format!("Download it and put {w} in UserPlugins, then reopen Reaper."),
                None => "Download it from its website, then reopen Reaper.".to_owned(),
            }
        };
        out.push(ExtensionState {
            product_id: id.clone(),
            name: k.name.to_owned(),
            status: Status::Missing,
            file: None,
            detail: format!(
                "Not installed, and {} here {} it. {how}",
                count(scripts.len(), "script"),
                if scripts.len() == 1 { "uses" } else { "use" },
            ),
            fix: None,
            download: Some(Download {
                page: k.page.to_owned(),
                file: want.map(str::to_owned),
                reapack: k.reapack,
            }),
            needed_by: scripts.iter().cloned().collect(),
        });
    }
    out.sort_by(|a, b| (a.status, &a.name).cmp(&(b.status, &b.name)));
    out
}

fn count(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// Which scripts call which extensions, from a manifest's script items'
/// `requires`.
pub fn needed_by_scripts(m: &patchpilot_core::Manifest) -> BTreeMap<ProductId, BTreeSet<String>> {
    let mut out: BTreeMap<ProductId, BTreeSet<String>> = BTreeMap::new();
    for item in &m.items {
        let PortablePath::Logical { root, rel } = &item.path else {
            continue;
        };
        if root != REAPER_RESOURCES {
            continue;
        }
        for r in item.requires.iter().filter(|r| is_extension(r)) {
            out.entry(r.clone()).or_default().insert(rel.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(s: &str) -> Option<(String, Os)> {
        parse_file_name(s).map(|n| (n.key, n.os))
    }

    #[test]
    fn reads_every_builds_name() {
        let sws = Some(("sws".to_owned(), Os::Macos));
        assert_eq!(name("reaper_sws-arm64.dylib"), sws);
        assert_eq!(name("reaper_sws-x86_64.dylib"), sws);
        assert_eq!(name("reaper_sws64.dylib"), sws);
        assert_eq!(name("reaper_sws-arm64 (1).dylib"), sws);
        assert_eq!(name("reaper_sws-arm64 copy.dylib"), sws);
        assert_eq!(
            name("reaper_sws-x64.dll"),
            Some(("sws".into(), Os::Windows))
        );
        assert_eq!(
            name("reaper_js_ReaScriptAPI64ARM.dylib"),
            Some(("js_reascriptapi".into(), Os::Macos))
        );
        assert_eq!(
            name("reaper_js_ReaScriptAPI64.dll"),
            Some(("js_reascriptapi".into(), Os::Windows))
        );
        assert_eq!(name("reaper_reapack-arm64ec.dll").unwrap().0, "reapack");
        assert_eq!(name("reaper_imgui-x64.dll").unwrap().0, "imgui");
        assert_eq!(name("reaper_kbd_tools-x64.dll").unwrap().0, "kbd_tools");
        assert_eq!(name("sws_python64.py"), None);
        assert_eq!(name("reaper_.dll"), None);
        assert_eq!(name("libsws.dylib"), None);
        // Every known build's own name reads back as its extension.
        for k in KNOWN {
            for (os, _, file) in k.files {
                let n = parse_file_name(file).unwrap();
                assert_eq!((n.key.as_str(), n.os), (k.key, *os), "{file}");
            }
        }
    }

    fn macho(cpu: u32) -> Vec<u8> {
        let mut b = vec![0xCF, 0xFA, 0xED, 0xFE];
        b.extend(cpu.to_le_bytes());
        b.extend([0; 24]);
        b
    }

    fn fat(cpus: &[u32]) -> Vec<u8> {
        let mut b = vec![0xCA, 0xFE, 0xBA, 0xBE];
        b.extend((cpus.len() as u32).to_be_bytes());
        for c in cpus {
            b.extend(c.to_be_bytes());
            b.extend([0; 16]);
        }
        b
    }

    fn pe(machine: u16) -> Vec<u8> {
        let mut b = vec![0u8; 0x80];
        b[0] = b'M';
        b[1] = b'Z';
        b[0x3C] = 0x40;
        b[0x40..0x44].copy_from_slice(b"PE\0\0");
        b[0x44..0x46].copy_from_slice(&machine.to_le_bytes());
        b
    }

    const ARM: u32 = 0x0100_000C;
    const INTEL: u32 = 0x0100_0007;

    #[test]
    fn reads_the_cpu_from_the_header() {
        let mac = |cpus: Vec<Cpu>| {
            Some(Binary {
                os: Os::Macos,
                cpus,
            })
        };
        assert_eq!(read_binary(&macho(ARM)), mac(vec![Cpu::Arm64]));
        assert_eq!(read_binary(&macho(INTEL)), mac(vec![Cpu::X86_64]));
        assert_eq!(
            read_binary(&fat(&[INTEL, ARM])),
            mac(vec![Cpu::X86_64, Cpu::Arm64])
        );
        let win = |cpus: Vec<Cpu>| {
            Some(Binary {
                os: Os::Windows,
                cpus,
            })
        };
        assert_eq!(read_binary(&pe(0x8664)), win(vec![Cpu::X86_64]));
        assert_eq!(read_binary(&pe(0x014C)), win(vec![Cpu::X86]));
        assert_eq!(read_binary(&pe(0x01C4)), win(vec![]));
        assert_eq!(read_binary(b""), None);
        assert_eq!(read_binary(b"<!DOCTYPE html>"), None);
        assert_eq!(read_binary(&[0u8; 4096]), None);
        assert_eq!(read_binary(&pe(0x8664)[..0x42]), None, "cut short");
    }

    #[test]
    fn rosetta_shows_in_which_plugin_list_reaper_wrote_last() {
        let mac = |a, i| reaper_cpu(Os::Macos, Arch::Aarch64, a, i);
        assert_eq!(mac(Some(100), Some(50)), Cpu::Arm64);
        assert_eq!(mac(Some(100), Some(150)), Cpu::X86_64);
        assert_eq!(mac(None, Some(150)), Cpu::X86_64);
        assert_eq!(mac(None, None), Cpu::Arm64);
        assert_eq!(reaper_cpu(Os::Macos, Arch::X86_64, None, None), Cpu::X86_64);
        assert_eq!(
            reaper_cpu(Os::Windows, Arch::X86_64, None, None),
            Cpu::X86_64
        );
    }

    #[test]
    fn finds_the_extensions_scripts_call() {
        let ids = |rel: &str, text: &str| -> Vec<String> {
            script_needs(rel, text.as_bytes())
                .into_iter()
                .map(|p| p.0)
                .collect()
        };
        let lua = r#"
            if not reaper.APIExists("CF_GetSWSVersion") then return end
            local hwnd = reaper.JS_Window_Find("Mixer", true)
            local ctx = reaper.ImGui_CreateContext('Tool')
            local BR_note = 1 -- a local variable, not SWS
            local x = reaper.GetCursorPosition()
        "#;
        assert_eq!(
            ids("Scripts/a.lua", lua),
            [
                "reaper-ext/imgui",
                "reaper-ext/js-reascriptapi",
                "reaper-ext/sws"
            ]
        );
        assert_eq!(
            ids(
                "Scripts/b.lua",
                "local c = reaper.BR_GetMouseCursorContext()"
            ),
            ["reaper-ext/sws"]
        );
        assert_eq!(
            ids("Scripts/c.eel", "item = BR_GetMediaItemByGUID(0, guid);"),
            ["reaper-ext/sws"]
        );
        assert_eq!(
            ids("Scripts/d.py", "ReaPack_BrowsePackages('x', '', False, 0)"),
            ["reaper-ext/reapack"]
        );
        assert!(ids("Scripts/e.lua", "local SNM_x = BR_lower").is_empty());
        assert!(
            ids("Scripts/e.lua", "CF_Thing()").is_empty(),
            "Lua calls go through reaper."
        );
        assert!(ids("Scripts/readme.txt", "reaper.BR_GetMouseCursorContext()").is_empty());
    }

    fn file(rel: &str, binary: Option<Binary>, quarantined: bool, mtime: i64) -> FoundFile {
        FoundFile {
            rel: rel.into(),
            binary,
            quarantined,
            mtime,
        }
    }

    fn mac(cpus: &[Cpu]) -> Option<Binary> {
        Some(Binary {
            os: Os::Macos,
            cpus: cpus.to_vec(),
        })
    }

    fn one(files: &[FoundFile], cpu: Cpu) -> ExtensionState {
        let mut all = check(files, Os::Macos, cpu, &BTreeMap::new());
        assert_eq!(all.len(), 1, "{all:?}");
        all.remove(0)
    }

    #[test]
    fn a_downloaded_extension_is_blocked_until_unblocked() {
        let s = one(
            &[file("reaper_sws-arm64.dylib", mac(&[Cpu::Arm64]), true, 1)],
            Cpu::Arm64,
        );
        assert_eq!(s.status, Status::Blocked);
        assert_eq!(
            s.fix,
            Some(Fix {
                action: FixAction::Unblock,
                file: "reaper_sws-arm64.dylib".into()
            })
        );
        let s = one(
            &[file("reaper_sws-arm64.dylib", mac(&[Cpu::Arm64]), false, 1)],
            Cpu::Arm64,
        );
        assert_eq!(
            (s.status, s.file.as_deref()),
            (Status::Ok, Some("reaper_sws-arm64.dylib"))
        );
    }

    #[test]
    fn a_subfolder_copy_moves_up() {
        let s = one(
            &[file(
                "sws-2.14/reaper_sws-arm64.dylib",
                mac(&[Cpu::Arm64]),
                true,
                1,
            )],
            Cpu::Arm64,
        );
        assert_eq!(s.status, Status::InSubfolder);
        assert!(s.detail.contains("sws-2.14"), "{}", s.detail);
        assert_eq!(s.fix.unwrap().action, FixAction::MoveUp);
    }

    #[test]
    fn the_wrong_build_names_the_right_one() {
        let s = one(
            &[file(
                "reaper_sws-x86_64.dylib",
                mac(&[Cpu::X86_64]),
                false,
                1,
            )],
            Cpu::Arm64,
        );
        assert_eq!(s.status, Status::WrongCpu);
        assert!(s.detail.contains("Intel build"), "{}", s.detail);
        assert!(s.detail.contains("reaper_sws-arm64.dylib"), "{}", s.detail);
        assert_eq!(
            s.download.unwrap().file.as_deref(),
            Some("reaper_sws-arm64.dylib")
        );
        // Under Rosetta the Intel build is the right one, and a universal one always is.
        assert_eq!(
            one(
                &[file(
                    "reaper_sws-x86_64.dylib",
                    mac(&[Cpu::X86_64]),
                    false,
                    1
                )],
                Cpu::X86_64
            )
            .status,
            Status::Ok
        );
        assert_eq!(
            one(
                &[file(
                    "reaper_sws.dylib",
                    mac(&[Cpu::X86_64, Cpu::Arm64]),
                    false,
                    1
                )],
                Cpu::Arm64
            )
            .status,
            Status::Ok
        );
        // An extra Intel copy beside a working one is fine, for Rosetta.
        let both = [
            file("reaper_sws-x86_64.dylib", mac(&[Cpu::X86_64]), false, 1),
            file("reaper_sws-arm64.dylib", mac(&[Cpu::Arm64]), false, 1),
        ];
        assert_eq!(one(&both, Cpu::Arm64).status, Status::Ok);

        let win = Some(Binary {
            os: Os::Windows,
            cpus: vec![Cpu::X86_64],
        });
        let s = one(&[file("reaper_sws-x64.dll", win, false, 1)], Cpu::Arm64);
        assert_eq!(s.status, Status::WrongOs);
        assert!(s.detail.contains("Windows build"), "{}", s.detail);
    }

    #[test]
    fn a_broken_file_and_two_copies_are_named() {
        let s = one(
            &[file("reaper_sws-arm64.dylib", None, false, 1)],
            Cpu::Arm64,
        );
        assert_eq!(s.status, Status::Damaged);
        let s = one(
            &[
                file("reaper_sws64.dylib", mac(&[Cpu::Arm64]), false, 1),
                file("reaper_sws-arm64.dylib", mac(&[Cpu::Arm64]), false, 9),
            ],
            Cpu::Arm64,
        );
        assert_eq!(s.status, Status::TwoCopies);
        assert_eq!(
            s.fix,
            Some(Fix {
                action: FixAction::Remove,
                file: "reaper_sws64.dylib".into()
            })
        );
    }

    #[test]
    fn scripts_ask_for_missing_extensions() {
        let mut needed = BTreeMap::new();
        needed.insert(
            product_id("sws"),
            BTreeSet::from(["Scripts/nvk/a.lua".to_owned()]),
        );
        needed.insert(
            product_id("js_reascriptapi"),
            BTreeSet::from(["Scripts/b.lua".to_owned(), "Scripts/c.lua".to_owned()]),
        );
        let reapack = [file(
            "reaper_reapack-arm64.dylib",
            mac(&[Cpu::Arm64]),
            false,
            1,
        )];
        let all = check(&reapack, Os::Macos, Cpu::Arm64, &needed);
        let names: Vec<_> = all.iter().map(|s| (s.name.as_str(), s.status)).collect();
        assert_eq!(
            names,
            [
                ("SWS Extension", Status::Missing),
                ("js_ReaScriptAPI", Status::Missing),
                ("ReaPack", Status::Ok),
            ]
        );
        assert!(
            all[0].detail.contains("reaper_sws-arm64.dylib"),
            "{}",
            all[0].detail
        );
        assert!(
            all[1].detail.contains("Browse packages"),
            "{}",
            all[1].detail
        );
        assert_eq!(all[1].needed_by, ["Scripts/b.lua", "Scripts/c.lua"]);
        // Without ReaPack, js_ReaScriptAPI is a download too.
        let all = check(&[], Os::Windows, Cpu::X86_64, &needed);
        assert!(
            all[1].detail.contains("reaper_js_ReaScriptAPI64.dll"),
            "{}",
            all[1].detail
        );
    }

    #[test]
    fn items_are_installed_software_with_their_cpu() {
        let n = parse_file_name("reaper_sws-arm64.dylib").unwrap();
        let (p, i) = extension_item(
            &n,
            "reaper_sws-arm64.dylib",
            &Binary {
                os: Os::Macos,
                cpus: vec![Cpu::Arm64],
            },
            None,
            Some(10),
            Some(5),
        );
        assert_eq!(p.canonical_id.0, "reaper-ext/sws");
        assert_eq!(p.name, "SWS Extension");
        assert_eq!(i.kind, ItemKind::VendorApp);
        assert_eq!(i.arch, Some(Arch::Aarch64));
        assert_eq!(
            i.path,
            PortablePath::logical(REAPER_RESOURCES, "UserPlugins/reaper_sws-arm64.dylib")
        );
        assert!(is_extension(&p.canonical_id));
        assert!(known(&p.canonical_id).is_some());
    }
}
