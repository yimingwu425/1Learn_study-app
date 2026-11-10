#!/usr/bin/env python3
"""Local, review-only PDF intake. Native text is a proposal, never publication.

Usage: python3 tools/inspect_pdf.py /absolute/file.pdf
Success JSON is written to stdout. Invalid/unsafe input returns JSON and exit 2.
All page numbers are 1-based. No source PDF is modified and no network is used.
"""
from __future__ import annotations
import json
import re
import sys
from pathlib import Path
import pymupdf as fitz

MAX_BYTES = 25 * 1024 * 1024
MAX_PAGES = 500
MAX_TEXT_CHARS = 1_000_000
MAX_PAGE_CHARS = 40_000
MAX_CANDIDATES = 2000
HEADING = re.compile(r"^(\d{1,3})\s*(?:[.)]\s+|\s+|(?=\())(.+)$")
MARK = re.compile(r"\[(\d{1,2})\]")

class IntakeError(ValueError):
    pass

def open_pdf(path: Path):
    if not path.is_file():
        raise IntakeError("PDF file does not exist")
    if path.stat().st_size > MAX_BYTES:
        raise IntakeError("PDF exceeds the 25 MiB limit")
    with path.open("rb") as handle:
        if b"%PDF-" not in handle.read(1024):
            raise IntakeError("Input is not a PDF")
    try:
        doc = fitz.open(path)
    except Exception as exc:
        raise IntakeError("Malformed or unreadable PDF") from exc
    if not doc.is_pdf:
        doc.close()
        raise IntakeError("Input is not a PDF")
    if doc.is_encrypted or doc.needs_pass:
        doc.close()
        raise IntakeError("Encrypted PDFs are not accepted; provide an unlocked copy")
    if not 1 <= len(doc) <= MAX_PAGES:
        doc.close()
        raise IntakeError("PDF must contain between 1 and 500 pages")
    return doc

def page_lines(page):
    """Text coordinates from PyMuPDF are unrotated, even for rotated pages."""
    width, height = page.cropbox.width, page.cropbox.height
    data = page.get_text("dict", flags=fitz.TEXTFLAGS_DICT & ~fitz.TEXT_PRESERVE_IMAGES)
    lines = []
    for block in data.get("blocks", []):
        if block.get("type") != 0:
            continue
        for line in block.get("lines", []):
            text = "".join(span.get("text", "") for span in line.get("spans", [])).strip()
            if text:
                x0, y0, x1, y1 = line["bbox"]
                lines.append((y0, x0, text, y1))
    lines.sort(key=lambda item: (round(item[0] / 3), item[1]))
    return lines, width, height

