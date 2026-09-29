#!/usr/bin/env python3
"""Generate the original geometric application icon using Pillow + macOS iconutil."""
from pathlib import Path
import subprocess
import tempfile
from PIL import Image, ImageDraw

root = Path(__file__).resolve().parents[1]
icons = root / "src-tauri" / "icons"
icons.mkdir(parents=True, exist_ok=True)
image = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
draw = ImageDraw.Draw(image)
draw.rounded_rectangle((72, 72, 952, 952), radius=176, fill="#F6F5ED")
draw.rectangle((215, 235, 829, 813), fill="#20201D")
draw.rectangle((195, 215, 809, 793), fill="#5548E8", outline="#20201D", width=22)
draw.rectangle((237, 257, 767, 327), fill="#E6E2FF")
draw.rectangle((237, 367, 467, 537), fill="#FFFEF8")
draw.rectangle((507, 367, 767, 707), fill="#E6E2FF")
draw.rectangle((237, 577, 467, 707), fill="#FFFEF8")
draw.rectangle((541, 403, 579, 441), fill="#159B60")
draw.rectangle((541, 481, 731, 503), fill="#5548E8")
draw.rectangle((541, 527, 693, 549), fill="#5548E8")
draw.rectangle((541, 573, 717, 595), fill="#5548E8")
for size, name in [(32, "32x32.png"), (128, "128x128.png"), (256, "128x128@2x.png"), (1024, "icon.png")]:
    image.resize((size, size), Image.Resampling.LANCZOS).save(icons / name)
with tempfile.TemporaryDirectory(prefix="pixel-workspace-icon-") as folder:
    iconset = Path(folder) / "icon.iconset"
    iconset.mkdir()
    for size in [16, 32, 128, 256, 512]:
        for scale in [1, 2]:
            suffix = "@2x" if scale == 2 else ""
            name = f"icon_{size}x{size}{suffix}.png"
            image.resize((size * scale, size * scale), Image.Resampling.LANCZOS).save(iconset / name)
    subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(icons / "icon.icns")], check=True)
print(f"Generated icons in {icons}")
