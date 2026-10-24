//! ReaPack: which repositories and packages a machine has, and what another
//! machine needs to match it.
//!
//! ReaPack keeps its repositories in `reapack.ini` in Reaper's resource folder:
//!
//! ```text
//! [remotes]
//! remote0=ReaTeam Scripts|https://github.com/ReaTeam/ReaScripts/raw/master/index.xml|1|2
//! size=1
//! ```
//!
//! (`name|index URL|enabled|auto-install`), and its installed packages in the
//! SQLite file `ReaPack/registry.db` (read in `patchpilot-local`). Packages
//! are free scripts, effects and themes that ReaPack itself downloads; Patch
//! Pilot never copies them. Restoring adds the missing repositories and lets
//! ReaPack's own "Synchronize packages" install the rest (decision #40).

use patchpilot_core::{Confidence, Item, ItemKind, Manifest, Os, PortablePath, Product, ProductId};
use serde::Serialize;

use crate::{ini, reaper::slug};

/// Logical root of ReaPack package items: `REAPACK/<repository>/<category>/<package>`.
pub const REAPACK: &str = "REAPACK";

const REPO_PREFIX: &str = "reapack-repo/";
const PACKAGE_PREFIX: &str = "reapack/";
const VENDOR: &str = "ReaPack";

/// ReaPack's package type for native extensions, the only per-OS kind.
const EXTENSION_TYPE: i64 = 2;

/// One repository line of `reapack.ini`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Remote {
    pub name: String,
    pub url: String,
    pub enabled: bool,
}

/// One installed package, as ReaPack's registry lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Package {
    /// Repository name, as in `reapack.ini`.
    pub remote: String,
    pub category: String,
    /// The package's file name in its repository, e.g. `Script: Foo.lua`.
    pub package: String,
    /// Display name; empty when the repository gives none.
    pub desc: String,
    pub version: String,
    /// ReaPack's type number (1 script, 2 extension, 3 effect, ...).
    pub kind: i64,
}

impl Package {
    pub fn label(&self) -> &str {
        if self.desc.is_empty() {
            &self.package
        } else {
            &self.desc
        }
    }
}

/// The repositories in `reapack.ini`, in file order.
pub fn parse_remotes(text: &str) -> Vec<Remote> {
    ini::section(text, "remotes")
        .into_iter()
        .filter(|(k, _)| is_remote_key(k))
        .filter_map(|(_, v)| {
            let mut parts = v.split('|');
            let name = parts.next()?.trim();
            let url = parts.next()?.trim();
            let enabled = parts.next().is_none_or(|e| e.trim() != "0");
            (!name.is_empty() && !url.is_empty()).then(|| Remote {
                name: name.to_owned(),
                url: url.to_owned(),
                enabled,
            })
        })
        .collect()
}

/// `reapack.ini` with `remotes` appended as enabled repositories that follow
/// ReaPack's global auto-install setting.
pub fn add_remotes(text: &str, remotes: &[Remote]) -> String {
    let lines = ini::section(text, "remotes");
    let size = ini::get(text, "remotes", "size")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0)
        .max(lines.iter().filter(|(k, _)| is_remote_key(k)).count());
    let mut values: Vec<(String, String)> = remotes
        .iter()
        .enumerate()
        .map(|(n, r)| {
            (
                format!("remote{}", size + n),
                format!("{}|{}|1|2", r.name, r.url),
            )
        })
        .collect();
    values.push(("size".to_owned(), (size + remotes.len()).to_string()));
    ini::set(text, "remotes", &values)
}

/// `remote0`, `remote1`, ...
fn is_remote_key(k: &str) -> bool {
    k.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("remote"))
        && k.len() > 6
        && k[6..].bytes().all(|b| b.is_ascii_digit())
}