def inspect(path: Path):
    doc = open_pdf(path)
    warnings = ["Detected questions are draft suggestions. Confirm question numbers, page ranges, marks and QP/MS pairing before publishing.",
                "Previews use complete source pages. A page may contain other questions or shared material; no automatic cropping is performed."]
    candidates = []
    current = None
    total = 0
    empty_pages = []
    truncated_pages = []
    rotation_pages = []
    fallback_pages = []
    corrupt_text_pages = []
    try:
        for index, page in enumerate(doc):
            number = index + 1
            if page.rotation:
                rotation_pages.append(number)
            lines, width, height = page_lines(page)
            text = "\n".join(line[2] for line in lines)
            if text and sum(ord(c) < 32 and c not in "\n\r\t" for c in text) / len(text) > .02:
                corrupt_text_pages.append(number)
            if not text.strip():
                empty_pages.append(number)
                fallback_pages.append((number, ""))
                # A scan between native pages cannot safely be attached to prior question.
                current = None
                continue
            remaining = min(MAX_PAGE_CHARS, MAX_TEXT_CHARS - total)
            if remaining <= 0:
                warnings.append("Text extraction budget reached; remaining pages require manual review.")
                break
            if len(text) > remaining:
                truncated_pages.append(number)
                text = text[:remaining]
            total += len(text)
            consumed = 0
            found_on_page = False
            for y, x, content, y1 in lines:
                if consumed >= remaining:
                    break
                content = content[:remaining - consumed]
                consumed += len(content) + 1
                match = HEADING.match(content)
                body = match.group(2) if match else ""
                # Avoid bare page numbers, numeric table cells and footer IDs.
                candidate_heading = (match and x <= width * .30 and height * .045 <= y < height * .94
                                     and (re.search(r"[A-Za-z\u4e00-\u9fff]", body) or body.startswith("(")))
                if candidate_heading:
                    qno = match.group(1)
                    if current is None or current["question_no"] != qno:
                        if len(candidates) >= MAX_CANDIDATES:
                            warnings.append("Candidate limit reached; review remaining pages manually.")
                            break
                        current = {"question_no": qno, "qp_pages": [number], "ms_pages": [],
                                   "text": content, "marks": None, "topics": [],
                                   "confidence": "low", "needs_review": True,
                                   "detection": "native_text_heading"}
                        candidates.append(current)
                    else:
                        if number not in current["qp_pages"]:
                            current["qp_pages"].append(number)
                        current["text"] += "\n" + content
                    found_on_page = True
                elif current is not None:
                    if number not in current["qp_pages"]:
                        current["qp_pages"].append(number)
                    current["text"] += "\n" + content
            if not found_on_page and current is None:
                fallback_pages.append((number, text))
        for candidate in candidates:
            marks = [int(n) for n in MARK.findall(candidate["text"])]
            if marks:
                candidate["marks"] = sum(marks)
        if not candidates:
            for number, text in fallback_pages:
                candidates.append({"question_no": f"page-{number}", "qp_pages": [number], "ms_pages": [],
                                   "text": text, "marks": None, "topics": [], "confidence": "none",
                                   "needs_review": True, "detection": "manual_page_review"})
            warnings.append("No reliable native question headings found. Page placeholders are not questions; set boundaries and numbers manually.")
        elif empty_pages:
            # Explicit unassigned placeholders avoid silently losing scanned content.
            for number in empty_pages:
                candidates.append({"question_no": f"page-{number}", "qp_pages": [number], "ms_pages": [],
                                   "text": "", "marks": None, "topics": [], "confidence": "none",
                                   "needs_review": True, "detection": "manual_page_review"})
        if empty_pages:
            warnings.append(f"No native text on pages {','.join(map(str, empty_pages))}. OCR has not been performed; inspect these pages manually.")
        if corrupt_text_pages:
            warnings.append(f"Unreliable native text encoding on pages {corrupt_text_pages}; a text layer exists but requires visual extraction. OCR has not been performed.")
        if truncated_pages:
            warnings.append(f"Native text truncated on pages {','.join(map(str, truncated_pages))}; inspect source pages.")
        if rotation_pages:
            warnings.append(f"Rotated pages {','.join(map(str, rotation_pages))} detected; text uses unrotated coordinates and previews retain page rotation.")
        return {"pages": len(doc), "encrypted": False, "candidates": candidates, "warnings": warnings,
                "status": "needs_review", "text_chars": total, "unassigned_pages": [p for p, _ in fallback_pages],
                "rotation_pages": rotation_pages, "ocr_performed": False,
                "native_text_quality": "corrupt" if corrupt_text_pages else ("missing_or_partial" if empty_pages else "available_unverified"),
                "needs_visual": bool(corrupt_text_pages or empty_pages), "corrupt_text_pages": corrupt_text_pages}
    finally:
        doc.close()

def main():
    try:
        if len(sys.argv) != 2:
            raise IntakeError("Usage: inspect_pdf.py /absolute/file.pdf")
        result = inspect(Path(sys.argv[1]))
        print(json.dumps(result, ensure_ascii=False))
        return 0
    except Exception as exc:
        message = str(exc) if isinstance(exc, IntakeError) else "PDF inspection failed; source requires manual review"
        print(json.dumps({"error": message, "status": "failed", "pages": 0,
                          "encrypted": "Encrypted" in message, "candidates": [], "warnings": []}, ensure_ascii=False))
        return 2

if __name__ == "__main__":
    raise SystemExit(main())
