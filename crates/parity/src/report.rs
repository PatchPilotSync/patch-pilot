//! Rig Report: a shareable summary of one machine's setup, as a
//! self-contained web page or as Markdown for a forum post.
//!
//! Built from the [`Inventory`], so it only ever holds names, formats and
//! versions: no file paths, machine IDs, usernames, serials or licence details.

use std::fmt::Write;

use patchpilot_core::{Arch, ItemKind, Manifest, Os, PluginFormat};
use serde::{Deserialize, Serialize};

use crate::{Inventory, inventory};

/// What the user chose to include. Mirrored by `ReportOptions` in the app's `src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReportOptions {
    /// Shown as the report's heading, e.g. "Home studio". Empty means "My rig".
    pub title: String,
    /// Already formatted for the reader's locale, e.g. "24 October 2026".
    pub date: String,
    /// List every plug-in by vendor, not just the counts.
    pub plugins: bool,
    /// Show each plug-in's and app's version.
    pub versions: bool,
    /// List vendor apps and licensing tools.
    pub apps: bool,
    /// Show the parity score, when one is given.
    pub parity: bool,
}

impl Default for ReportOptions {
    fn default() -> Self {
        Self {
            title: String::new(),
            date: String::new(),
            plugins: true,
            versions: true,
            apps: true,
            parity: true,
        }
    }
}

/// The machine's score against the Golden Rig.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportParity {
    pub score: u8,
    pub golden_version: u32,
}

/// The report in both forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RigReport {
    pub html: String,
    pub markdown: String,
}

pub fn rig_report(m: &Manifest, parity: Option<ReportParity>, opts: &ReportOptions) -> RigReport {
    let r = Report::new(m, parity, opts);
    RigReport {
        html: r.html(),
        markdown: r.markdown(),
    }
}

struct Report<'a> {
    inv: Inventory,
    opts: &'a ReportOptions,
    title: String,
    /// "macOS · Apple silicon · 24 October 2026"
    subtitle: String,
    parity: Option<ReportParity>,
}

impl<'a> Report<'a> {
    fn new(m: &Manifest, parity: Option<ReportParity>, opts: &'a ReportOptions) -> Self {
        let title = match opts.title.trim() {
            "" => "My rig".to_owned(),
            t => t.to_owned(),
        };
        let mut subtitle = vec![platform(m.machine.os, m.machine.arch).to_owned()];
        if !opts.date.trim().is_empty() {
            subtitle.push(opts.date.trim().to_owned());
        }
        Self {
            inv: inventory(m),
            opts,
            title,
            subtitle: subtitle.join(" · "),
            parity: parity.filter(|_| opts.parity),
        }
    }

    fn vendor_count(&self) -> usize {
        self.inv
            .vendors
            .iter()
            .filter(|v| !v.vendor.is_empty())
            .count()
    }

    fn stats(&self) -> Vec<(String, String)> {
        let mut s = vec![
            (
                count(self.inv.products),
                plural(self.inv.products, "plug-in"),
            ),
            (
                count(self.vendor_count()),
                plural(self.vendor_count(), "vendor"),
            ),
        ];
        if self.inv.templates > 0 {
            s.push((
                count(self.inv.templates),
                plural(self.inv.templates, "Reaper template"),
            ));
        }
        s
    }

    fn formats_line(&self) -> String {
        self.inv
            .formats
            .iter()
            .map(|f| format!("{} {}", format_label(f.format), count(f.count)))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    fn parity_line(&self) -> Option<String> {
        self.parity.map(|p| {
            format!(
                "{}% in parity with Golden Rig v{}",
                p.score, p.golden_version
            )
        })
    }

    fn markdown(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "## {}", md(&self.title));
        let _ = writeln!(out, "*{}*\n", md(&self.subtitle));
        let stats: Vec<String> = self
            .stats()
            .into_iter()
            .map(|(n, w)| format!("**{n}** {w}"))
            .collect();
        let _ = writeln!(out, "{}", stats.join(" · "));
        if !self.inv.formats.is_empty() {
            let _ = writeln!(out, "\nFormats: {}", self.formats_line());
        }
        if let Some(p) = self.parity_line() {
            let _ = writeln!(out, "\n{p}");
        }
        if self.opts.apps && !self.inv.apps.is_empty() {
            let _ = writeln!(out, "\n### Vendor apps and licensing tools\n");
            for a in &self.inv.apps {
                let _ = writeln!(out, "- {}{}", md(&a.name), self.md_version(&a.version));
            }
        }
        if self.opts.plugins && self.inv.products > 0 {
            let _ = writeln!(out, "\n### Plug-ins");
            for v in &self.inv.vendors {
                let _ = writeln!(
                    out,
                    "\n**{}** ({})\n",
                    md(vendor_label(&v.vendor)),
                    v.products.len()
                );
                for p in &v.products {
                    let _ = writeln!(
                        out,
                        "- {} ({}){}",
                        md(&p.name),
                        formats(&p.formats),
                        self.md_version(&p.version)
                    );
                }
            }
        }
        let _ = writeln!(out, "\n*{}*", FOOTER);
        out
    }

