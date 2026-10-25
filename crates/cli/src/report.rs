//! Plain-text diff and history reports.

use std::fmt::Write;

use patchpilot_core::{GoldenVersionInfo, ItemKind, Manifest, TargetProfile};
use patchpilot_parity::{ParityScore, ParityState, Plan, TemplateReadiness};
use patchpilot_store::{SnapshotReason, SnapshotSummary};

pub fn render(
    golden: &Manifest,
    machine: &Manifest,
    profile: &TargetProfile,
    score: &ParityScore,
    readiness: &[TemplateReadiness],
    all: bool,
) -> String {
    let mut out = String::new();
    let profile_note = if profile.name.is_empty() {
        String::new()
    } else {
        format!(", profile: {}", profile.name)
    };
    let _ = writeln!(
        out,
        "Parity {}%  ({} vs {}{profile_note})",
        score.score, machine.machine.name, golden.machine.name
    );
    for r in &score.reasons {
        let _ = writeln!(out, "  - {} (-{:.1})", r.text, r.points);
    }

    let mut current = None;
    for p in &score.diff.products {
        if p.state == ParityState::InParity && !all {
            continue;
        }
        if current != Some(p.state) {
            current = Some(p.state);
            let _ = writeln!(out, "\n{}", state_title(p.state));
        }
        let versions = match (&p.golden_version, &p.machine_version) {
            (Some(g), Some(m)) if g != m => format!("{m} (Primary {g})"),
            (Some(g), None) => format!("Primary {g}"),
            (None, Some(m)) | (Some(_), Some(m)) => m.clone(),
            (None, None) => String::new(),
        };
        let formats = if p.missing_formats.is_empty() {
            String::new()
        } else {
            let names: Vec<_> = p
                .missing_formats
                .iter()
                .map(|f| format!("{f:?}").to_uppercase())
                .collect();
            format!("  missing {}", names.join(", "))
        };
        let line = format!("  {:<40} {versions}{formats}", p.name);
        let _ = writeln!(out, "{}", line.trim_end());
    }
    if !score.diff.unresolved.is_empty() {
        let _ = writeln!(
            out,
            "\n{} item(s) on {} couldn't be identified and weren't compared.",
            score.diff.unresolved.len(),
            machine.machine.name
        );
    }
    if !readiness.is_empty() {
        let ready = readiness.iter().filter(|t| t.is_ready()).count();
        let _ = writeln!(
            out,
            "\nTemplates: {ready} of {} ready on {}",
            readiness.len(),
            machine.machine.name
        );
        for t in readiness.iter().filter(|t| !t.is_ready() || all) {
            if t.is_ready() {
                let _ = writeln!(out, "  {}  ready", t.name);
                continue;
            }
            let names: Vec<_> = t
                .missing
                .iter()
                .map(|id| {
                    golden
                        .product(id)
                        .or_else(|| machine.product(id))
                        .map_or_else(|| id.to_string(), |p| p.name.clone())
                })
                .collect();
            let _ = writeln!(out, "  {}  needs {}", t.name, names.join(", "));
        }
    }
    out
}

/// The Up To Speed checklist, grouped by phase.
pub fn plan(golden: &Manifest, machine: &Manifest, profile: &TargetProfile, plan: &Plan) -> String {
    let mut out = String::new();
    let profile_note = if profile.name.is_empty() {
        String::new()
    } else {
        format!(", profile: {}", profile.name)
    };
    let _ = writeln!(
        out,
        "Bring {} up to speed with {}{profile_note}",
        machine.machine.name, golden.machine.name
    );
    if plan.steps.is_empty() {
        let _ = writeln!(out, "Nothing to do: it already matches.");
        return out;
    }
    let _ = writeln!(
        out,
        "{} step(s). Nothing is installed or copied for you.",
        plan.steps.len()
    );
    let mut phase = None;
    for s in &plan.steps {
        if phase != Some(s.phase) {
            phase = Some(s.phase);
            let _ = writeln!(out, "\n{}", s.phase.title());
        }
        let after = if s.after.is_empty() {
            String::new()
        } else {
            let n: Vec<_> = s.after.iter().map(|a| a.to_string()).collect();
            format!("  (after {})", n.join(", "))
        };
        let _ = writeln!(out, "  {}. {}{after}", s.number, s.title);
        for d in &s.details {
            let _ = writeln!(out, "       {d}");
        }
    }
    out
}

