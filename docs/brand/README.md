# Patch Pilot brand

The logo is a 16×16 pixel-art airplane seen from above, in the app's Frost colours (decision #34).

| File | Use |
|---|---|
| `mark-dark.svg` | The plane for dark backgrounds (in-app brand, 14–16 px nav and up) |
| `mark-light.svg` | The plane for light backgrounds |
| `mark-mono.svg` | One colour (`currentColor`), for places that tint the icon |
| `app-icon.svg`, `app-icon-1024.png` | App icon on the dark Frost tile |
| `app-icon-light.svg`, `app-icon-light-1024.png` | Light-tile variant, for light-mode marketing or a future light app icon |
| `../../app/src-tauri/icons/tray-template*.png` | macOS menu bar icon (black template image; set `icon_as_template(true)`) |
| `../../app/src-tauri/icons/tray-white*.png` | Windows tray icon on the dark taskbar |

All of these, and the app icons in `app/src-tauri/icons`, come from `make-icons.py` (needs Pillow). Edit the sprite there and rerun it; don't use `tauri icon`, which resamples one PNG and blurs the pixels.

## Colours

| | Dark | Light |
|---|---|---|
| Plane | `#F2F2F7` | `#1D1D1F` |
| Wing shade | `#A1A1A8` | `#6E6E73` |
| Nose, wing and tail tips | `#3D8BFF` | `#0A64F0` |
| Cockpit | `#7DD3FC` | `#5AC8FA` |
| Tile | `#26262C` | `#F6F6F9` |

## Wordmark

"Patch Pilot" set in Inter SemiBold (600), tracking about -0.025em, next to the mark. Tagline: "Keep every rig production-ready".

## Rules

- Scale the mark by whole numbers only (16, 32, 48 px, ...), with crisp edges (`shape-rendering="crispEdges"`, `image-rendering: pixelated`), so pixels never blur.
- Use the dark mark on dark surfaces and the light mark on light ones; the menu bar and tray use the one-colour version.
