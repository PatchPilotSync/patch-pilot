"""Regenerate Patch Pilot's logo files and app icons from the 16x16 sprite below.

    pip install pillow
    python docs/brand/make-icons.py

Pixel art must not be resampled: every size is drawn from the sprite at an
integer scale, so don't regenerate these with `tauri icon` (it resizes one PNG).
"""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
BRAND = ROOT / "docs" / "brand"
ICONS = ROOT / "app" / "src-tauri" / "icons"

# Airplane seen from above, nose up. G = tips, W = body, L = cockpit, w = wing shade.
SPRITE = """
................
.......GG.......
.......WW.......
.......LL.......
.......WW.......
......WWWW......
....WWWWWWWW....
.WWWWWWWWWWWWWW.
GwwwwwwWWwwwwwwG
.......WW.......
.......WW.......
.......WW.......
.....WWWWWW.....
....GwwWWwwG....
................
................
""".split()

# Frost palettes (see docs/decisions.md, logo decision).
DARK = {"tile": "#26262C", "G": "#3D8BFF", "W": "#F2F2F7", "L": "#7DD3FC", "w": "#A1A1A8"}
LIGHT = {"tile": "#F6F6F9", "G": "#0A64F0", "W": "#1D1D1F", "L": "#5AC8FA", "w": "#6E6E73"}

# The sprite's drawn rows are 1-13; shift down one so it sits centred.
DY = 1


def runs():
    """Yield (x, y, length, colour key) for each horizontal run of pixels."""
    for y, row in enumerate(SPRITE):
        x = 0
        while x < 16:
            c = row[x]
            if c == ".":
                x += 1
                continue
            start = x
            while x < 16 and row[x] == c:
                x += 1
            yield start, y + DY, x - start, c


def svg_paths(palette):
    by_fill = {}
    for x, y, n, c in runs():
        fill = palette[c] if palette else "currentColor"
        if fill is None:
            continue
        by_fill.setdefault(fill, []).append(f"M{x} {y}h{n}v1h-{n}z")
    return "".join(f'<path d="{"".join(d)}" fill="{f}"/>' for f, d in by_fill.items())


def write_svgs():
    head = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}" shape-rendering="crispEdges">'
    files = {
        "mark-dark.svg": head.format(vb="0 0 16 16") + svg_paths(DARK) + "</svg>\n",
        "mark-light.svg": head.format(vb="0 0 16 16") + svg_paths(LIGHT) + "</svg>\n",
        "mark-mono.svg": head.format(vb="0 0 16 16")
        + svg_paths({"G": "currentColor", "W": "currentColor", "L": None, "w": "currentColor"})
        + "</svg>\n",
    }
    for name, pal in (("app-icon.svg", DARK), ("app-icon-light.svg", LIGHT)):
        files[name] = (
            '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20">'
            f'<rect width="20" height="20" rx="4.5" fill="{pal["tile"]}"/>'
            f'<g transform="translate(2 2)" shape-rendering="crispEdges">{svg_paths(pal)}</g></svg>\n'
        )
    for name, text in files.items():
        (BRAND / name).write_text(text)


def sprite_scale(size):
    # Small icons fill the tile so the plane stays legible in taskbars and docks.
    if size <= 32:
        return max(1, size // 16)
    return round(size * 0.75 / 16)


def draw_sprite(img, scale, ox, oy, palette):
    d = ImageDraw.Draw(img)
    for x, y, n, c in runs():
        colour = palette[c]
        if colour is None:
            continue
        d.rectangle(
            [ox + x * scale, oy + y * scale, ox + (x + n) * scale - 1, oy + (y + 1) * scale - 1],
            fill=colour,
        )


def app_icon(size, palette=DARK):
    # Rounded tile, antialiased by drawing the mask at 4x and scaling it down.
    big = Image.new("L", (size * 4, size * 4), 0)
    ImageDraw.Draw(big).rounded_rectangle(
        [0, 0, size * 4 - 1, size * 4 - 1], radius=round(size * 4 * 0.225), fill=255
    )
    mask = big.resize((size, size), Image.LANCZOS)
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    img.paste(Image.new("RGBA", (size, size), palette["tile"]), (0, 0), mask)
    scale = sprite_scale(size)
    off = (size - 16 * scale) // 2
    draw_sprite(img, scale, off, off, palette)
    return img


def tray_icon(size, colour):
    """Single-colour silhouette. Black is a macOS template image; white suits the Windows tray."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    pal = {"G": colour, "W": colour, "w": colour, "L": None}
    draw_sprite(img, size // 16, 0, 0, pal)
    return img


def main():
    write_svgs()
    app_icon(1024).save(BRAND / "app-icon-1024.png")
    app_icon(1024, LIGHT).save(BRAND / "app-icon-light-1024.png")

    app_icon(32).save(ICONS / "32x32.png")
    app_icon(128).save(ICONS / "128x128.png")
    app_icon(256).save(ICONS / "128x128@2x.png")
    app_icon(512).save(ICONS / "icon.png")

    ico_sizes = [16, 24, 32, 48, 64, 256]
    ico = [app_icon(s) for s in ico_sizes]
    ico[-1].save(ICONS / "icon.ico", sizes=[(s, s) for s in ico_sizes], append_images=ico[:-1])

    icns_sizes = [16, 32, 64, 128, 256, 512, 1024]
    icns = [app_icon(s) for s in icns_sizes]
    icns[-1].save(ICONS / "icon.icns", append_images=icns[:-1])

    tray_icon(16, "#000000").save(ICONS / "tray-template.png")
    tray_icon(32, "#000000").save(ICONS / "tray-template@2x.png")
    tray_icon(16, "#FFFFFF").save(ICONS / "tray-white.png")
    tray_icon(32, "#FFFFFF").save(ICONS / "tray-white@2x.png")


if __name__ == "__main__":
    main()
