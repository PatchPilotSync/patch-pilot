//! New-machine setup v1 (strategy section 17): the Up To Speed plan frozen
//! when setup starts, each step's live progress after rescans, a few final
//! checks only the user can confirm, and the setup report.
//!
//! Read-only like [`crate::up_to_speed`]: progress comes from comparing the
//! machine's latest scan with the plan, never from installing anything.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use patchpilot_core::{Manifest, Os, ProductId, TargetProfile};
use serde::Serialize;

use crate::diff::{ParityState, diff_manifests};
use crate::{
    Need, OtherFormats, Plan, Step, TemplateReadiness, align_products, apply_profile, compare,
    template_readiness, up_to_speed,
};

/// Checks after the plan that Patch Pilot can't see for itself, so the user
/// ticks them: `(key, title, detail)`.
const CHECKS: &[(&str, &str, &str)] = &[
    (
        "authorize",
        "Authorize your plug-ins",
        "Sign in to iLok and your vendor apps on this machine. Patch Pilot never handles licences.",
    ),
    (
        "libraries",
        "Point sample libraries at their new location",
        "Kontakt and other instruments may ask where their libraries live on this drive.",
    ),
    (
        "open-daws",
        "Open your DAWs once",
        "Let Reaper and Ableton find the new plug-ins, then open a template to check it.",
    ),
];

