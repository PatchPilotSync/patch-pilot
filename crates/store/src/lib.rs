//! Local SQLite index for one machine's Patch Pilot install.
//!
//! The database lives in the OS app-data directory ([`default_db_path`]),
//! never in user folders (CLAUDE.md rule 3). Schema changes are embedded SQL
//! migrations tracked with `PRAGMA user_version`.

mod blobs;
mod machine_id;
mod migrations;
mod paths;
mod restore_points;

use std::path::Path;

use patchpilot_core::{
    Bundle, BundleGolden, BundleMachine, Confidence, ContentHash, GoldenVersion, GoldenVersionInfo,
    IgnoreEntry, IgnoredSuggestion, Item, MachineInfo, Manifest, PathMap, PortablePath, Product,
    ProfileRule, RuleAction, TargetProfile, golden_machine_id, golden_name,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use blobs::BlobStore;
pub use machine_id::{load_or_create_machine_id, read_machine_id};
pub use migrations::{LATEST_VERSION, MIGRATIONS};
pub use paths::default_db_path;
pub use restore_points::{FileRecord, NewRestorePoint, RestoreKind, RestorePointRecord};

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
    #[error("nothing to save: the Primary Rig already matches v{0}")]
    GoldenUnchanged(u32),
    #[error("unknown target profile {0:?}")]
    UnknownProfile(String),
    #[error("{0:?} is a built-in profile and can't be changed; import it under another id")]
    BuiltinProfile(String),
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
    /// A Primary Rig version (#35).
    Golden,
    /// Brought in from an exchange bundle (#38).
    Imported,
}

impl SnapshotReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstScan => "first_scan",
            Self::Scan => "scan",
            Self::BeforeApply => "before_apply",
            Self::DriftAccepted => "drift_accepted",
            Self::Manual => "manual",
            Self::Golden => "golden",
            Self::Imported => "imported",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "first_scan" => Self::FirstScan,
            "scan" => Self::Scan,
            "before_apply" => Self::BeforeApply,
            "drift_accepted" => Self::DriftAccepted,
            "manual" => Self::Manual,
            "golden" => Self::Golden,
            "imported" => Self::Imported,
            _ => {
                return Err(StoreError::BadValue {
                    column: "snapshots.reason",
                    value: s.to_owned(),
                });
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotInfo {
    pub id: i64,
    pub parent_id: Option<i64>,
    /// BLAKE3 of the manifest's JSON without `generated_at`, `blake3:<hex>`,
    /// so rescanning an unchanged machine gives the same hash.
    pub manifest_hash: String,
    /// True when an identical snapshot already existed and was reused.
    pub unchanged: bool,
}

/// A target profile as stored: rules, whether it's a built-in preset, and
/// the machines it is assigned to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProfile {
    pub profile: TargetProfile,
    pub builtin: bool,
    pub machines: Vec<String>,
}

/// What [`Store::import_bundle`] did. Import only ever adds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Machine names with a new snapshot.
    pub machines_added: Vec<String>,
    /// Machine names whose latest snapshot already matched.
    pub machines_unchanged: Vec<String>,
    /// True if the bundle held this machine's own scan, which is skipped.
    pub skipped_own_machine: bool,
    pub golden: GoldenImport,
    pub profiles_added: Vec<String>,
    /// Profiles whose ID exists here with different rules; left as they are.
    pub profiles_conflicting: Vec<String>,
    /// Primary Rig suggestions ignored (or offered again) on the other
    /// machine and now here too.
    pub ignored_added: usize,
    /// Ignores made or taken back on the other machine and now here too.
    pub ignores_changed: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum GoldenImport {
    /// The bundle has no Primary Rig.
    #[default]
    NotInBundle,
    /// This machine already has the bundle's latest Primary Rig version (or newer).
    UpToDate,
    /// New local versions, oldest first.
    Imported(Vec<u32>),
    /// Both sides changed the Primary Rig since they last matched. Nothing was
    /// imported; the user picks with `primary promote` / `primary propose`.
    Diverged,
}

