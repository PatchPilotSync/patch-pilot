//! Rig Report: a shareable summary of this machine, previewed in the app and
//! saved as a web page or Markdown file where the user picks.

use std::path::{Path, PathBuf};

use patchpilot_parity::{ReportOptions, ReportParity, RigReport, compare, rig_report};
use patchpilot_store::Store;

use crate::rig::{db_path, err, open};

/// The report for this machine's latest scan, in both forms.
#[tauri::command]
pub async fn rig_report_preview(options: ReportOptions) -> Result<RigReport, String> {
    let db = db_path()?;
    let store = open(&db)?;
    let id = patchpilot_store::read_machine_id(patchpilot_local::app_dir(&db))
        .map_err(|e| e.to_string())?
        .ok_or("scan this machine first")?;
    build(&store, &id, &options)
}

/// Writes the report to `path`: Markdown when it ends in `.md`, otherwise the
/// web page. The path comes from the save dialog, so the user chose it.
#[tauri::command]
pub async fn save_rig_report(path: PathBuf, options: ReportOptions) -> Result<(), String> {
    let report = rig_report_preview(options).await?;
    let markdown = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("md"));
    write_atomically(
        &path,
        if markdown {
            &report.markdown
        } else {
            &report.html
        },
    )
}

fn build(store: &Store, id: &str, options: &ReportOptions) -> Result<RigReport, String> {
    let scan = store
        .latest_manifest(id)
        .map_err(err)?
        .ok_or("scan this machine first")?;
    let parity = match store.golden(None).map_err(err)? {
        Some(g) => {
            let profile = store.machine_profile(id).map_err(err)?.unwrap_or_default();
            Some(ReportParity {
                score: compare(&g.manifest, &profile, &scan).score,
                golden_version: g.info.version,
            })
        }
        None => None,
    };
    Ok(rig_report(&scan, parity, options))
}

/// Staged next to the target, then renamed over it, so a failed write never
/// leaves half a file.
fn write_atomically(path: &Path, text: &str) -> Result<(), String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("{}: not a file name", path.display()))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(".patchpilot-tmp");
    let tmp = path.with_file_name(tmp_name);
    let result = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::Manifest;

    fn fixture() -> Manifest {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/parity/plan/golden.json");
        Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn report_includes_the_score_once_there_is_a_golden_rig() {
        let m = fixture();
        let id = m.machine.id.clone();
        let mut store = Store::open_in_memory().unwrap();
        patchpilot_local::save_scan(&mut store, &m).unwrap();
        let opts = ReportOptions::default();

        let r = build(&store, &id, &opts).unwrap();
        assert!(!r.markdown.contains("in parity"));

        store.save_golden(&m, Some(&id), None, 5).unwrap();
        let r = build(&store, &id, &opts).unwrap();
        assert!(r.markdown.contains("100% in parity with Golden Rig v1"));
        assert!(build(&store, "someone-else", &opts).is_err());
    }

    #[test]
    fn writes_replace_the_file_without_leftovers() {
        let dir = std::env::temp_dir().join(format!("pp-report-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Rig Report.html");
        write_atomically(&path, "one").unwrap();
        write_atomically(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