/// Group label for [`CHECKS`].
const CHECKS_GROUP: &str = "Final checks";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Todo,
    /// Some of the step's products are done.
    Partly,
    /// Done, as the latest scan shows (or ticked, for a final check).
    Done,
    /// The user marked it done or skipped it; the scan doesn't show it.
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupStep {
    /// Stable for the whole setup: `step:<n>` or `check:<name>`.
    pub key: String,
    /// Plan number, None for final checks.
    pub number: Option<usize>,
    /// "Foundation", "Vendor apps", ..., "Final checks".
    pub group: String,
    pub title: String,
    /// The vendor app or licensing tool the step uses (scanner adapter ID).
    pub app: Option<String>,
    /// The vendor's website, for plug-ins from the vendor's own downloads.
    pub link: Option<String>,
    /// Each open line is best found by searching for it (no known vendor).
    pub search: bool,
    pub status: StepStatus,
    /// Patch Pilot ticks it after a rescan; false for final checks.
    pub automatic: bool,
    /// Lines still to do and lines done, from the plan's details.
    pub open: Vec<String>,
    pub finished: Vec<String>,
    /// Products still to do.
    pub open_products: Vec<ProductId>,
    /// For each open product: not installed at all, or an older version.
    pub open_needs: Vec<Need>,
    pub after: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupProgress {
    pub steps: Vec<SetupStep>,
    /// Steps done or skipped, of all steps.
    pub done: usize,
    pub total: usize,
    pub score_before: u8,
    pub score_now: u8,
    /// Templates on the machine and the plug-ins each one still lacks.
    pub templates: Vec<TemplateReadiness>,
    pub templates_ready: usize,
    /// Every step done or skipped and every template ready.
    pub complete: bool,
    /// Plug-ins here in other formats than the Primary Rig's, which count as
    /// installed (see [`Plan::other_formats`]).
    pub other_formats: Vec<OtherFormats>,
}

/// Progress of a setup that started from `baseline` (the machine's scan when
/// setup began) towards `golden` through `profile`, judged by `machine` (its
/// latest scan). `ticked` holds the keys the user marked done. Pass
/// manifests as stored; IDs are aligned here.
pub fn setup_progress(
    golden: &Manifest,
    profile: &TargetProfile,
    baseline: &Manifest,
    machine: &Manifest,
    ticked: &BTreeSet<String>,
) -> SetupProgress {
    let (g0, b) = aligned(golden, baseline);
    let plan = up_to_speed(&g0, profile, &b);
    let (g, m) = aligned(golden, machine);
    let live = up_to_speed(&g, profile, &m);
    let target = apply_profile(&g, profile);
    let open: BTreeSet<ProductId> = diff_manifests(&target, &m)
        .products
        .into_iter()
        .filter(|p| matches!(p.state, ParityState::Missing | ParityState::Outdated))
        .map(|p| p.product_id)
        .collect();

    let mut steps: Vec<SetupStep> = plan
        .steps
        .iter()
        .map(|s| plan_step(s, &open, &live, ticked))
        .collect();
    steps.extend(CHECKS.iter().map(|(key, title, detail)| {
        let key = format!("check:{key}");
        SetupStep {
            status: if ticked.contains(&key) {
                StepStatus::Done
            } else {
                StepStatus::Todo
            },
            key,
            number: None,
            group: CHECKS_GROUP.to_owned(),
            title: (*title).to_owned(),
            app: None,
            link: None,
            search: false,
            automatic: false,
            open: vec![(*detail).to_owned()],
            finished: Vec::new(),
            open_products: Vec::new(),
            open_needs: Vec::new(),
            after: Vec::new(),
        }
    }));

    let templates = template_readiness(&m);
    let templates_ready = templates.iter().filter(|t| t.is_ready()).count();
    let done = steps
        .iter()
        .filter(|s| matches!(s.status, StepStatus::Done | StepStatus::Skipped))
        .count();
    SetupProgress {
        complete: done == steps.len() && templates_ready == templates.len(),
        total: steps.len(),
        done,
        score_before: compare(golden, profile, baseline).score,
        score_now: compare(golden, profile, machine).score,
        templates_ready,
        templates,
        steps,
        other_formats: live.other_formats,
    }
}

/// Get Up To Speed with live progress: the plan made from `baseline`, each
/// step ticked off as `machine` (the latest scan) shows it done. Like
/// [`setup_progress`] without the final checks; `complete` ignores templates.
pub fn plan_progress(
    golden: &Manifest,
    profile: &TargetProfile,
    baseline: &Manifest,
    machine: &Manifest,
    ticked: &BTreeSet<String>,
) -> SetupProgress {
    let mut p = setup_progress(golden, profile, baseline, machine, ticked);
    p.steps.retain(|s| s.automatic);
    p.total = p.steps.len();
    p.done = p
        .steps
        .iter()
        .filter(|s| matches!(s.status, StepStatus::Done | StepStatus::Skipped))
        .count();
    p.complete = p.done == p.total;
    p
}

/// True while the plan made from `baseline` still covers everything
/// `machine` needs, so it can keep tracking progress. False once something
/// new is missing (or the Primary Rig changed), when a fresh plan is due.
pub fn plan_still_covers(
    golden: &Manifest,
    profile: &TargetProfile,
    baseline: &Manifest,
    machine: &Manifest,
) -> bool {
    let (g0, b) = aligned(golden, baseline);
    let plan = up_to_speed(&g0, profile, &b);
    let planned: BTreeSet<&ProductId> = plan.steps.iter().flat_map(|s| &s.products).collect();
    let (g, m) = aligned(golden, machine);
    up_to_speed(&g, profile, &m).steps.iter().all(|s| {
        if s.products.is_empty() {
            plan.steps.iter().any(|p| p.title == s.title)
        } else {
            s.products.iter().all(|p| planned.contains(p))
        }
    })
}

fn aligned(golden: &Manifest, machine: &Manifest) -> (Manifest, Manifest) {
    let (mut g, mut m) = (golden.clone(), machine.clone());
    align_products(&mut g, &mut m);
    (g, m)
}

fn plan_step(
    s: &Step,
    open: &BTreeSet<ProductId>,
    live: &Plan,
    ticked: &BTreeSet<String>,
) -> SetupStep {
    let key = format!("step:{}", s.number);
    // `details` and `products` line up one to one when a step has products.
    let mut open_lines = Vec::new();
    let mut finished = Vec::new();
    let mut open_products = Vec::new();
    let mut open_needs = Vec::new();
    // Something missing when the plan was made can turn up older since
    // (an old Arturia Software Center found): say what's left now.
    let live_line = |p: &ProductId| {
        live.steps.iter().find_map(|l| {
            let i = l.products.iter().position(|x| x == p)?;
            Some((l.details.get(i)?.clone(), *l.needs.get(i)?))
        })
    };
    for (i, p) in s.products.iter().enumerate() {
        let line = s.details.get(i).cloned().unwrap_or_else(|| p.0.clone());
        if open.contains(p) {
            let need = s.needs.get(i).copied().unwrap_or(Need::Install);
            let (line, need) = live_line(p).unwrap_or((line, need));
            open_lines.push(line);
            open_needs.push(need);
            open_products.push(p.clone());
        } else {
            finished.push(line);
        }
    }
    let status = if s.products.is_empty() {
        // "Install Native Access" when Primary Rig doesn't list the app: done once
        // the live plan stops asking for it.
        if live.steps.iter().any(|l| l.title == s.title) {
            StepStatus::Todo
        } else {
            StepStatus::Done
        }
    } else if open_products.is_empty() {
        StepStatus::Done
    } else if finished.is_empty() {
        StepStatus::Todo
    } else {
        StepStatus::Partly
    };
    let status = match status {
        StepStatus::Todo | StepStatus::Partly if ticked.contains(&key) => StepStatus::Skipped,
        s => s,
    };
    SetupStep {
        key,
        number: Some(s.number),
        group: s.phase.title().to_owned(),
        // "Install X" becomes "Update X" when that's what's left.
        title: live
            .steps
            .iter()
            .find(|l| !s.products.is_empty() && l.products == s.products)
            .map_or_else(|| s.title.clone(), |l| l.title.clone()),
        app: s.app.clone(),
        link: s.link.clone(),
        search: s.search,
        status,
        automatic: true,
        open: open_lines,
        finished,
        open_products,
        open_needs,
        after: s.after.clone(),
    }
}

/// What the report says about the setup besides its progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportInfo<'a> {
    pub machine: &'a str,
    pub os: Os,
    /// Target profile name, None for the whole Primary Rig.
    pub profile: Option<&'a str>,
    pub golden_version: u32,
    /// Unix seconds.
    pub started_at: i64,
    /// Unix seconds; when the report was made if setup isn't finished.
    pub finished_at: Option<i64>,
    pub now: i64,
}