/// Manifest products and items for enabled repositories and installed packages.
pub fn reapack_items(
    remotes: &[Remote],
    packages: &[Package],
    os: Os,
) -> (Vec<Product>, Vec<Item>) {
    let mut products = Vec::new();
    let mut items = Vec::new();
    for r in remotes.iter().filter(|r| r.enabled) {
        let id = ProductId(format!("{REPO_PREFIX}{}", slug(&r.name)));
        products.push(Product {
            canonical_id: id.clone(),
            vendor: VENDOR.to_owned(),
            name: format!("{} repository", r.name),
        });
        items.push(item(
            ItemKind::Config,
            id,
            os,
            PortablePath::Absolute(r.url.clone()),
            None,
            true,
        ));
    }
    for p in packages {
        let id = ProductId(format!(
            "{PACKAGE_PREFIX}{}/{}/{}",
            slug(&p.remote),
            slug(&p.category),
            slug(&p.package)
        ));
        products.push(Product {
            canonical_id: id.clone(),
            vendor: VENDOR.to_owned(),
            name: p.label().to_owned(),
        });
        let rel = format!("{}/{}/{}", p.remote, p.category, p.package);
        items.push(item(
            ItemKind::DawAsset,
            id,
            os,
            PortablePath::logical(REAPACK, rel),
            Some(p.version.clone()),
            p.kind != EXTENSION_TYPE,
        ));
    }
    (products, items)
}

fn item(
    kind: ItemKind,
    id: ProductId,
    os: Os,
    path: PortablePath,
    version: Option<String>,
    portable: bool,
) -> Item {
    Item {
        kind,
        product_id: Some(id),
        format: None,
        platform: os,
        arch: None,
        version,
        path,
        content_hash: None,
        size: None,
        mtime: None,
        platform_portable: portable,
        confidence: Confidence::High,
        requires: Vec::new(),
    }
}

/// True for products this module adds.
pub fn is_reapack(id: &ProductId) -> bool {
    id.0.starts_with(REPO_PREFIX) || id.0.starts_with(PACKAGE_PREFIX)
}

/// ReaPack repositories and packages in a manifest, read back from its items.
pub fn from_manifest(m: &Manifest) -> (Vec<Remote>, Vec<Package>) {
    let mut remotes = Vec::new();
    let mut packages = Vec::new();
    for i in &m.items {
        let Some(id) = &i.product_id else { continue };
        let name = m.product(id).map(|p| p.name.as_str()).unwrap_or_default();
        match &i.path {
            PortablePath::Absolute(url) if id.0.starts_with(REPO_PREFIX) => {
                remotes.push(Remote {
                    name: name.strip_suffix(" repository").unwrap_or(name).to_owned(),
                    url: url.clone(),
                    enabled: true,
                });
            }
            PortablePath::Logical { root, rel } if root == REAPACK => {
                let mut parts = rel.split('/');
                let (Some(remote), Some(package)) = (parts.next(), parts.next_back()) else {
                    continue;
                };
                let category = parts.collect::<Vec<_>>().join("/");
                packages.push(Package {
                    remote: remote.to_owned(),
                    category,
                    package: package.to_owned(),
                    desc: name.to_owned(),
                    version: i.version.clone().unwrap_or_default(),
                    kind: if i.platform_portable {
                        1
                    } else {
                        EXTENSION_TYPE
                    },
                });
            }
            _ => {}
        }
    }
    (remotes, packages)
}

/// What a machine needs to match the Golden Rig's ReaPack setup.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ReaPackRestore {
    /// Golden's repositories this machine doesn't have (matched by URL, then name).
    pub missing_remotes: Vec<Remote>,
    /// Golden's packages this machine hasn't installed. Extensions built for
    /// another OS are left out: ReaPack picks the right build itself.
    pub missing_packages: Vec<Package>,
}

impl ReaPackRestore {
    pub fn is_empty(&self) -> bool {
        self.missing_remotes.is_empty() && self.missing_packages.is_empty()
    }
}

pub fn restore_plan(golden: &Manifest, machine: &Manifest) -> ReaPackRestore {
    let (g_remotes, g_packages) = from_manifest(golden);
    let (m_remotes, m_packages) = from_manifest(machine);
    let missing_remotes = g_remotes
        .into_iter()
        .filter(|g| {
            !m_remotes
                .iter()
                .any(|m| same_url(&m.url, &g.url) || m.name.eq_ignore_ascii_case(&g.name))
        })
        .collect();
    let missing_packages = g_packages
        .into_iter()
        .filter(|g| {
            !m_packages.iter().any(|m| {
                m.remote.eq_ignore_ascii_case(&g.remote)
                    && m.category.eq_ignore_ascii_case(&g.category)
                    && m.package.eq_ignore_ascii_case(&g.package)
            })
        })
        .collect();
    ReaPackRestore {
        missing_remotes,
        missing_packages,
    }
}