fn state_title(s: ParityState) -> &'static str {
    match s {
        ParityState::Missing => "Missing",
        ParityState::Incomplete => "Missing a format",
        ParityState::Outdated => "Out of date",
        ParityState::Modified => "Different from Primary",
        ParityState::VersionUnknown => "Version can't be compared",
        ParityState::NewerThanGolden => "Newer than Primary",
        ParityState::Extra => "Not in Primary",
        ParityState::NotApplicable => "Not available on this OS",
        ParityState::InParity => "In parity",
    }
}

/// Snapshot history, newest first, e.g.
/// `#3  2026-10-23 18:04 UTC  scan        412 items  +2 -1`.
pub fn history(machine: &str, snapshots: &[SnapshotSummary]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{} snapshot(s) of {machine}", snapshots.len());
    for s in snapshots {
        let reason = match s.reason {
            SnapshotReason::FirstScan => "first scan",
            SnapshotReason::Scan => "scan",
            SnapshotReason::BeforeApply => "before apply",
            SnapshotReason::DriftAccepted => "drift accepted",
            SnapshotReason::Manual => "manual",
            SnapshotReason::Golden => "primary rig",
            SnapshotReason::Imported => "imported",
        };
        let changes = if s.parent_id.is_some() {
            format!("  +{} -{}", s.added, s.removed)
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "  #{:<4} {}  {reason:<14} {:>5} items{changes}",
            s.id,
            utc(s.created_at),
            s.items
        );
    }
    out
}

/// One Primary Rig version: header, then counts by plug-in format.
pub fn golden(v: &GoldenVersionInfo, source: Option<&str>, m: &Manifest) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}", golden_line(v, source));
    let plugins: Vec<_> = m
        .items
        .iter()
        .filter(|i| i.kind == ItemKind::Plugin)
        .collect();
    let mut formats: Vec<(String, usize)> = Vec::new();
    for f in plugins.iter().filter_map(|i| i.format) {
        let name = serde_json::to_value(f)
            .ok()
            .and_then(|v| v.as_str().map(str::to_uppercase))
            .unwrap_or_default();
        match formats.iter_mut().find(|(n, _)| *n == name) {
            Some((_, c)) => *c += 1,
            None => formats.push((name, 1)),
        }
    }
    let formats: Vec<_> = formats.iter().map(|(n, c)| format!("{c} {n}")).collect();
    let _ = writeln!(
        out,
        "  {} products, {} plug-ins ({})",
        m.products.len(),
        plugins.len(),
        formats.join(", ")
    );
    out
}

/// Primary Rig versions, newest first.
pub fn golden_history(versions: &[(&GoldenVersionInfo, Option<String>)]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{} Primary Rig version(s)", versions.len());
    for (v, source) in versions {
        let _ = writeln!(out, "  {}", golden_line(v, source.as_deref()));
    }
    out
}

/// `v2  2026-10-23 18:04 UTC  from Travel MacBook  "note"`.
fn golden_line(v: &GoldenVersionInfo, source: Option<&str>) -> String {
    let mut line = format!("Primary Rig v{}  {}", v.version, utc(v.created_at));
    if let Some(s) = source {
        let _ = write!(line, "  from {s}");
    }
    if let Some(n) = &v.note {
        let _ = write!(line, "  \"{n}\"");
    }
    line
}

/// `YYYY-MM-DD HH:MM UTC` from Unix seconds (proleptic Gregorian).
fn utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates() {
        assert_eq!(utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00 UTC");
        assert_eq!(utc(1_792_778_640), "2026-10-23 18:04 UTC");
    }
}
