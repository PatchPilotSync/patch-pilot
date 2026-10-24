Reaper template fixtures (#10). `REAPER/` stands in for the resource folder
(`%APPDATA%\REAPER` or `~/Library/Application Support/REAPER`). Plug-in state
lines are fake base64.

Covers VST3 (class ID in braces), VST3 instruments (`VST3i:`), VST2 (file
name key), AU instruments, CLAP (with a nested `<STATE` block), JSFX with and
without quotes, `'`-quoted tokens, a name with brackets inside
(`Acme EQ (Mono) (Acme Audio)`), a non-plug-in block (`<VIDEO_EFFECT`), a
plug-in used twice in one project, nested template folders, and a stray
non-template file.

Also a script (`Scripts/Tools/Nudge.lua`) and a theme (`ColorThemes/`) that
Bring over tracks (#42), next to things it must skip under `Scripts/`: a
compiled `helper.dll`, a `.git` folder and `__pycache__`.
