#!/usr/bin/env python3
"""Render one full source PDF page for local review, without changing source."""
import json
import sys
import pymupdf as fitz
from pathlib import Path
from inspect_pdf import open_pdf, IntakeError

RENDER_VERSION = "pymupdf-region-v1"

def render_region(page, bbox):
    """Canonical unrotated region PNG; caller validates bbox. No source writes."""
    box = fitz.Rect(bbox)
    if box.is_empty or box.is_infinite:
        raise IntakeError("Invalid render region")
    scale = min(2.0, 2400 / max(box.width, box.height))
    original = page.rotation
    try:
        page.set_rotation(0)
        return page.get_pixmap(matrix=fitz.Matrix(scale, scale), clip=box,
                               colorspace=fitz.csRGB, alpha=False, annots=False).tobytes("png")
    finally:
        page.set_rotation(original)

def main():
    doc = None
    try:
        if len(sys.argv) != 4:
            raise IntakeError("Usage: render_pdf_page.py file.pdf page-number output.png")
        doc = open_pdf(Path(sys.argv[1]))
        page_num = int(sys.argv[2])
        if not 1 <= page_num <= len(doc):
            raise IntakeError("Page number is outside PDF")
        page = doc[page_num - 1]
        # Bound output raster size, including hostile enormous page dimensions.
        longest = max(page.rect.width, page.rect.height)
        if longest <= 0:
            raise IntakeError("Invalid PDF page size")
        scale = min(1.5, 1800 / longest)
        pix = page.get_pixmap(matrix=fitz.Matrix(scale, scale), alpha=False)
        dest = Path(sys.argv[3])
        if dest.suffix.lower() != ".png":
            raise IntakeError("Output must be PNG")
        pix.save(dest)
        print(json.dumps({"page": page_num, "width": pix.width, "height": pix.height, "output": str(dest)}))
        return 0
    except Exception as exc:
        print(json.dumps({"error": str(exc) if isinstance(exc, (IntakeError, ValueError)) else "Page rendering failed"}))
        return 2
    finally:
        if doc is not None:
            doc.close()

if __name__ == "__main__":
    raise SystemExit(main())