    fn md_version(&self, v: &Option<String>) -> String {
        match v {
            Some(v) if self.opts.versions => {
                let v = if v.starts_with(|c: char| c.is_ascii_digit()) {
                    format!("v{v}")
                } else {
                    v.clone()
                };
                format!(" {}", md(&v))
            }
            _ => String::new(),
        }
    }

    fn html(&self) -> String {
        let mut out = String::new();
        let _ = write!(
            out,
            "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
             <meta name=\"generator\" content=\"Patch Pilot\">\n\
             <title>{} · Rig Report</title>\n<style>{CSS}</style>\n</head>\n<body>\n<main>\n",
            esc(&self.title)
        );
        let _ = write!(
            out,
            "<p class=\"brand\">{MARK}<span>Rig Report</span></p>\n\
             <h1>{}</h1>\n<p class=\"sub\">{}</p>\n",
            esc(&self.title),
            esc(&self.subtitle)
        );

        out.push_str("<section class=\"stats\">\n");
        for (n, w) in self.stats() {
            let _ = writeln!(out, "<div><b>{n}</b><span>{}</span></div>", esc(&w));
        }
        out.push_str("</section>\n");
        if !self.inv.formats.is_empty() {
            out.push_str("<p class=\"formats\">");
            for f in &self.inv.formats {
                let _ = write!(
                    out,
                    "<span class=\"chip\">{} <i>{}</i></span>",
                    format_label(f.format),
                    count(f.count)
                );
            }
            out.push_str("</p>\n");
        }
        if let Some(p) = self.parity {
            let tone = match p.score {
                95.. => "ok",
                75.. => "warn",
                _ => "bad",
            };
            let _ = writeln!(
                out,
                "<p class=\"parity\"><span class=\"dot {tone}\"></span>{}</p>",
                esc(&self.parity_line().unwrap_or_default())
            );
        }

        if self.opts.apps && !self.inv.apps.is_empty() {
            out.push_str("<h2>Vendor apps and licensing tools</h2>\n<ul class=\"rows\">\n");
            for a in &self.inv.apps {
                let kind = if a.kind == ItemKind::LicenseTool {
                    "<span class=\"tag\">licensing</span>"
                } else {
                    ""
                };
                let _ = writeln!(
                    out,
                    "<li><span class=\"name\">{}{kind}</span>{}</li>",
                    esc(&a.name),
                    self.html_version(&a.version)
                );
            }
            out.push_str("</ul>\n");
        }

        if self.opts.plugins && self.inv.products > 0 {
            out.push_str("<h2>Plug-ins</h2>\n");
            for v in &self.inv.vendors {
                let _ = writeln!(
                    out,
                    "<details>\n<summary>{} <i>{}</i></summary>\n<ul class=\"rows\">",
                    esc(vendor_label(&v.vendor)),
                    v.products.len()
                );
                for p in &v.products {
                    let _ = writeln!(
                        out,
                        "<li><span class=\"name\">{}</span><span class=\"fmt\">{}</span>{}</li>",
                        esc(&p.name),
                        formats(&p.formats),
                        self.html_version(&p.version)
                    );
                }
                out.push_str("</ul>\n</details>\n");
            }
        }

        let _ = write!(
            out,
            "<footer>{}</footer>\n</main>\n</body>\n</html>\n",
            esc(FOOTER)
        );
        out
    }

    fn html_version(&self, v: &Option<String>) -> String {
        if !self.opts.versions {
            return String::new();
        }
        format!(
            "<span class=\"ver\">{}</span>",
            v.as_deref().map(esc).unwrap_or_default()
        )
    }
}

const FOOTER: &str = "Made with Patch Pilot. Lists names and versions only: no file paths, serial numbers or licence details.";

fn platform(os: Os, arch: Arch) -> &'static str {
    match (os, arch) {
        (Os::Macos, Arch::Aarch64) => "macOS · Apple silicon",
        (Os::Macos, Arch::X86_64) => "macOS · Intel",
        (Os::Macos, Arch::Universal) => "macOS",
        (Os::Windows, Arch::Aarch64) => "Windows · Arm",
        (Os::Windows, _) => "Windows",
    }
}

fn format_label(f: PluginFormat) -> &'static str {
    match f {
        PluginFormat::Vst3 => "VST3",
        PluginFormat::Au => "AU",
        PluginFormat::Clap => "CLAP",
        PluginFormat::Aax => "AAX",
        PluginFormat::Vst2 => "VST2",
        PluginFormat::Jsfx => "JSFX",
    }
}

