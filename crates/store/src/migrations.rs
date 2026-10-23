//! Embedded schema migrations, tracked with `PRAGMA user_version`.

use rusqlite::Connection;

use crate::{Result, StoreError};

/// `(version, sql)`, ascending. Never edit a released migration; add a new one.
pub const MIGRATIONS: &[(u32, &str)] = &[
    (1, include_str!("../migrations/0001_init.sql")),
    (2, include_str!("../migrations/0002_golden.sql")),
    (3, include_str!("../migrations/0003_target_profiles.sql")),
];

pub const LATEST_VERSION: u32 = MIGRATIONS[MIGRATIONS.len() - 1].0;

/// Apply every migration newer than the database, each in its own transaction.
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
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}
