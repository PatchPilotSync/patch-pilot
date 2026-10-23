Fake plug-in folders for the enumerator (#5). Every file is empty: the walk
only looks at names and folder structure. `windows/` and `macos/` stand in for
the per-OS plug-in roots; tests point `PluginRoot`s at them directly.

Covers: bundles that contain a same-named binary (must count once), the legacy
single-file `.vst3`, CLAP as a file (Windows) and a bundle (macOS), VST2 `.dll`
in a VST2 folder, nested vendor folders, and non-plug-in files. Symlink loops
are built at test time in a temp dir.