fn formats(f: &[PluginFormat]) -> String {
    f.iter()
        .map(|f| format_label(*f))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn vendor_label(v: &str) -> &str {
    if v.is_empty() { "Unknown vendor" } else { v }
}

fn count(n: usize) -> String {
    // Thousands separators without a locale: 1,234.
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_owned()
    } else {
        format!("{word}s")
    }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escapes Markdown punctuation so a plug-in called `*Foo_Bar*` stays literal.
fn md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|' | '~'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The pixel-plane logo (docs/brand/mark-mono.svg).
const MARK: &str = "<svg width=\"16\" height=\"16\" viewBox=\"0 0 16 16\" shape-rendering=\"crispEdges\" aria-hidden=\"true\"><path fill=\"currentColor\" d=\"M7 2h2v1h-2zM7 3h2v1h-2zM7 5h2v1h-2zM6 6h4v1h-4zM4 7h8v1h-8zM1 8h14v1h-14zM0 9h1v1h-1zM1 9h6v1h-6zM7 9h2v1h-2zM9 9h6v1h-6zM15 9h1v1h-1zM7 10h2v1h-2zM7 11h2v1h-2zM7 12h2v1h-2zM5 13h6v1h-6zM4 14h1v1h-1zM5 14h2v1h-2zM7 14h2v1h-2zM9 14h2v1h-2zM11 14h1v1h-1z\"/></svg>";

/// "Mica + Frost" (decision #33), standalone: the app's tokens, light and dark
/// following the reader's system, system fonts, nothing loaded from the web.
const CSS: &str = r#"
:root{color-scheme:light dark;--canvas:#fff;--panel:#f4f4f7;--fg:#1d1d1f;--dim:#6e6e73;--line:#e5e5ea;--sel:#e8e8ed;--accent:#0a64f0;--ok:#1e8e3e;--warn:#a15c00;--bad:#d70015}
@media (prefers-color-scheme:dark){:root{--canvas:#1e1e22;--panel:#26262b;--fg:#f2f2f7;--dim:#a1a1a8;--line:#34343a;--sel:#34343a;--accent:#6aa5ff;--ok:#32d74b;--warn:#ffd60a;--bad:#ff6961}}
*{box-sizing:border-box}
body{margin:0;background:var(--canvas);color:var(--fg);font:13px/1.45 "Segoe UI Variable Text","Segoe UI",system-ui,-apple-system,sans-serif;-webkit-font-smoothing:antialiased}
main{max-width:720px;margin:0 auto;padding:32px 16px 40px}
.brand{display:flex;align-items:center;gap:6px;margin:0 0 14px;color:var(--accent);font-weight:600;font-size:12px}
.brand span{color:var(--dim);font-weight:500}
h1{margin:0;font-size:22px;font-weight:600;letter-spacing:-.01em;overflow-wrap:anywhere}
.sub{margin:2px 0 18px;color:var(--dim)}
.stats{display:flex;flex-wrap:wrap;gap:8px}
.stats div{flex:1 1 120px;background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:10px 12px}
.stats b{display:block;font-size:20px;font-weight:600;font-variant-numeric:tabular-nums}
.stats span{color:var(--dim);font-size:12px}
.formats{display:flex;flex-wrap:wrap;gap:6px;margin:12px 0 0}
.chip{background:var(--sel);border-radius:5px;padding:2px 8px;font-size:12px;font-weight:500}
.chip i{font-style:normal;color:var(--dim);font-variant-numeric:tabular-nums}
.parity{display:flex;align-items:center;gap:8px;margin:12px 0 0;color:var(--dim)}
.dot{width:6px;height:6px;border-radius:50%}.dot.ok{background:var(--ok)}.dot.warn{background:var(--warn)}.dot.bad{background:var(--bad)}
h2{margin:28px 0 4px;font-size:14px;font-weight:600}
summary{display:flex;align-items:center;gap:6px;min-height:28px;border-bottom:1px solid var(--line);font-weight:500;cursor:pointer;list-style:none}
summary::-webkit-details-marker{display:none}
summary::before{content:"";width:5px;height:5px;margin:0 3px 0 1px;border:solid var(--dim);border-width:0 1.5px 1.5px 0;transform:rotate(-45deg);transition:transform .15s}
details[open]>summary::before{transform:rotate(45deg)}
summary:hover{background:var(--sel)}
summary i{font-style:normal;color:var(--dim);font-size:12px;font-variant-numeric:tabular-nums}
details .rows{border-top:0;padding-left:16px}
.rows{list-style:none;margin:0;padding:0;border-top:1px solid var(--line)}
.rows li{display:flex;align-items:center;gap:12px;min-height:28px;padding:4px 0;border-bottom:1px solid var(--line)}
.name{flex:1;min-width:0;overflow-wrap:anywhere}
.tag{margin-left:8px;background:var(--sel);color:var(--dim);border-radius:4px;padding:0 6px;font-size:11px}
.fmt{color:var(--dim);font-size:12px;white-space:nowrap}
.ver{width:72px;text-align:right;color:var(--dim);font:11px ui-monospace,"Cascadia Mono","SF Mono",Menlo,Consolas,monospace;overflow-wrap:anywhere}
footer{margin-top:28px;color:var(--dim);font-size:11px}
@media print{body{background:#fff;color:#000}.stats div{border-color:#ccc}}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Manifest {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/parity/plan/golden.json"
        );
        Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn opts() -> ReportOptions {
        ReportOptions {
            title: "Home <studio>".into(),
            date: "24 October 2026".into(),
            ..ReportOptions::default()
        }
    }

    #[test]
    fn lists_counts_formats_and_plugins() {
        let m = fixture();
        let inv = inventory(&m);
        let r = rig_report(
            &m,
            Some(ReportParity {
                score: 96,
                golden_version: 3,
            }),
            &opts(),
        );
        assert!(
            r.markdown
                .contains(&format!("**{}** plug-ins", inv.products))
        );
        assert!(r.markdown.contains("Formats: VST3"));
        assert!(r.markdown.contains("96% in parity with Golden Rig v3"));
        assert!(r.html.contains("<h1>Home &lt;studio&gt;</h1>"));
        assert!(r.html.contains("24 October 2026"));
        assert!(r.html.contains("dot ok"));
        // Each vendor is a folder that opens on click, without scripts.
        assert!(r.html.contains("<details>\n<summary>"));
        for v in &inv.vendors {
            for p in &v.products {
                assert!(r.html.contains(&esc(&p.name)), "{}", p.name);
                assert!(r.markdown.contains(&md(&p.name)), "{}", p.name);
            }
        }
        for a in &inv.apps {
            assert!(r.html.contains(&esc(&a.name)));
        }
    }

    #[test]
    fn never_includes_paths_ids_or_the_machine_name() {
        let m = fixture();
        let r = rig_report(&m, None, &ReportOptions::default());
        for text in [&r.html, &r.markdown] {
            assert!(!text.contains(&m.machine.id));
            assert!(!text.contains(&m.machine.name), "{}", m.machine.name);
            assert!(!text.contains(":\\"), "a Windows path leaked");
            assert!(!text.contains("/Users/") && !text.contains("/Library/"));
            for item in &m.items {
                if let patchpilot_core::PortablePath::Logical { rel, .. } = &item.path
                    && rel.contains('/')
                {
                    assert!(!text.contains(rel.as_str()), "{rel}");
                }
            }
        }
        assert!(r.markdown.starts_with("## My rig\n"));
        assert!(!r.html.contains("<script"));
        assert!(!r.html.contains("http"), "the page loads nothing");
    }

    #[test]
    fn options_leave_things_out() {
        let m = fixture();
        let parity = Some(ReportParity {
            score: 50,
            golden_version: 1,
        });
        let o = ReportOptions {
            plugins: false,
            apps: false,
            parity: false,
            ..ReportOptions::default()
        };
        let r = rig_report(&m, parity, &o);
        let inv = inventory(&m);
        let first = &inv.vendors[0].products[0].name;
        assert!(!r.markdown.contains(first.as_str()));
        assert!(!r.html.contains("<h2>Plug-ins"));
        assert!(!r.html.contains("Vendor apps"));
        assert!(!r.markdown.contains("parity"));

        let o = ReportOptions {
            versions: false,
            ..ReportOptions::default()
        };
        let r = rig_report(&m, parity, &o);
        assert!(!r.html.contains("class=\"ver\""));
        assert!(r.html.contains("dot bad"));
        let versioned = inv
            .vendors
            .iter()
            .flat_map(|v| &v.products)
            .find_map(|p| p.version.clone())
            .expect("fixture has a versioned plug-in");
        assert!(!r.markdown.contains(&format!("v{versioned}\n")));
    }

    #[test]
    fn escapes_markdown_and_formats_counts() {
        assert_eq!(md("*Foo_Bar* [x]"), "\\*Foo\\_Bar\\* \\[x\\]");
        assert_eq!(count(7), "7");
        assert_eq!(count(1234), "1,234");
        assert_eq!(count(1234567), "1,234,567");
        assert_eq!(plural(1, "vendor"), "vendor");
        assert_eq!(plural(2, "vendor"), "vendors");
    }

    #[test]
    fn options_deserialize_with_defaults() {
        let o: ReportOptions =
            serde_json::from_str(r#"{"title":"Studio","versions":false}"#).unwrap();
        assert_eq!(o.title, "Studio");
        assert!(!o.versions && o.plugins && o.apps && o.parity);
    }
}
