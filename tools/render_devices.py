"""Render generic top-down keyboard and mouse images for RazerLite.

Our own drawings (no product photos), so they can ship in the repo and the exe.
The keyboard uses the app's key layout (tools/layout.json, written by
`cargo test dump_layout -- --ignored`) at the same origin and unit as
`ui/images.rs` (KB_ORIGIN, KB_UNIT), so key hit areas line up.

    python tools/render_devices.py
"""

import colorsys
import json
import math
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "generic"
SS = 4  # supersampling

W, H = 750, 420
ORIGIN = (46.5, 115.0)
UNIT = 26.36


def font(px):
    for name in ("segoeuisb.ttf", "segoeui.ttf", "arial.ttf"):
        try:
            return ImageFont.truetype(name, px)
        except OSError:
            pass
    return ImageFont.load_default()


def vgrad(size, top, bottom):
    """Vertical gradient image (RGBA)."""
    w, h = size
    g = Image.new("RGBA", (1, h))
    for y in range(h):
        t = y / max(h - 1, 1)
        g.putpixel((0, y), tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(4)))
    return g.resize((w, h))


def rounded_mask(size, box, r):
    m = Image.new("L", size, 0)
    ImageDraw.Draw(m).rounded_rectangle(box, r, fill=255)
    return m


