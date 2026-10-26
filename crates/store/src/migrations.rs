//! Embedded schema migrations, tracked with `PRAGMA user_version`.

use rusqlite::{Connection, TransactionBehavior};

use crate::{Result, StoreError};

/// `(version, sql)`, ascending. Never edit a released migration; add a new one.
pub const MIGRATIONS: &[(u32, &str)] = &[
    (1, include_str!("../migrations/0001_init.sql")),
    (2, include_str!("../migrations/0002_golden.sql")),
    (3, include_str!("../migrations/0003_target_profiles.sql")),
    (4, include_str!("../migrations/0004_restore_points.sql")),
    (5, include_str!("../migrations/0005_needs_files.sql")),
    (
        6,
        include_str!("../migrations/0006_snapshot_items_index.sql"),
    ),
];

pub const LATEST_VERSION: u32 = MIGRATIONS[MIGRATIONS.len() - 1].0;

/// Apply every migration newer than the database, each in its own transaction,
/// once even when several connections open it at the same time.
/// Refuses a database written by a newer build.
pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if current > LATEST_VERSION {
        return Err(StoreError::TooNew {
            found: current,
            supported: LATEST_VERSION,
        });
    }
    for &(version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
        // The app opens the store from several threads at launch. Take the
        // write lock before migrating and look again, so a migration another
        // connection just applied isn't applied twice ("index ... already
        // exists" in 0.4.7).
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now: u32 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if now >= version {
            continue;
        }
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}