fn same_url(a: &str, b: &str) -> bool {
    let norm = |u: &str| u.trim().trim_end_matches('/').to_ascii_lowercase();
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use patchpilot_core::{Arch, MachineInfo, PathMap};

    use super::*;

    const INI: &str = "[general]\nversion=4\n\n[remotes]\nremote0=ReaPack|https://reapack.com/index.xml|1|2\nremote1=ReaTeam Scripts|https://github.com/ReaTeam/ReaScripts/raw/master/index.xml|1|2\nremote2=Old|https://example.com/old.xml|0|2\nsize=3\n";

    fn manifest(os: Os, remotes: &[Remote], packages: &[Package]) -> Manifest {
        let mut m = Manifest::new(
            MachineInfo {
                id: "m".into(),
                name: "M".into(),
                os,
                arch: Arch::X86_64,
                os_version: None,
                path_map: PathMap::new(os),
            },
            0,
        );
        let (p, i) = reapack_items(remotes, packages, os);
        m.products = p;
        m.items = i;
        m
    }

    fn pkg(remote: &str, package: &str, kind: i64) -> Package {
        Package {
            remote: remote.into(),
            category: "Items Editing".into(),
            package: package.into(),
            desc: format!("{package} desc"),
            version: "1.2".into(),
            kind,
        }
    }

    #[test]
    fn parses_remotes() {
        let r = parse_remotes(INI);
        assert_eq!(r.len(), 3);
        assert_eq!(r[1].name, "ReaTeam Scripts");
        assert!(r[1].enabled);
        assert!(!r[2].enabled);
    }

    #[test]
    fn appends_remotes_after_the_last_index() {
        let add = [Remote {
            name: "X-Raym".into(),
            url: "https://x-raym.example/index.xml".into(),
            enabled: true,
        }];
        let out = add_remotes(INI, &add);
        assert!(out.contains("remote3=X-Raym|https://x-raym.example/index.xml|1|2\n"));
        assert!(out.contains("size=4\n"));
        assert!(!out.contains("size=3"));
        assert_eq!(parse_remotes(&out).len(), 4);
    }

    #[test]
    fn adds_a_remotes_section_when_there_is_none() {
        let add = [Remote {
            name: "A".into(),
            url: "https://a/index.xml".into(),
            enabled: true,
        }];
        let out = add_remotes("", &add);
        assert_eq!(parse_remotes(&out), add);
        assert_eq!(ini::get(&out, "remotes", "size"), Some("1"));
    }

    #[test]
    fn round_trips_through_a_manifest() {
        let remotes = parse_remotes(INI);
        let packages = [pkg("ReaTeam Scripts", "Script: Foo.lua", 1)];
        let m = manifest(Os::Windows, &remotes, &packages);
        let (r, p) = from_manifest(&m);
        assert_eq!(r.len(), 2, "disabled repositories are left out");
        assert_eq!(r[1].name, "ReaTeam Scripts");
        assert_eq!(p, packages);
    }

    #[test]
    fn plans_missing_remotes_and_packages() {
        let remotes = parse_remotes(INI);
        let golden = manifest(
            Os::Windows,
            &remotes,
            &[
                pkg("ReaTeam Scripts", "Script: Foo.lua", 1),
                pkg("ReaTeam Scripts", "Script: Bar.lua", 1),
            ],
        );
        let machine = manifest(
            Os::Macos,
            &remotes[..1],
            &[pkg("reateam scripts", "Script: Foo.lua", 1)],
        );
        let plan = restore_plan(&golden, &machine);
        assert_eq!(plan.missing_remotes.len(), 1);
        assert_eq!(plan.missing_remotes[0].name, "ReaTeam Scripts");
        assert_eq!(plan.missing_packages.len(), 1);
        assert_eq!(plan.missing_packages[0].package, "Script: Bar.lua");
        assert!(restore_plan(&golden, &golden).is_empty());
    }
}