def keyboard():
    lay = json.loads((ROOT / "tools" / "layout.json").read_text())
    sw, sh = W * SS, H * SS
    u = UNIT * SS
    ox, oy = ORIGIN[0] * SS, ORIGIN[1] * SS
    kw, kh = lay["size"]

    img = Image.new("RGBA", (sw, sh), (0, 0, 0, 0))

    # Case: dark aluminium plate with a soft drop shadow.
    pad_x, pad_top, pad_bot = 0.62 * u, 1.55 * u, 0.62 * u
    case = (ox - pad_x, oy - pad_top, ox + kw * u + pad_x, oy + kh * u + pad_bot)
    shadow = Image.new("RGBA", (sw, sh), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle(
        (case[0] + 4 * SS, case[1] + 10 * SS, case[2] - 4 * SS, case[3] + 12 * SS), 22 * SS, fill=(0, 0, 0, 150))
    img = Image.alpha_composite(img, shadow.filter(ImageFilter.GaussianBlur(9 * SS)))

    cm = rounded_mask((sw, sh), case, 18 * SS)
    body = vgrad((sw, sh), (52, 54, 58, 255), (26, 27, 30, 255))
    img.paste(body, (0, 0), cm)
    d = ImageDraw.Draw(img)
    d.rounded_rectangle(case, 18 * SS, outline=(90, 93, 99, 255), width=2 * SS)
    inner = (case[0] + 2 * SS, case[1] + 2 * SS, case[2] - 2 * SS, case[3] - 2 * SS)
    d.rounded_rectangle(inner, 16 * SS, outline=(14, 15, 17, 255), width=1 * SS)

    # Recessed key well.
    well = (ox - 0.18 * u, oy - 0.18 * u, ox + kw * u + 0.18 * u, oy + kh * u + 0.18 * u)
    d.rounded_rectangle(well, 6 * SS, fill=(12, 12, 14, 255))

    # Rainbow underglow: saturated light around every key (the app tints this layer).
    glow = Image.new("RGBA", (sw, sh), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    for _id, _l, x, y, w, h in lay["keys"]:
        hue = (x + w / 2) / kw * 0.85
        r, g, b = (int(c * 255) for c in colorsys.hsv_to_rgb(hue, 0.95, 1.0))
        x0, y0 = ox + x * u, oy + y * u
        gd.rounded_rectangle((x0 - 0.06 * u, y0 - 0.06 * u, x0 + w * u + 0.06 * u, y0 + h * u + 0.06 * u), 5 * SS, fill=(r, g, b, 255))
    glow = glow.filter(ImageFilter.GaussianBlur(2.2 * SS))
    img = Image.alpha_composite(img, glow)
    d = ImageDraw.Draw(img)

    # Keycaps: dark skirt, lighter dished top, legend.
    f_small, f_big = font(int(0.27 * u)), font(int(0.34 * u))
    for _id, label, x, y, w, h in lay["keys"]:
        x0, y0, x1, y1 = ox + x * u, oy + y * u, ox + (x + w) * u, oy + (y + h) * u
        d.rounded_rectangle((x0, y0, x1, y1), 4 * SS, fill=(30, 31, 34, 255), outline=(8, 8, 9, 255), width=SS)
        t = 0.11 * u
        top = (x0 + t, y0 + t * 0.55, x1 - t, y1 - t * 1.45)
        cap = vgrad((int(top[2] - top[0]), int(top[3] - top[1])), (60, 62, 67, 255), (40, 41, 45, 255))
        img.paste(cap, (int(top[0]), int(top[1])), rounded_mask(cap.size, (0, 0, cap.size[0] - 1, cap.size[1] - 1), 3 * SS))
        d.line((top[0] + 3 * SS, top[1], top[2] - 3 * SS, top[1]), fill=(88, 90, 96, 255), width=SS)
        if label:
            txt = label.replace("\n", " ")
            f = f_big if len(txt) == 1 else f_small
            px = f.size
            while d.textlength(txt, font=f) > (top[2] - top[0]) - 5 * SS and px > 0.18 * u:
                px -= SS
                f = font(px)
            d.text((top[0] + 3 * SS, top[1] + 1.5 * SS), txt, font=f, fill=(206, 208, 214, 255))

    # Top-right: media keys and a volume dial.
    cx = case[2] - 0.95 * u
    cy = case[1] + pad_top * 0.5
    r = 0.42 * u
    d.ellipse((cx - r - SS, cy - r - SS, cx + r + SS, cy + r + SS), fill=(10, 10, 11, 255))
    dial = vgrad((int(2 * r), int(2 * r)), (110, 113, 120, 255), (48, 50, 54, 255))
    dm = Image.new("L", dial.size, 0)
    ImageDraw.Draw(dm).ellipse((0, 0, dial.size[0] - 1, dial.size[1] - 1), fill=255)
    img.paste(dial, (int(cx - r), int(cy - r)), dm)
    for i in range(36):
        a = i / 36 * math.tau
        d.line((cx + math.cos(a) * r * 0.82, cy + math.sin(a) * r * 0.82, cx + math.cos(a) * r * 0.97, cy + math.sin(a) * r * 0.97), fill=(60, 62, 66, 255), width=SS)
    for i in range(3):
        bx = cx - (1.0 + i * 0.62) * u
        d.rounded_rectangle((bx - 0.24 * u, cy - 0.16 * u, bx + 0.24 * u, cy + 0.16 * u), 3 * SS, fill=(36, 37, 41, 255), outline=(10, 10, 11, 255), width=SS)

    # Status LEDs.
    for i in range(3):
        lx = case[2] - (4.1 - i * 0.32) * u
        d.ellipse((lx - 2 * SS, cy - 2 * SS, lx + 2 * SS, cy + 2 * SS), fill=(70, 72, 76, 255))

    img = img.resize((W, H), Image.LANCZOS)
    img.save(OUT / "keyboard.png", optimize=True)


def mouse():
    w, h = 300, 510
    sw, sh = w * SS, h * SS
    s = sw / 100.0  # outline is defined in a 100×170 box, like art::mouse_body

    def m(x, y):
        return (x * s, y * s)

    def cubic(p0, p1, p2, p3, out, n=24):
        for i in range(1, n + 1):
            t = i / n
            v = 1 - t
            out.append(tuple(v ** 3 * p0[k] + 3 * v * v * t * p1[k] + 3 * v * t * t * p2[k] + t ** 3 * p3[k] for k in range(2)))

    pts = [m(50, 4)]
    cubic(m(50, 4), m(24, 4), m(13, 28), m(13, 60), pts)
    pts.append(m(15, 122))
    cubic(m(15, 122), m(17, 152), m(33, 166), m(50, 166), pts)
    cubic(m(50, 166), m(67, 166), m(83, 152), m(85, 122), pts)
    pts.append(m(87, 60))
    cubic(m(87, 60), m(87, 28), m(76, 4), m(50, 4), pts)

    mask = Image.new("L", (sw, sh), 0)
    ImageDraw.Draw(mask).polygon(pts, fill=255)

    img = Image.new("RGBA", (sw, sh), (0, 0, 0, 0))
    # Body: dark matte with a radial highlight toward the back of the shell.
    body = Image.new("RGBA", (sw, sh), (22, 23, 26, 255))
    hl = Image.new("L", (sw, sh), 0)
    ImageDraw.Draw(hl).ellipse((28 * s, 70 * s, 72 * s, 150 * s), fill=255)
    hl = hl.filter(ImageFilter.GaussianBlur(14 * s))
    body = Image.composite(Image.new("RGBA", (sw, sh), (62, 64, 70, 255)), body, ImageChops.multiply(hl, Image.new("L", (sw, sh), 170)))
    img.paste(body, (0, 0), mask)

    # Rim light and edge shading.
    edge = mask.filter(ImageFilter.GaussianBlur(3 * s))
    rim = ImageChops.subtract(mask, edge)
    img = Image.composite(Image.new("RGBA", (sw, sh), (8, 8, 9, 255)), img, rim.point(lambda v: min(255, v * 3)))
    d = ImageDraw.Draw(img)
    d.line(pts + [pts[0]], fill=(96, 99, 106, 255), width=int(0.9 * s))

    # Button split, seam and wheel.
    d.line((m(50, 5), m(50, 64)), fill=(6, 6, 7, 255), width=int(1.1 * s))
    seam = []
    for i in range(25):
        t = i / 24
        seam.append(m(14 + 72 * t, 62 + 12 * (1 - (2 * t - 1) ** 2)))
    d.line(seam, fill=(6, 6, 7, 255), width=int(1.0 * s))
    d.rounded_rectangle((*m(44, 18), *m(56, 44)), int(6 * s), fill=(8, 8, 9, 255))
    wheel = vgrad((int(8 * s), int(22 * s)), (120, 123, 130, 255), (40, 42, 46, 255))
    img.paste(wheel, (int(46 * s), int(20 * s)), rounded_mask(wheel.size, (0, 0, wheel.size[0] - 1, wheel.size[1] - 1), int(4 * s)))
    for i in range(10):
        y = (21.5 + i * 2.0) * s
        d.line((46.5 * s, y, 53.5 * s, y), fill=(30, 31, 34, 255), width=max(1, int(0.4 * s)))
    # Side buttons on the left flank.
    for a, b in ((70, 88), (92, 110)):
        d.line((m(14.2, a), m(14.2, b)), fill=(120, 123, 130, 255), width=int(1.6 * s))
    # Small logo dot in the palm area.
    d.ellipse((*m(47.5, 128), *m(52.5, 133)), fill=(70, 72, 78, 255))

    img = img.resize((w, h), Image.LANCZOS)
    img.save(OUT / "mouse.png", optimize=True)


if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    keyboard()
    mouse()
    print("wrote", OUT / "keyboard.png", OUT / "mouse.png")