/// One row of a machine's snapshot history, see [`Store::snapshots`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotSummary {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub created_at: i64,
    pub reason: SnapshotReason,
    pub items: u64,
    /// Items in this snapshot but not its parent (a changed item counts as
    /// one added and one removed).
    pub added: u64,
    pub removed: u64,
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
    /// If the manifest matches the machine's latest snapshot (ignoring
    /// `generated_at`), that snapshot is returned instead of creating a
    /// duplicate.
    pub fn save_manifest(
        &mut self,
        manifest: &Manifest,
        reason: SnapshotReason,
        now: i64,
    ) -> Result<SnapshotInfo> {
        let tx = self.conn.transaction()?;
        let info = save_manifest_tx(&tx, manifest, reason, now)?;
        tx.commit()?;
        Ok(info)
    }

    /// A machine's snapshots, newest first.
    pub fn snapshots(&self, machine_id: &str) -> Result<Vec<SnapshotSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.parent_id, s.created_at, s.reason,
                    (SELECT count(*) FROM snapshot_items WHERE snapshot_id = s.id),
                    (SELECT count(*) FROM snapshot_items c WHERE c.snapshot_id = s.id
                        AND c.item_id NOT IN
                            (SELECT item_id FROM snapshot_items WHERE snapshot_id = s.parent_id)),
                    (SELECT count(*) FROM snapshot_items p WHERE p.snapshot_id = s.parent_id
                        AND p.item_id NOT IN
                            (SELECT item_id FROM snapshot_items WHERE snapshot_id = s.id))
             FROM snapshots s WHERE s.machine_id = ?1 ORDER BY s.id DESC",
        )?;
        let rows = stmt.query_map([machine_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (id, parent_id, created_at, reason, items, added, removed) = row?;
            Ok(SnapshotSummary {
                id,
                parent_id,
                created_at,
                reason: SnapshotReason::parse(&reason)?,
                items: items as u64,
                added: added as u64,
                removed: removed as u64,
            })
        })
        .collect()
    }

    /// Save `manifest` as the next Primary Rig version (v1 if none yet).
    ///
    /// The stored manifest's `machine` is renamed to the version
    /// (`golden@<n>`, "Primary Rig v<n>"); everything else is kept. Fails with
    /// [`StoreError::GoldenUnchanged`] if its products and items equal the
    /// latest version's, so history only grows on real changes.
    pub fn save_golden(
        &mut self,
        manifest: &Manifest,
        source_machine_id: Option<&str>,
        note: Option<&str>,
        now: i64,
    ) -> Result<GoldenVersionInfo> {
        let parent = self.golden(None)?;
        if let Some(p) = &parent
            && same_content(&p.manifest, manifest)
        {
            return Err(StoreError::GoldenUnchanged(p.info.version));
        }
        let parent = parent.map(|p| p.info.version);
        let version = parent.map_or(1, |v| v + 1);
        let mut m = manifest.clone();
        m.machine.id = golden_machine_id(version);
        m.machine.name = golden_name(version);

        let tx = self.conn.transaction()?;
        let snapshot = save_manifest_tx(&tx, &m, SnapshotReason::Golden, now)?;
        tx.execute(
            "INSERT INTO golden_versions
                 (golden_id, version, parent_version, snapshot_id, created_at, source_machine_id, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![GOLDEN_ID, version, parent, snapshot.id, now, source_machine_id, note],
        )?;
        tx.commit()?;
        Ok(GoldenVersionInfo {
            version,
            parent,
            created_at: now,
            source_machine_id: source_machine_id.map(str::to_owned),
            note: note.map(str::to_owned),
        })
    }

    /// A Primary Rig version, or the latest when `version` is `None`.
    /// `Ok(None)` if there is no such version.
    pub fn golden(&self, version: Option<u32>) -> Result<Option<GoldenVersion>> {
        let row = self
            .conn
            .query_row(
                "SELECT version, parent_version, created_at, source_machine_id, note, snapshot_id
                 FROM golden_versions WHERE golden_id = ?1 AND (?2 IS NULL OR version = ?2)
                 ORDER BY version DESC LIMIT 1",
                params![GOLDEN_ID, version],
                |r| Ok((golden_info(r)?, r.get::<_, i64>(5)?)),
            )
            .optional()?;
        row.map(|(info, snapshot_id)| {
            let mut manifest = self.snapshot_manifest(snapshot_id)?;
            // Versions saved before the rename (decision #60) are stored as
            // "Golden Rig v<n>".
            manifest.machine.name = golden_name(info.version);
            Ok(GoldenVersion { info, manifest })
        })
        .transpose()
    }

    /// Every Primary Rig version, newest first.
    pub fn golden_history(&self) -> Result<Vec<GoldenVersionInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT version, parent_version, created_at, source_machine_id, note
             FROM golden_versions WHERE golden_id = ?1 ORDER BY version DESC",
        )?;
        let rows = stmt.query_map([GOLDEN_ID], golden_info)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Bundle these machines' latest scans (and their profile assignments),
    /// every Primary Rig version and every user target profile.
    pub fn export_bundle(
        &self,
        machine_ids: &[String],
        exported_from: Option<&str>,
        now: i64,
    ) -> Result<Bundle> {
        let mut bundle = Bundle::new(now, exported_from.map(str::to_owned));
        for id in machine_ids {
            let manifest = self
                .latest_manifest(id)?
                .ok_or_else(|| StoreError::UnknownMachine(id.clone()))?;
            let profile_id: Option<String> = self.conn.query_row(
                "SELECT target_profile_id FROM machines WHERE id = ?1",
                [id],
                |r| r.get(0),
            )?;
            bundle.machines.push(BundleMachine {
                manifest,
                profile_id,
            });
        }
        for info in self.golden_history()?.into_iter().rev() {
            let g = self
                .golden(Some(info.version))?
                .expect("listed version exists");
            bundle.golden.push(BundleGolden {
                version: info.version,
                created_at: info.created_at,
                source_machine_id: info.source_machine_id,
                note: info.note,
                manifest: g.manifest,
            });
        }
        bundle.profiles = self
            .target_profiles()?
            .into_iter()
            .filter(|p| !p.builtin)
            .map(|p| p.profile)
            .collect();
        bundle.ignored = self.suggestion_decisions()?;
        bundle.ignores = self.ignores()?;
        Ok(bundle)
    }

    /// Add what a bundle holds without overwriting anything (#38).
    ///
    /// Machines get a new `imported` snapshot (none if unchanged); this
    /// machine's own scan is skipped. User profiles are added if their ID is
    /// free. Primary Rig versions are added only as a fast-forward: when this
    /// machine has no Primary Rig, or its latest version appears in the
    /// bundle's history. A profile assignment is copied only if the machine
    /// has none here.
    pub fn import_bundle(
        &mut self,
        bundle: &Bundle,
        this_machine: Option<&str>,
        now: i64,
    ) -> Result<ImportReport> {
        let mut report = ImportReport::default();

        for p in &bundle.profiles {
            match self.target_profile(&p.id)? {
                Some(existing) if existing == *p => {}
                Some(_) => report.profiles_conflicting.push(p.id.clone()),
                None => match self.save_target_profile(p) {
                    Ok(()) => report.profiles_added.push(p.id.clone()),
                    Err(StoreError::BuiltinProfile(_)) => {
                        report.profiles_conflicting.push(p.id.clone());
                    }
                    Err(e) => return Err(e),
                },
            }
        }

        for m in &bundle.machines {
            let id = &m.manifest.machine.id;
            if Some(id.as_str()) == this_machine || id.starts_with("golden@") {
                report.skipped_own_machine |= Some(id.as_str()) == this_machine;
                continue;
            }
            let info = self.save_manifest(&m.manifest, SnapshotReason::Imported, now)?;
            let name = m.manifest.machine.name.clone();
            if info.unchanged {
                report.machines_unchanged.push(name);
            } else {
                report.machines_added.push(name);
            }
            if let Some(p) = &m.profile_id
                && self.machine_profile(id)?.is_none()
                && self.target_profile(p)?.is_some()
            {
                self.set_machine_profile(id, Some(p))?;
            }
        }

        report.golden = self.import_golden(bundle)?;
        for i in &bundle.ignored {
            if self.set_suggestion_ignored(
                &i.product_id,
                i.version.as_deref(),
                !i.undone,
                i.ignored_at,
            )? {
                report.ignored_added += 1;
            }
        }
        for e in &bundle.ignores {
            if self.set_ignore(e)? {
                report.ignores_changed += 1;
            }
        }
        Ok(report)
    }

    fn import_golden(&mut self, bundle: &Bundle) -> Result<GoldenImport> {
        let Some(theirs_latest) = bundle.golden.last() else {
            return Ok(GoldenImport::NotInBundle);
        };
        let start = match self.golden(None)? {
            None => 0,
            Some(ours) => {
                match bundle
                    .golden
                    .iter()
                    .rposition(|g| same_content(&g.manifest, &ours.manifest))
                {
                    Some(i) => i + 1,
                    None => {
                        // Their latest may be one of our older versions.
                        let ours_has_theirs = self.golden_history()?.iter().any(|v| {
                            self.golden(Some(v.version))
                                .ok()
                                .flatten()
                                .is_some_and(|g| same_content(&g.manifest, &theirs_latest.manifest))
                        });
                        return Ok(if ours_has_theirs {
                            GoldenImport::UpToDate
                        } else {
                            GoldenImport::Diverged
                        });
                    }
                }
            }
        };
        if start == bundle.golden.len() {
            return Ok(GoldenImport::UpToDate);
        }
        let from = bundle.exported_from.as_deref().unwrap_or("another machine");
        let mut added = Vec::new();
        for g in &bundle.golden[start..] {
            let note = match &g.note {
                Some(n) => format!("{n} (imported: v{} from {from})", g.version),
                None => format!("Imported: v{} from {from}", g.version),
            };
            let info = self.save_golden(
                &g.manifest,
                g.source_machine_id.as_deref(),
                Some(&note),
                g.created_at,
            )?;
            added.push(info.version);
        }
        Ok(GoldenImport::Imported(added))
    }

    /// Stop suggesting this product version for the Primary Rig (M9). Kept in
    /// the `decisions` table as an `ignore` of state `suggestion`, with the
    /// version in `note`. False if it was already ignored.
    pub fn ignore_suggestion(
        &mut self,
        product_id: &str,
        version: Option<&str>,
        now: i64,
    ) -> Result<bool> {
        self.set_suggestion_ignored(product_id, version, true, now)
    }

    /// Ignore a suggestion, or with `ignored` false offer it again (kept as
    /// an `accept`, so the change travels in bundles). The later decision
    /// wins; false if nothing changed.
    pub fn set_suggestion_ignored(
        &mut self,
        product_id: &str,
        version: Option<&str>,
        ignored: bool,
        at: i64,
    ) -> Result<bool> {
        let existing: Option<(i64, String, i64)> = self
            .conn
            .query_row(
                "SELECT id, decision, decided_at FROM decisions WHERE state = 'suggestion'
                     AND product_id = ?1 AND note IS ?2 ORDER BY id DESC LIMIT 1",
                params![product_id, version],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        self.decide(existing, ignored, at, |conn, decision| {
            conn.execute(
                "INSERT INTO decisions (machine_id, product_id, state, decision, decided_at, note)
                 VALUES (NULL, ?1, 'suggestion', ?2, ?3, ?4)",
                params![product_id, decision, at, version],
            )
        })
    }

    /// Update the latest decision row to `ignored` at `at` if that is newer
    /// and different, or add one with `insert` when there is none.
    fn decide(
        &mut self,
        existing: Option<(i64, String, i64)>,
        ignored: bool,
        at: i64,
        insert: impl FnOnce(&Connection, &str) -> rusqlite::Result<usize>,
    ) -> Result<bool> {
        let decision = if ignored { "ignore" } else { "accept" };
        match existing {
            Some((_, d, _)) if d == decision => Ok(false),
            Some((_, _, when)) if when > at => Ok(false),
            Some((id, _, _)) => {
                self.conn.execute(
                    "UPDATE decisions SET decision = ?2, decided_at = ?3 WHERE id = ?1",
                    params![id, decision, at],
                )?;
                Ok(true)
            }
            // Nothing to take back.
            None if !ignored => Ok(false),
            None => {
                insert(&self.conn, decision)?;
                Ok(true)
            }
        }
    }

    /// Every ignored Primary Rig suggestion, oldest first.
    pub fn ignored_suggestions(&self) -> Result<Vec<IgnoredSuggestion>> {
        Ok(self
            .suggestion_decisions()?
            .into_iter()
            .filter(|i| !i.undone)
            .collect())
    }

    /// Ignored suggestions and the ones offered again, for bundles.
    fn suggestion_decisions(&self) -> Result<Vec<IgnoredSuggestion>> {
        let mut stmt = self.conn.prepare(
            "SELECT product_id, note, decided_at, decision FROM decisions
             WHERE state = 'suggestion' ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(IgnoredSuggestion {
                product_id: r.get(0)?,
                version: r.get(1)?,
                ignored_at: r.get(2)?,
                undone: r.get::<_, String>(3)? == "accept",
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Ignore something on a machine (or every machine), or with
    /// `entry.undone` take it back. Kept in `decisions` with state `hidden`,
    /// the rules' key as `product_id` and the label in `note`. The later
    /// decision wins; false if nothing changed, or if the machine isn't
    /// known here.
    pub fn set_ignore(&mut self, entry: &IgnoreEntry) -> Result<bool> {
        if let Some(m) = &entry.machine_id
            && !self.machine_exists(m)?
        {
            return Ok(false);
        }
        let key = entry.key();
        let existing: Option<(i64, String, i64)> = self
            .conn
            .query_row(
                "SELECT id, decision, decided_at FROM decisions WHERE state = 'hidden'
                     AND machine_id IS ?1 AND product_id = ?2 ORDER BY id DESC LIMIT 1",
                params![entry.machine_id, key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        self.decide(existing, !entry.undone, entry.at, |conn, decision| {
            conn.execute(
                "INSERT INTO decisions (machine_id, product_id, state, decision, decided_at, note)
                 VALUES (?1, ?2, 'hidden', ?3, ?4, ?5)",
                params![entry.machine_id, key, decision, entry.at, entry.label],
            )
        })
    }

    /// Every ignore, taken back or not, oldest first.
    pub fn ignores(&self) -> Result<Vec<IgnoreEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT machine_id, product_id, note, decided_at, decision FROM decisions
             WHERE state = 'hidden' ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            let key: String = r.get(1)?;
            Ok(IgnoreEntry {
                machine_id: r.get(0)?,
                rules: serde_json::from_str(&key).unwrap_or_default(),
                label: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                at: r.get(3)?,
                undone: r.get::<_, String>(4)? == "accept",
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The ignores in force on a machine: its own and every machine's.
    pub fn ignores_for(&self, machine_id: &str) -> Result<Vec<IgnoreEntry>> {
        Ok(self
            .ignores()?
            .into_iter()
            .filter(|e| !e.undone && e.machine_id.as_deref().is_none_or(|m| m == machine_id))
            .collect())
    }

    /// What a machine is measured against: its target profile (or the whole
    /// Primary Rig), with everything ignored there left out.
    pub fn target_for(&self, machine_id: &str) -> Result<TargetProfile> {
        let mut profile = self.machine_profile(machine_id)?.unwrap_or_default();
        // Last matching rule wins, so these come after the profile's own.
        profile.rules.extend(self.ignore_rules(machine_id)?);
        Ok(profile)
    }

    /// The ignores in force on a machine as profile exclusions.
    pub fn ignore_rules(&self, machine_id: &str) -> Result<Vec<ProfileRule>> {
        Ok(self
            .ignores_for(machine_id)?
            .into_iter()
            .flat_map(|e| e.rules)
            .map(|matcher| ProfileRule {
                action: RuleAction::Exclude,
                matcher,
            })
            .collect())
    }

    fn machine_exists(&self, id: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM machines WHERE id = ?1)",
            [id],
            |r| r.get(0),
        )?)
    }

    /// Every machine with saved scans (not Primary Rig versions), by name.
    pub fn machines(&self) -> Result<Vec<MachineInfo>> {
        let ids: Vec<String> = self
            .conn
            .prepare(
                "SELECT id FROM machines WHERE id NOT IN
                     (SELECT s.machine_id FROM golden_versions g JOIN snapshots s ON s.id = g.snapshot_id)
                 ORDER BY name, id",
            )?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        ids.iter().map(|id| self.machine(id)).collect()
    }

    /// Every target profile, built-ins first, then by name.
    pub fn target_profiles(&self) -> Result<Vec<StoredProfile>> {
        let rows: Vec<(String, bool)> = self
            .conn
            .prepare(
                "SELECT id, builtin FROM profiles WHERE kind = 'target'
                 ORDER BY builtin DESC, name, id",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        rows.into_iter()
            .map(|(id, builtin)| {
                let profile = self
                    .target_profile(&id)?
                    .ok_or_else(|| StoreError::UnknownProfile(id.clone()))?;
                let machines = self
                    .conn
                    .prepare("SELECT id FROM machines WHERE target_profile_id = ?1 ORDER BY name")?
                    .query_map([&id], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?;
                Ok(StoredProfile {
                    profile,
                    builtin,
                    machines,
                })
            })
            .collect()
    }

    /// A target profile with its rules in order.
    pub fn target_profile(&self, id: &str) -> Result<Option<TargetProfile>> {
        let name: Option<String> = self
            .conn
            .query_row(
                "SELECT name FROM profiles WHERE id = ?1 AND kind = 'target'",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(name) = name else {
            return Ok(None);
        };
        let mut stmt = self.conn.prepare(
            "SELECT action, matcher FROM profile_rules WHERE profile_id = ?1 ORDER BY position",
        )?;
        let rules = stmt
            .query_map([id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .map(|row| {
                let (action, matcher) = row?;
                Ok(ProfileRule {
                    action: from_text("profile_rules.action", &action)?,
                    matcher: serde_json::from_str(&matcher).map_err(|_| StoreError::BadValue {
                        column: "profile_rules.matcher",
                        value: matcher,
                    })?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Some(TargetProfile {
            id: id.to_owned(),
            name,
            rules,
        }))
    }

    /// Create or replace a user target profile (rules replaced wholesale).
    /// Built-in presets and the Primary Rig profile can't be overwritten.
    pub fn save_target_profile(&mut self, profile: &TargetProfile) -> Result<()> {
        let existing: Option<(String, bool)> = self
            .conn
            .query_row(
                "SELECT kind, builtin FROM profiles WHERE id = ?1",
                [&profile.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if profile.id.trim().is_empty()
            || existing.is_some_and(|(kind, builtin)| builtin || kind != "target")
        {
            return Err(StoreError::BuiltinProfile(profile.id.clone()));
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO profiles (id, name, kind, golden_id) VALUES (?1, ?2, 'target', ?3)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name",
            params![profile.id, profile.name, GOLDEN_ID],
        )?;
        tx.execute(
            "DELETE FROM profile_rules WHERE profile_id = ?1",
            [&profile.id],
        )?;
        for (pos, rule) in profile.rules.iter().enumerate() {
            tx.execute(
                "INSERT INTO profile_rules (profile_id, position, action, matcher) VALUES (?1, ?2, ?3, ?4)",
                params![
                    profile.id,
                    pos as i64,
                    to_text(&rule.action),
                    serde_json::to_string(&rule.matcher).expect("serializable"),
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Assign a target profile to a machine (`None` clears it).
    pub fn set_machine_profile(
        &mut self,
        machine_id: &str,
        profile_id: Option<&str>,
    ) -> Result<()> {
        if let Some(p) = profile_id
            && self.target_profile(p)?.is_none()
        {
            return Err(StoreError::UnknownProfile(p.to_owned()));
        }
        let n = self.conn.execute(
            "UPDATE machines SET target_profile_id = ?2 WHERE id = ?1",
            params![machine_id, profile_id],
        )?;
        if n == 0 {
            return Err(StoreError::UnknownMachine(machine_id.to_owned()));
        }
        Ok(())
    }

    /// The target profile assigned to a machine, if any.
    pub fn machine_profile(&self, machine_id: &str) -> Result<Option<TargetProfile>> {
        let id: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT target_profile_id FROM machines WHERE id = ?1",
                [machine_id],
                |r| r.get(0),
            )
            .optional()?;
        match id {
            None => Err(StoreError::UnknownMachine(machine_id.to_owned())),
            Some(None) => Ok(None),
            Some(Some(id)) => self.target_profile(&id),
        }
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
                    i.platform_portable, i.confidence, i.id
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
                id: r.get(14)?,
            })
        })?;
        let raw = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut deps = self.conn.prepare(
            "SELECT product_id FROM dependencies WHERE asset_item_id = ?1 ORDER BY product_id",
        )?;
        let items = raw
            .into_iter()
            .map(|r| {
                let requires = deps
                    .query_map([r.id], |row| row.get(0).map(patchpilot_core::ProductId))?
                    .collect::<rusqlite::Result<_>>()?;
                let mut item = r.into_item()?;
                item.requires = requires;
                Ok(item)
            })
            .collect::<Result<Vec<_>>>()?;

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

/// The one Primary Rig per install (`profiles` row from migration 2).
const GOLDEN_ID: &str = "golden";

/// Same items and same products in use, for Primary Rig versions. The machine
/// header, time and item paths are ignored: paths don't change what the rig
/// is, and a bundle's copy may have the username removed (#38).
fn same_content(a: &Manifest, b: &Manifest) -> bool {
    let pathless = |m: &Manifest| -> Vec<Item> {
        m.items
            .iter()
            .map(|i| Item {
                path: PortablePath::Absolute(String::new()),
                ..i.clone()
            })
            .collect()
    };
    pathless(a) == pathless(b) && used_products(a) == used_products(b)
}

/// Products that items refer to, sorted: what a snapshot keeps of
/// `manifest.products`.
fn used_products(m: &Manifest) -> Vec<&Product> {
    let mut v: Vec<&Product> = m
        .products
        .iter()
        .filter(|p| {
            m.items
                .iter()
                .any(|i| i.product_id.as_ref() == Some(&p.canonical_id))
        })
        .collect();
    v.sort_by(|a, b| a.canonical_id.cmp(&b.canonical_id));
    v
}

fn golden_info(r: &rusqlite::Row) -> rusqlite::Result<GoldenVersionInfo> {
    Ok(GoldenVersionInfo {
        version: r.get(0)?,
        parent: r.get(1)?,
        created_at: r.get(2)?,
        source_machine_id: r.get(3)?,
        note: r.get(4)?,
    })
}

/// [`Store::save_manifest`] inside a caller's transaction.
fn save_manifest_tx(
    tx: &Transaction,
    manifest: &Manifest,
    reason: SnapshotReason,
    now: i64,
) -> Result<SnapshotInfo> {
    let manifest_hash = manifest_hash(manifest);
    let m = &manifest.machine;
    upsert_machine(tx, m, now)?;

    let latest: Option<(i64, String)> = tx
        .query_row(
            "SELECT id, manifest_hash FROM snapshots WHERE machine_id = ?1 ORDER BY id DESC LIMIT 1",
            [&m.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((id, hash)) = &latest
        && *hash == manifest_hash
    {
        return Ok(SnapshotInfo {
            id: *id,
            parent_id: None,
            manifest_hash,
            unchanged: true,
        });
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
        let (item_id, hash) = insert_item(tx, &m.id, item)?;
        tx.execute(
            "INSERT INTO snapshot_items (snapshot_id, position, item_id, hash) VALUES (?1, ?2, ?3, ?4)",
            params![snapshot_id, pos as i64, item_id, hash],
        )?;
    }
    Ok(SnapshotInfo {
        id: snapshot_id,
        parent_id,
        manifest_hash,
        unchanged: false,
    })
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
    let id: i64 = tx.query_row(
        "SELECT id FROM items WHERE machine_id = ?1 AND item_hash = ?2",
        params![machine_id, hash],
        |r| r.get(0),
    )?;
    for p in &item.requires {
        tx.execute(
            "INSERT OR IGNORE INTO dependencies (asset_item_id, product_id) VALUES (?1, ?2)",
            params![id, p.0],
        )?;
    }
    Ok((id, hash))
}

struct RawItem {
    id: i64,
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
            requires: Vec::new(),
            needs_files: Vec::new(),
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

fn manifest_hash(m: &Manifest) -> String {
    let mut v = serde_json::to_value(m).expect("serializable");
    if let Some(o) = v.as_object_mut() {
        o.remove("generated_at");
    }
    hash_json(&v)
}

fn hash_json<T: Serialize>(v: &T) -> String {
    let json = serde_json::to_vec(v).expect("serializable");
    format!("blake3:{}", blake3::hash(&json).to_hex())
}
