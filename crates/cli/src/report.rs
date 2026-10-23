//! Plain-text diff report.

use std::fmt::Write;

use patchpilot_core::{Manifest, TargetProfile};
use patchpilot_parity::{ParityScore, ParityState};

pub fn render(
    golden: &Manifest,
    machine: &Manifest,
    profile: &TargetProfile,
    score: &ParityScore,
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
        "Parity {}%  ({} vs Golden {}{profile_note})",
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
            (Some(g), Some(m)) if g != m => format!("{m} (Golden {g})"),
            (Some(g), None) => format!("Golden {g}"),
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
    out
}

fn state_title(s: ParityState) -> &'static str {
    match s {
        ParityState::Missing => "Missing",
        ParityState::Incomplete => "Missing a format",
        ParityState::Outdated => "Out of date",
        ParityState::Modified => "Different from Golden",
        ParityState::VersionUnknown => "Version can't be compared",
        ParityState::NewerThanGolden => "Newer than Golden",
        ParityState::Extra => "Not in Golden",
        ParityState::NotApplicable => "Not available on this OS",
        ParityState::InParity => "In parity",
    }
}
