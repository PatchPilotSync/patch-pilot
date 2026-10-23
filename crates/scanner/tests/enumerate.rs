use std::path::PathBuf;

use patchpilot_core::PluginFormat::{self, *};
use patchpilot_scanner::{PluginRoot, enumerate};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scanner/enumerate")
        .join(rel)
}

// Format as its Debug name so rows sort without needing `Ord` on the core type.
type Row = (String, String, String, bool);

/// (root, format, path relative to the root with `/` separators, is_bundle).
fn found(roots: &[PluginRoot]) -> Vec<Row> {
    let result = enumerate(roots);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    let mut out: Vec<_> = result
        .candidates
        .into_iter()
        .map(|c| {
            let root = roots.iter().find(|r| r.name == c.root).unwrap();
            let rel = c.path.strip_prefix(&root.path).unwrap();
            let rel = rel
                .iter()
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            (c.root, format!("{:?}", c.format), rel, c.is_bundle)
        })
        .collect();
    out.sort();
    out
}

fn row(root: &str, format: PluginFormat, rel: &str, bundle: bool) -> Row {
    (root.into(), format!("{format:?}"), rel.into(), bundle)
}

#[test]
fn windows_tree() {
    let roots = [
        PluginRoot::new("VST3_SYSTEM", Vst3, fixture("windows/VST3")),
        PluginRoot::new("CLAP_SYSTEM", Clap, fixture("windows/CLAP")),
        PluginRoot::new("AAX_SYSTEM", Aax, fixture("windows/AAX")),
        PluginRoot::new("VST2_PROGRAM_FILES", Vst2, fixture("windows/VSTPlugins")),
    ];
    let mut expected = vec![
        row("AAX_SYSTEM", Aax, "Acme Verb.aaxplugin", true),
        row("CLAP_SYSTEM", Clap, "Acme Synth.clap", false),
        row("VST2_PROGRAM_FILES", Vst2, "Acme Comp x64.dll", false),
        row("VST2_PROGRAM_FILES", Vst2, "Acme/Acme Gate.dll", false),
        // A .vst3 in a VST2 folder is still VST3.
        row("VST2_PROGRAM_FILES", Vst3, "Acme/Acme Verb.vst3", false),
        row("VST3_SYSTEM", Vst3, "Acme Verb.vst3", true),
        row(
            "VST3_SYSTEM",
            Vst3,
            "Vendor Folder/Legacy Delay.vst3",
            false,
        ),
    ];
    expected.sort();
    assert_eq!(found(&roots), expected);
}

#[test]
fn macos_tree() {
    let roots = [
        PluginRoot::new("AU_SYSTEM", Au, fixture("macos/Components")),
        PluginRoot::new("VST3_SYSTEM", Vst3, fixture("macos/VST3")),
        PluginRoot::new("VST2_SYSTEM", Vst2, fixture("macos/VST")),
        PluginRoot::new("CLAP_SYSTEM", Clap, fixture("macos/CLAP")),
        PluginRoot::new("AAX_SYSTEM", Aax, fixture("macos/Avid")),
    ];
    let mut expected = vec![
        row("AAX_SYSTEM", Aax, "Acme Verb.aaxplugin", true),
        row("AU_SYSTEM", Au, "Acme Verb.component", true),
        row("CLAP_SYSTEM", Clap, "Acme Synth.clap", true),
        row("VST2_SYSTEM", Vst2, "Acme Comp.vst", true),
        // Classification is by format, not OS: a stray .dll in a VST2 root is
        // listed, and the metadata reader decides whether it's usable here.
        row("VST2_SYSTEM", Vst2, "Acme Gate.dll", false),
        row("VST3_SYSTEM", Vst3, "Acme Verb.vst3", true),
    ];
    expected.sort();
    assert_eq!(found(&roots), expected);
}

#[test]
fn missing_roots_are_skipped() {
    let roots = [PluginRoot::new(
        "VST3_SYSTEM",
        Vst3,
        fixture("does-not-exist"),
    )];
    let result = enumerate(&roots);
    assert!(result.candidates.is_empty() && result.warnings.is_empty());
}

#[cfg(unix)]
#[test]
fn symlink_loops_terminate_and_duplicates_collapse() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().unwrap();
    let vst3 = tmp.path().join("VST3");
    std::fs::create_dir_all(vst3.join("Acme/Acme Verb.vst3/Contents")).unwrap();
    // Loop: VST3/Acme/loop -> VST3
    symlink(&vst3, vst3.join("Acme/loop")).unwrap();
    // A second root that links to the same bundle.
    let extra = tmp.path().join("Extra");
    std::fs::create_dir_all(&extra).unwrap();
    symlink(
        vst3.join("Acme/Acme Verb.vst3"),
        extra.join("Linked Verb.vst3"),
    )
    .unwrap();

    let roots = [
        PluginRoot::new("VST3_SYSTEM", Vst3, &vst3),
        PluginRoot::new("EXTRA", Vst3, &extra),
    ];
    let result = enumerate(&roots);
    assert_eq!(result.candidates.len(), 1, "{:?}", result.candidates);
    assert!(
        result.warnings.iter().any(|w| w.message.contains("loop")),
        "{:?}",
        result.warnings
    );
}
