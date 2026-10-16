"""Рисует иконку приложения: скруглённый малиновый квадрат с буквой «b».

Геометрия — та же, что у иконки трея (`crates/bars-app/src/icons.rs`, сетка 32 на 32),
только в большем размере и со сглаживанием. Запуск из корня репозитория:

    python scripts/make-icon.py

Пишет `crates/bars-app/icons/icon.png` (256 px) и `icon.ico` (16–256 px).
"""

from pathlib import Path

from PIL import Image

GRID = 32          # сетка иконки трея
SIZE = 256         # размер результата
SUPER = 4          # сверхдискретизация для гладких краёв
COLOR = (0xD6, 0x33, 0x6C, 255)
WHITE = (255, 255, 255, 255)
OUT = Path(__file__).resolve().parent.parent / "crates" / "bars-app" / "icons"


def inside_rounded_square(x: float, y: float) -> bool:
    radius = 7.0
    cx = min(max(x, radius), GRID - radius)
    cy = min(max(y, radius), GRID - radius)
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius * radius


def inside_letter(x: float, y: float) -> bool:
    stem = 9.0 <= x < 13.0 and 6.0 <= y < 26.0
    d = ((x - 17.0) ** 2 + (y - 19.0) ** 2) ** 0.5
    bowl = 3.2 <= d < 7.2 and x >= 12.0
    return stem or bowl


def draw() -> Image.Image:
    big = SIZE * SUPER
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    px = img.load()
    scale = GRID / big
    for row in range(big):
        for column in range(big):
            x, y = (column + 0.5) * scale, (row + 0.5) * scale
            if not inside_rounded_square(x, y):
                continue
            px[column, row] = WHITE if inside_letter(x, y) else COLOR
    return img.resize((SIZE, SIZE), Image.LANCZOS)


if __name__ == "__main__":
    icon = draw()
    icon.save(OUT / "icon.png")
    icon.save(OUT / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    print("готово:", OUT)
