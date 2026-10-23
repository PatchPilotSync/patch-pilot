//! Local SQLite index for one machine's Patch Pilot install.
//!
//! The database lives in the OS app-data directory ([`default_db_path`]),
//! never in user folders (CLAUDE.md rule 3). Schema changes are embedded SQL
//! migrations tracked with `PRAGMA user_version`.

mod migrations;
mod paths;

use std::path::Path;

use patchpilot_core::{
    Confidence, ContentHash, Item, MachineInfo, Manifest, PathMap, PortablePath, Product,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use migrations::{LATEST_VERSION, MIGRATIONS};
pub use paths::default_db_path;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(
        "database schema version {found} is newer than this build supports ({supported}); update Patch Pilot"
    )]
    TooNew { found: u32, supported: u32 },
    #[error("corrupt value in column {column}: {value:?}")]
    BadValue { column: &'static str, value: String },
    #[error("{path}: {source}")]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error("unknown machine {0:?}")]
    UnknownMachine(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Why a snapshot was taken (`docs/strategy.md` section 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotReason {
    FirstScan,
    Scan,
    BeforeApply,
    DriftAccepted,
    Manual,
}

impl SnapshotReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::FirstScan => "first_scan",
            Self::Scan => "scan",
            Self::BeforeApply => "before_apply",
            Self::DriftAccepted => "drift_accepted",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotInfo {
    pub id: i64,
    pub parent_id: Option<i64>,
    /// BLAKE3 of the manifest's canonical JSON, `blake3:<hex>`.
    pub manifest_hash: String,
    /// True when an identical snapshot already existed and was reused.
    pub unchanged: bool,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if needed) and migrate the database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|source| StoreError::Io {
                path: dir.to_owned(),
                source,
            })?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrations::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Record a scanned manifest as a new snapshot of its machine.
    ///
    /// Upserts the machine, its path map and the products; inserts only items
    /// not already stored (items are immutable rows keyed by content hash).
    /// If the manifest is identical to the machine's latest snapshot, that
    /// snapshot is returned instead of creating a duplicate.
    pub fn save_manifest(
        &mut self,
        manifest: &Manifest,
        reason: SnapshotReason,
        now: i64,
    ) -> Result<SnapshotInfo> {
        let manifest_hash = hash_json(manifest);
        let tx = self.conn.transaction()?;
        let m = &manifest.machine;
        upsert_machine(&tx, m, now)?;

        let latest: Option<(i64, String)> = tx
            .query_row(
                "SELECT id, manifest_hash FROM snapshots WHERE machine_id = ?1 ORDER BY id DESC LIMIT 1",
                [&m.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, hash)) = &latest {
            if *hash == manifest_hash {
                tx.commit()?;
                return Ok(SnapshotInfo {
                    id: *id,
                    parent_id: None,
                    manifest_hash,
                    unchanged: true,
                });
            }
        }

        for p in &manifest.products {
            tx.execute(
                "INSERT INTO products (canonical_id, vendor, name) VALUES (?1, ?2, ?3)
                 ON CONFLICT (canonical_id) DO UPDATE SET vendor = excluded.vendor, name = excluded.name",
                params![p.canonical_id.0, p.vendor, p.name],
            )?;
        }
        let parent_id = latest.map(|(id, _)| id);
        tx.execute(
            "INSERT INTO snapshots (machine_id, created_at, reason, parent_id, manifest_hash, generated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![m.id, now, reason.as_str(), parent_id, manifest_hash, manifest.generated_at],
        )?;
        let snapshot_id = tx.last_insert_rowid();
        for (pos, item) in manifest.items.iter().enumerate() {
            let (item_id, hash) = insert_item(&tx, &m.id, item)?;
            tx.execute(
                "INSERT INTO snapshot_items (snapshot_id, position, item_id, hash) VALUES (?1, ?2, ?3, ?4)",
                params![snapshot_id, pos as i64, item_id, hash],
            )?;
        }
        tx.commit()?;
        Ok(SnapshotInfo {
            id: snapshot_id,
            parent_id,
            manifest_hash,
            unchanged: false,
        })
    }

    /// The manifest of a machine's latest snapshot.
    pub fn latest_manifest(&self, machine_id: &str) -> Result<Option<Manifest>> {
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM snapshots WHERE machine_id = ?1 ORDER BY id DESC LIMIT 1",
                [machine_id],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| self.snapshot_manifest(id)).transpose()
    }

    /// Rebuild the manifest stored in a snapshot.
    pub fn snapshot_manifest(&self, snapshot_id: i64) -> Result<Manifest> {
        let (machine_id, generated_at): (String, i64) = self.conn.query_row(
            "SELECT machine_id, generated_at FROM snapshots WHERE id = ?1",
            [snapshot_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let machine = self.machine(&machine_id)?;

        let mut stmt = self.conn.prepare(
            "SELECT i.kind, i.product_id, i.format, i.platform, i.arch, i.version, i.path_root,
                    i.path_rel, i.path_absolute, i.content_hash, i.size, i.mtime,
                    i.platform_portable, i.confidence
             FROM snapshot_items s JOIN items i ON i.id = s.item_id
             WHERE s.snapshot_id = ?1 ORDER BY s.position",
        )?;
        let rows = stmt.query_map([snapshot_id], |r| {
            Ok(RawItem {
                kind: r.get(0)?,
                product_id: r.get(1)?,
                format: r.get(2)?,
                platform: r.get(3)?,
                arch: r.get(4)?,
                version: r.get(5)?,
                path_root: r.get(6)?,
                path_rel: r.get(7)?,
                path_absolute: r.get(8)?,
                content_hash: r.get(9)?,
                size: r.get(10)?,
                mtime: r.get(11)?,
                platform_portable: r.get(12)?,
                confidence: r.get(13)?,
            })
        })?;
        let items = rows.map(|r| r?.into_item()).collect::<Result<Vec<_>>>()?;

        // Products referenced by this snapshot, in first-use order.
        let mut products: Vec<Product> = Vec::new();
        for id in items.iter().filter_map(|i| i.product_id.as_ref()) {
            if products.iter().any(|p| &p.canonical_id == id) {
                continue;
            }
            let p = self
                .conn
                .query_row(
                    "SELECT vendor, name FROM products WHERE canonical_id = ?1",
                    [&id.0],
                    |r| {
                        Ok(Product {
                            canonical_id: id.clone(),
                            vendor: r.get(0)?,
                            name: r.get(1)?,
                        })
                    },
                )
                .optional()?;
            products.extend(p);
        }

        let mut manifest = Manifest::new(machine, generated_at);
        manifest.products = products;
        manifest.items = items;
        Ok(manifest)
    }

    pub fn machine(&self, id: &str) -> Result<MachineInfo> {
        let row: Option<(String, String, String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT name, os, arch, os_version FROM machines WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((name, os, arch, os_version)) = row else {
            return Err(StoreError::UnknownMachine(id.to_owned()));
        };
        let os = from_text("machines.os", &os)?;
        let mut path_map = PathMap::new(os);
        let mut stmt = self
            .conn
            .prepare("SELECT logical_name, physical_path FROM path_maps WHERE machine_id = ?1")?;
        for row in stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))? {
            let (k, v): (String, String) = row?;
            path_map.roots.insert(k, v);
        }
        Ok(MachineInfo {
            id: id.to_owned(),
            name,
            os,
            arch: from_text("machines.arch", &arch)?,
            os_version,
            path_map,
        })
    }
}

fn upsert_machine(tx: &Transaction, m: &MachineInfo, now: i64) -> Result<()> {
    tx.execute(
        "INSERT INTO machines (id, name, os, arch, os_version, last_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (id) DO UPDATE SET name = excluded.name, os = excluded.os, arch = excluded.arch,
             os_version = excluded.os_version, last_seen = excluded.last_seen",
        params![m.id, m.name, to_text(&m.os), to_text(&m.arch), m.os_version, now],
    )?;
    tx.execute("DELETE FROM path_maps WHERE machine_id = ?1", [&m.id])?;
    for (k, v) in &m.path_map.roots {
        tx.execute(
            "INSERT INTO path_maps (machine_id, logical_name, physical_path) VALUES (?1, ?2, ?3)",
            params![m.id, k, v],
        )?;
    }
    Ok(())
}

/// Insert an item if this machine doesn't have an identical one; return its id and hash.
fn insert_item(tx: &Transaction, machine_id: &str, item: &Item) -> Result<(i64, String)> {
    let hash = hash_json(item);
    let (root, rel, abs) = match &item.path {
        PortablePath::Logical { root, rel } => (Some(root.as_str()), Some(rel.as_str()), None),
        PortablePath::Absolute(a) => (None, None, Some(a.as_str())),
    };
    tx.execute(
        "INSERT INTO items (machine_id, item_hash, kind, product_id, format, platform, arch, version,
             path_root, path_rel, path_absolute, content_hash, size, mtime, platform_portable, confidence)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT (machine_id, item_hash) DO NOTHING",
        params![
            machine_id,
            hash,
            to_text(&item.kind),
            item.product_id.as_ref().map(|p| &p.0),
            item.format.as_ref().map(to_text),
            to_text(&item.platform),
            item.arch.as_ref().map(to_text),
            item.version,
            root,
            rel,
            abs,
            item.content_hash.as_ref().map(ContentHash::as_str),
            item.size.map(|s| s as i64),
            item.mtime,
            item.platform_portable,
            to_text(&item.confidence),
        ],
    )?;
    let id = tx.query_row(
        "SELECT id FROM items WHERE machine_id = ?1 AND item_hash = ?2",
        params![machine_id, hash],
        |r| r.get(0),
    )?;
    Ok((id, hash))
}

