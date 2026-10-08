"""Renders assets/icon.png (1024x1024, macOS Big Sur icon grid). Requires Pillow."""
import math
import random
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

K = 2  # supersampling
S = 1024 * K
BODY = (100 * K, 100 * K, 924 * K, 924 * K)  # squircle body on the macOS icon grid
rng = random.Random(7)


def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def vertical_gradient(w, h, top, bottom):
    img = Image.new("RGBA", (w, h))
    px = img.load()
    for y in range(h):
        c = lerp(top, bottom, y / (h - 1))
        for x in range(w):
            px[x, y] = c
    return img


def diagonal_gradient(w, h, a, b):
    small = Image.new("RGBA", (64, 64))
    px = small.load()
    for y in range(64):
        for x in range(64):
            px[x, y] = lerp(a, b, (x + y) / 126)
    return small.resize((w, h), Image.BILINEAR)


def rounded_mask(size, box, radius):
    m = Image.new("L", size, 0)
    ImageDraw.Draw(m).rounded_rectangle(box, radius, fill=255)
    return m


def photo(w, h):
    img = vertical_gradient(w, h, (255, 179, 64, 255), (255, 70, 110, 255))
    d = ImageDraw.Draw(img)
    sx, sy, r = w * 0.34, h * 0.44, w * 0.12
    d.ellipse((sx - r, sy - r, sx + r, sy + r), fill=(255, 236, 140, 255))
    d.polygon([(0, h * 0.78), (w * 0.22, h * 0.52), (w * 0.42, h * 0.72), (w * 0.62, h * 0.46),
               (w, h * 0.74), (w, h), (0, h)], fill=(88, 40, 120, 255))
    d.polygon([(0, h * 0.9), (w * 0.3, h * 0.7), (w * 0.55, h * 0.88), (w * 0.8, h * 0.66),
               (w, h * 0.82), (w, h), (0, h)], fill=(44, 22, 80, 255))
    img.putalpha(rounded_mask((w, h), (0, 0, w - 1, h - 1), 34 * K))
    return img


def disintegrate(src):
    """Same idea as the app: cells past a ragged edge break off and drift up-right."""
    w, h = src.size
    cell = 26 * K
    out = Image.new("RGBA", (S, S))
    flying = []
    rows = math.ceil(h / cell)
    edge = [0.42 + 0.18 * rng.random() for _ in range(rows)]
    for j in range(rows):
        for i in range(math.ceil(w / cell)):
            x, y = i * cell, j * cell
            piece = src.crop((x, y, min(x + cell, w), min(y + cell, h)))
            u = (x + cell / 2) / w
            t = (u - edge[j]) / (1.0 - edge[j])
            if t > 0 and rng.random() < 0.25 + 0.75 * t:
                travel = t * (0.5 + 0.5 * rng.random())
                dx = (30 + 340 * travel) * K
                dy = -(dx * 0.45 + rng.uniform(-40, 70) * K * travel)
                scale = 1.0 - 0.45 * travel
                size = max(4, round(cell * scale))
                piece = piece.resize((size, size), Image.NEAREST)
                alpha = piece.getchannel("A").point(lambda a: round(a * (1.0 - 0.75 * travel)))
                piece.putalpha(alpha)
                flying.append((travel, (round(x + dx), round(y + dy)), piece))
            else:
                out.alpha_composite(piece, (x, y))
    return out, flying


def main():
    body_w = BODY[2] - BODY[0]
    icon = Image.new("RGBA", (S, S), (0, 0, 0, 0))

    # Drop shadow under the squircle.
    shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle((BODY[0], BODY[1] + 14 * K, BODY[2], BODY[3] + 14 * K),
                                             185 * K, fill=(0, 0, 0, 110))
    icon.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(22 * K)))

    body = diagonal_gradient(S, S, (34, 30, 62, 255), (12, 10, 24, 255))

    pw, ph = 520 * K, 410 * K
    px0 = BODY[0] + (body_w - pw) // 2 - 70 * K
    py0 = BODY[1] + (body_w - ph) // 2 + 60 * K
    src = photo(pw, ph)

    intact, flying = disintegrate(src)

    # Soft shadow below what is left of the photo.
    ps = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ps.paste(Image.new("RGBA", (S, S), (0, 0, 0, 120)), (px0, py0 + 16 * K), intact.getchannel("A"))
    body.alpha_composite(ps.filter(ImageFilter.GaussianBlur(20 * K)))
    layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    layer.alpha_composite(intact, (px0, py0))
    for _, (x, y), piece in sorted(flying, key=lambda f: f[0]):
        layer.alpha_composite(piece, (px0 + x, py0 + y))
    body.alpha_composite(layer)

    # Subtle top highlight.
    hl = vertical_gradient(S, S // 2, (255, 255, 255, 28), (255, 255, 255, 0))
    body.alpha_composite(hl, (0, BODY[1]))

    mask = rounded_mask((S, S), BODY, 185 * K)
    icon.alpha_composite(Image.composite(body, Image.new("RGBA", (S, S)), mask))

    # Hairline edge.
    edge = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(edge).rounded_rectangle(BODY, 185 * K, outline=(255, 255, 255, 40), width=2 * K)
    icon.alpha_composite(edge)

    out = Path(__file__).with_name("icon.png")
    icon.resize((1024, 1024), Image.LANCZOS).save(out)
    print(out)


if __name__ == "__main__":
    main()
