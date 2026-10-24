//! Restore points (schema v4): which user files a change touched, and the
//! blob of each file before it.

use rusqlite::{OptionalExtension, params};

use crate::{Result, Store, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreKind {
    /// Patch Pilot changed files (Bring over, path fixes, ...).
    Change,
    /// Files put back as they were before another restore point.
    Undo,
}

impl RestoreKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Change => "change",
            Self::Undo => "undo",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        match s {
            "change" => Ok(Self::Change),
            "undo" => Ok(Self::Undo),
            _ => Err(StoreError::BadValue {
                column: "restore_points.kind",
                value: s.to_owned(),
            }),
        }
    }
}

/// One file a restore point covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    pub path: String,
    /// Blob hash of the file before the change; `None` if it didn't exist.
    pub before_hash: Option<String>,
    /// Hash of what was written; `None` if the change removed the file.
    pub after_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRestorePoint<'a> {
    pub machine_id: &'a str,
    pub created_at: i64,
    pub kind: RestoreKind,
    pub label: &'a str,
    pub daw: Option<&'a str>,
    pub undoes: Option<i64>,
    pub files: &'a [FileRecord],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePointRecord {
    pub id: i64,
    pub machine_id: String,
    pub created_at: i64,
    pub kind: RestoreKind,
    pub label: String,
    pub daw: Option<String>,
    /// The machine's latest scan when the change was made.
    pub snapshot_id: Option<i64>,
    /// For an undo, the restore point it undid.
    pub undoes: Option<i64>,
    /// The latest undo of this restore point, if any.
    pub undone_by: Option<i64>,
    pub files: Vec<FileRecord>,
}

const SELECT: &str = "SELECT r.id, r.machine_id, r.created_at, r.kind, r.label, r.daw,
        r.snapshot_id, r.undoes,
        (SELECT max(u.id) FROM restore_points u WHERE u.undoes = r.id)
     FROM restore_points r";

impl Store {
    /// Record a restore point; returns its ID. The blobs must already be
    /// stored.
    pub fn add_restore_point(&mut self, p: &NewRestorePoint) -> Result<i64> {
        let tx = self.conn.transaction()?;
        let snapshot: Option<i64> = tx
            .query_row(
                "SELECT id FROM snapshots WHERE machine_id = ?1 ORDER BY id DESC LIMIT 1",
                [p.machine_id],
                |r| r.get(0),
            )
            .optional()?;
        tx.execute(
            "INSERT INTO restore_points
                 (machine_id, created_at, kind, label, daw, snapshot_id, undoes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                p.machine_id,
                p.created_at,
                p.kind.as_str(),
                p.label,
                p.daw,
                snapshot,
                p.undoes
            ],
        )?;
        let id = tx.last_insert_rowid();
        for (n, f) in p.files.iter().enumerate() {
            tx.execute(
                "INSERT INTO restore_point_files
                     (restore_point_id, position, path, before_hash, after_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, n as i64, f.path, f.before_hash, f.after_hash],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Drop a restore point whose change never happened (its write failed
    /// and was put back). Blobs stay; they're shared and harmless.
    pub fn discard_restore_point(&mut self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM restore_points WHERE id = ?1", [id])?;
        Ok(())
    }

    /// A machine's restore points, newest first.
    pub fn restore_points(&self, machine_id: &str) -> Result<Vec<RestorePointRecord>> {
        let mut stmt = self.conn.prepare(&format!(
            "{SELECT} WHERE r.machine_id = ?1 ORDER BY r.id DESC"
        ))?;
        let rows = stmt.query_map([machine_id], raw)?;
        let rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(|r| self.finish(r)).collect()
    }

    pub fn restore_point(&self, id: i64) -> Result<Option<RestorePointRecord>> {
        let row = self
            .conn
            .query_row(&format!("{SELECT} WHERE r.id = ?1"), [id], raw)
            .optional()?;
        row.map(|r| self.finish(r)).transpose()
    }

    fn finish(&self, r: Raw) -> Result<RestorePointRecord> {
        let mut stmt = self.conn.prepare(
            "SELECT path, before_hash, after_hash FROM restore_point_files
             WHERE restore_point_id = ?1 ORDER BY position",
        )?;
        let files = stmt
            .query_map([r.id], |f| {
                Ok(FileRecord {
                    path: f.get(0)?,
                    before_hash: f.get(1)?,
                    after_hash: f.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(RestorePointRecord {
            id: r.id,
            machine_id: r.machine_id,
            created_at: r.created_at,
            kind: RestoreKind::parse(&r.kind)?,
            label: r.label,
            daw: r.daw,
            snapshot_id: r.snapshot_id,
            undoes: r.undoes,
            undone_by: r.undone_by,
            files,
        })
    }
}

struct Raw {
    id: i64,
    machine_id: String,
    created_at: i64,
    kind: String,
    label: String,
    daw: Option<String>,
    snapshot_id: Option<i64>,
    undoes: Option<i64>,
    undone_by: Option<i64>,
}

fn raw(r: &rusqlite::Row) -> rusqlite::Result<Raw> {
    Ok(Raw {
        id: r.get(0)?,
        machine_id: r.get(1)?,
        created_at: r.get(2)?,
        kind: r.get(3)?,
        label: r.get(4)?,
        daw: r.get(5)?,
        snapshot_id: r.get(6)?,
        undoes: r.get(7)?,
        undone_by: r.get(8)?,
    })
}
