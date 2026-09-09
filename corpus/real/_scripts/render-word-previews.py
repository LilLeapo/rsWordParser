"""Render PDFs exported by desktop Word for visual review; never edit DOCX files."""
from pathlib import Path
import argparse
import pypdfium2 as pdfium
from PIL import Image, ImageChops, ImageDraw, ImageFont

parser = argparse.ArgumentParser()
parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
parser.add_argument("--domain", default="")
args = parser.parse_args()
preview_root = args.root / "_previews"
png_root = preview_root / "rendered"
png_root.mkdir(parents=True, exist_ok=True)
font_path = Path("C:/Windows/Fonts/consola.ttf")
font = ImageFont.truetype(str(font_path), 20)
groups = {}
for pdf_path in sorted(preview_root.rglob("*.pdf")):
    domain = pdf_path.relative_to(preview_root).parts[0]
    if args.domain and domain != args.domain:
        continue
    document = pdfium.PdfDocument(pdf_path)
    for index, page in enumerate(document):
        bitmap = page.render(scale=1.6).to_pil().convert("RGB")
        bbox = ImageChops.difference(bitmap, Image.new("RGB", bitmap.size, "white")).getbbox()
        if bbox:
            left, top, right, bottom = bbox
            bitmap = bitmap.crop((max(0, left - 16), max(0, top - 16), min(bitmap.width, right + 16), min(bitmap.height, bottom + 16)))
        name = f"{domain}-{pdf_path.stem}-p{index + 1}"
        output = png_root / f"{name}.png"
        bitmap.save(output)
        groups.setdefault(domain, []).append((name, output))
    document.close()
for domain, items in groups.items():
    for start in range(0, len(items), 6):
        batch = items[start:start + 6]
        sheet = Image.new("RGB", (1640, 2100), "#e8e8e8")
        draw = ImageDraw.Draw(sheet)
        for position, (name, output) in enumerate(batch):
            x = (position % 2) * 820
            y = (position // 2) * 700
            draw.rectangle((x + 8, y + 8, x + 812, y + 692), fill="white")
            draw.text((x + 20, y + 15), name, font=font, fill="black")
            bitmap = Image.open(output)
            bitmap.thumbnail((780, 638), Image.Resampling.LANCZOS)
            sheet.paste(bitmap, (x + (820 - bitmap.width) // 2, y + 50))
        target = png_root / f"contact-{domain}-{start // 6 + 1}.jpg"
        sheet.save(target, quality=95)
        print(target)