struct RawItem {
    kind: String,
    product_id: Option<String>,
    format: Option<String>,
    platform: String,
    arch: Option<String>,
    version: Option<String>,
    path_root: Option<String>,
    path_rel: Option<String>,
    path_absolute: Option<String>,
    content_hash: Option<String>,
    size: Option<i64>,
    mtime: Option<i64>,
    platform_portable: bool,
    confidence: String,
}

impl RawItem {
    fn into_item(self) -> Result<Item> {
        let path = match (self.path_root, self.path_rel, self.path_absolute) {
            (Some(root), Some(rel), None) => PortablePath::Logical { root, rel },
            (None, None, Some(a)) => PortablePath::Absolute(a),
            _ => {
                return Err(StoreError::BadValue {
                    column: "items.path_*",
                    value: String::new(),
                });
            }
        };
        let content_hash = self
            .content_hash
            .map(|h| {
                ContentHash::try_from(h.clone()).map_err(|_| StoreError::BadValue {
                    column: "items.content_hash",
                    value: h,
                })
            })
            .transpose()?;
        Ok(Item {
            kind: from_text("items.kind", &self.kind)?,
            product_id: self.product_id.map(patchpilot_core::ProductId),
            format: self
                .format
                .map(|f| from_text("items.format", &f))
                .transpose()?,
            platform: from_text("items.platform", &self.platform)?,
            arch: self.arch.map(|a| from_text("items.arch", &a)).transpose()?,
            version: self.version,
            path,
            content_hash,
            size: self.size.map(|s| s as u64),
            mtime: self.mtime,
            platform_portable: self.platform_portable,
            confidence: from_text::<Confidence>("items.confidence", &self.confidence)?,
        })
    }
}

/// An enum's serde name, e.g. `ItemKind::DawAsset` → `daw_asset`, so the DB
/// uses the same spelling as the manifest JSON.
fn to_text<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        other => panic!("not a unit enum: {other:?}"),
    }
}

fn from_text<T: DeserializeOwned>(column: &'static str, s: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).map_err(|_| {
        StoreError::BadValue {
            column,
            value: s.to_owned(),
        }
    })
}

fn hash_json<T: Serialize>(v: &T) -> String {
    let json = serde_json::to_vec(v).expect("serializable");
    format!("blake3:{}", blake3::hash(&json).to_hex())
}