/// The setup report as Markdown, for the user to keep or share.
pub fn setup_report(info: &ReportInfo, p: &SetupProgress) -> String {
    let mut out = String::new();
    let os = match info.os {
        Os::Macos => "macOS",
        Os::Windows => "Windows",
    };
    let profile = info.profile.unwrap_or("Whole Primary Rig");
    let end = info.finished_at.unwrap_or(info.now);
    let _ = writeln!(out, "# Setup report: {}\n", info.machine);
    let _ = writeln!(
        out,
        "{os} · {profile} · compared with Primary Rig v{}\n",
        info.golden_version
    );
    let _ = writeln!(out, "- Started: {}", utc(info.started_at));
    match info.finished_at {
        Some(f) => {
            let _ = writeln!(out, "- Finished: {}", utc(f));
        }
        None => {
            let _ = writeln!(out, "- Not finished yet (report made {})", utc(info.now));
        }
    }
    let _ = writeln!(out, "- Took: {}", duration(end - info.started_at));
    let _ = writeln!(out, "- Parity score: {} → {}", p.score_before, p.score_now);
    let _ = writeln!(out, "- Steps done: {} of {}", p.done, p.total);
    let ready = if p.templates.is_empty() {
        "no templates on this machine".to_owned()
    } else {
        format!("{} of {} ready", p.templates_ready, p.templates.len())
    };
    let _ = writeln!(out, "- Templates: {ready}");

    let section = |out: &mut String, title: &str, steps: &[&SetupStep], open: bool| {
        if steps.is_empty() {
            return;
        }
        let _ = writeln!(out, "\n## {title}\n");
        for s in steps {
            let _ = writeln!(out, "- {}", s.title);
            let lines: &[String] = if open {
                &s.open
            } else if s.automatic {
                &s.finished
            } else {
                &[]
            };
            for l in lines {
                let _ = writeln!(out, "  - {l}");
            }
        }
    };
    let by = |want: &[StepStatus]| -> Vec<&SetupStep> {
        p.steps
            .iter()
            .filter(|s| want.contains(&s.status))
            .collect()
    };
    section(&mut out, "Done", &by(&[StepStatus::Done]), false);
    section(
        &mut out,
        "Marked done by hand",
        &by(&[StepStatus::Skipped]),
        true,
    );
    section(
        &mut out,
        "Still to do",
        &by(&[StepStatus::Todo, StepStatus::Partly]),
        true,
    );

    if !p.other_formats.is_empty() {
        let _ = writeln!(out, "\n## Installed in other formats\n");
        for o in &p.other_formats {
            let _ = writeln!(
                out,
                "- {}: {} here, Primary Rig also has {}",
                o.name,
                o.here.join(", "),
                o.missing.join(", ")
            );
        }
    }

    let not_ready: Vec<_> = p.templates.iter().filter(|t| !t.is_ready()).collect();
    if !not_ready.is_empty() {
        let _ = writeln!(out, "\n## Templates not ready\n");
        for t in not_ready {
            let missing: Vec<_> = t.missing.iter().map(|m| m.0.as_str()).collect();
            let _ = writeln!(out, "- {} needs {}", t.name, missing.join(", "));
        }
    }
    let _ = writeln!(
        out,
        "\nMade by Patch Pilot. Plug-ins and licences were never copied between machines."
    );
    out
}

/// `1 h 5 min`, `12 min`, `3 d 2 h`.
fn duration(secs: i64) -> String {
    let m = secs.max(0) / 60;
    let (d, h, m) = (m / 1440, m % 1440 / 60, m % 60);
    match (d, h) {
        (0, 0) => format!("{m} min"),
        (0, h) => format!("{h} h {m} min"),
        (d, h) => format!("{d} d {h} h"),
    }
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
    fn durations_and_dates() {
        assert_eq!(duration(59), "0 min");
        assert_eq!(duration(65 * 60), "1 h 5 min");
        assert_eq!(duration(3 * 86_400 + 2 * 3600), "3 d 2 h");
        assert_eq!(utc(1_792_778_640), "2026-10-23 18:04 UTC");
    }
}
