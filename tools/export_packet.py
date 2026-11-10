#!/usr/bin/env python3
"""Build a local reviewable practice packet from confirmed full-page mappings.

Usage: export_packet.py manifest.json output.zip
No source PDF is modified. Input paths must be authorized by the calling app.
"""
from __future__ import annotations
import json
import os
from pathlib import Path
import sys
import tempfile
import zipfile
import pymupdf as fitz
from inspect_pdf import open_pdf, IntakeError

MAX_QUESTIONS = 200
MAX_OUTPUT_PAGES = 1000
MAX_MANIFEST_BYTES = 2 * 1024 * 1024


def bounded_string(value, field, maximum=1000):
    if not isinstance(value, str) or not value.strip() or len(value) > maximum:
        raise IntakeError(f"{field} must be a non-empty string up to {maximum} characters")
    return value


def page_list(value, field, required=False):
    if not isinstance(value, list) or (required and not value):
        raise IntakeError(f"{field} must be a {'non-empty ' if required else ''}list of page numbers")
    if any(type(n) is not int or n < 1 for n in value):
        raise IntakeError(f"{field} requires positive integer page numbers")
    if len(value) != len(set(value)):
        raise IntakeError(f"{field} contains duplicate page numbers")
    return value


def normalized_questions(manifest):
    if not isinstance(manifest, dict) or manifest.get('mode') not in ('practice', 'study'):
        raise IntakeError("mode must be practice or study")
    questions = manifest.get('questions')
    if not isinstance(questions, list) or not 1 <= len(questions) <= MAX_QUESTIONS:
        raise IntakeError("questions must contain between 1 and 200 entries")
    result, seen = [], set()
    total = 0
    for pos, q in enumerate(questions, 1):
        if not isinstance(q, dict):
            raise IntakeError(f"Question {pos} is not an object")
        ident = bounded_string(q.get('id'), f'Question {pos} id', 200)
        if ident in seen:
            raise IntakeError("Duplicate question id")
        seen.add(ident)
        source = bounded_string(q.get('source'), f'Question {pos} source', 1000)
        raw_no = q.get('question_no')
        if type(raw_no) is int:
            raw_no = str(raw_no)
        qno = bounded_string(raw_no, f'Question {pos} question_no', 100)
        qp_path = bounded_string(q.get('qp_path'), f'Question {pos} qp_path', 4096)
        qp_pages = page_list(q.get('qp_pages'), f'Question {pos} qp_pages', required=True)
        ms_pages = page_list(q.get('ms_pages', []), f'Question {pos} ms_pages')
        ms_path = q.get('ms_path')
        if ms_path is not None:
            ms_path = bounded_string(ms_path, f'Question {pos} ms_path', 4096)
        if ms_pages and not ms_path:
            raise IntakeError(f"Question {pos} has answer pages but no ms_path")
        if ms_path and not ms_pages:
            raise IntakeError(f"Question {pos} has ms_path but no answer pages")
        note = q.get('note', '')
        if not isinstance(note, str) or len(note) > 2000:
            raise IntakeError(f"Question {pos} note must be at most 2000 characters")
        total += 1 + len(qp_pages) + (1 + len(ms_pages) if ms_pages else 0)
        if total > MAX_OUTPUT_PAGES:
            raise IntakeError("Packet exceeds 1000 output pages including source dividers")
        result.append({'id': ident, 'source': source, 'question_no': qno,
                       'qp_path': qp_path, 'qp_pages': qp_pages,
                       'ms_path': ms_path, 'ms_pages': ms_pages, 'note': note})
    return result


def divider(out, number, q, role, mode):
    page = out.new_page(width=595, height=842)
    text = (f"{'Practice question' if role == 'qp' else 'Official answer reference'} {number}\n\n"
            f"Question: {q['question_no']}\nSource: {q['source']}\n"
            f"Original page(s): {', '.join(map(str, q[role+'_pages']))}\n\n"
            "The following pages are complete pages from the original PDF.\n"
            "They may contain adjacent questions, shared material or other answers.\n"
            "Work only on the question identified above. These are not exact question crops.\n")
    if role == 'qp' and mode == 'study' and q['note']:
        text += '\nMy note:\n' + q['note']
    # Built-in CJK font keeps Chinese user notes visible, without remote font fetch.
    result = page.insert_textbox(fitz.Rect(48, 48, 547, 794), text, fontname='china-s', fontsize=11, lineheight=1.4)
    if result < 0:
        raise IntakeError("Question metadata/notes do not fit the divider; shorten the source or note")


def export_packet(manifest_path: Path, output_path: Path):
    if not manifest_path.is_file() or manifest_path.stat().st_size > MAX_MANIFEST_BYTES:
        raise IntakeError("Manifest is missing or exceeds 2 MiB")
    if output_path.suffix.lower() != '.zip':
        raise IntakeError("Output must be a ZIP file")
    try:
        manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
    except (ValueError, UnicodeError) as exc:
        raise IntakeError("Manifest is not valid UTF-8 JSON") from exc
    questions = normalized_questions(manifest)
    mode = manifest['mode']
    documents = {}
    student, answers = fitz.open(), fitz.open()
    tmp_zip = None
    try:
        # Validate all references first: no silently missing questions or answers.
        for q in questions:
            for role in ('qp', 'ms'):
                path = q[f'{role}_path']
                if not path:
                    continue
                resolved = Path(path).resolve()
                if resolved == output_path.resolve() or resolved == manifest_path.resolve():
                    raise IntakeError("Output/manifest must not replace a source PDF")
                key = str(resolved)
                if key not in documents:
                    documents[key] = open_pdf(resolved)
                doc = documents[key]
                if any(n > len(doc) for n in q[f'{role}_pages']):
                    raise IntakeError(f"Question {q['id']} {role.upper()} page number is outside source PDF")
        provenance = []
        missing_answers = []
        for number, q in enumerate(questions, 1):
            entry = {'id': q['id'], 'source': q['source'], 'question_no': q['question_no'],
                     'qp_file': Path(q['qp_path']).name, 'qp_pages': q['qp_pages'],
                     'ms_file': Path(q['ms_path']).name if q['ms_path'] else None,
                     'ms_pages': q['ms_pages'], 'page_mapping': 'complete_source_pages'}
            for role, out in [('qp', student), ('ms', answers)]:
                if not q[f'{role}_path']:
                    continue
                divider(out, number, q, role, mode)
                start = len(out) + 1
                doc = documents[str(Path(q[f'{role}_path']).resolve())]
                for n in q[f'{role}_pages']:
                    # Do not copy interactive links, annotations or widgets into packets.
                    out.insert_pdf(doc, from_page=n-1, to_page=n-1, links=False, annots=False, widgets=False)
                entry[f'{role}_packet_pages'] = list(range(start, len(out)+1))
            if not q['ms_path']:
                missing_answers.append(q['id'])
            provenance.append(entry)
        readme = ("LOCAL STUDY PACKET\n\n"
                  "Source pages are preserved in full, including diagrams and page rotation.\n"
                  "They can contain adjacent questions and shared material; use each divider to identify the target question.\n"
                  "No answer text or marks have been invented. Source/answer pairing must have been reviewed by the user.\n"
                  "student.pdf: question dividers and source QP pages.\n"
                  "answers.pdf: available MS pages only; omitted when none are available.\n"
                  "provenance.json: original filenames/page references and output page mapping.\n"
                  "Original source files were not modified.\n"
                  f"Mode: {mode}. Study mode includes supplied notes; practice mode hides notes.\n"
                  f"Questions without supplied answers: {', '.join(missing_answers) or 'none'}\n")
        output_path.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(prefix='.packet-', suffix='.zip', dir=output_path.parent, delete=False) as handle:
            tmp_zip = Path(handle.name)
        with zipfile.ZipFile(tmp_zip, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
            archive.writestr('student.pdf', student.tobytes(garbage=4, deflate=True))
            if len(answers):
                archive.writestr('answers.pdf', answers.tobytes(garbage=4, deflate=True))
            archive.writestr('provenance.json', json.dumps({'version':1,'mode':mode,'questions':provenance,
                                                          'missing_answers':missing_answers},ensure_ascii=False,indent=2))
            archive.writestr('README.txt',readme)
        os.replace(tmp_zip, output_path)
        tmp_zip = None
        return {'status':'exported','question_count':len(questions),'student_pages':len(student),
                'answer_pages':len(answers),'missing_answers':missing_answers,'output':str(output_path),
                'warnings':['Full source pages may include adjacent questions or shared material.']}
    finally:
        student.close(); answers.close()
        for doc in documents.values():
            doc.close()
        if tmp_zip is not None:
            tmp_zip.unlink(missing_ok=True)


def main():
    try:
        if len(sys.argv) != 3:
            raise IntakeError("Usage: export_packet.py manifest.json output.zip")
        result = export_packet(Path(sys.argv[1]), Path(sys.argv[2]))
        print(json.dumps(result, ensure_ascii=False))
        return 0
    except Exception as exc:
        print(json.dumps({'status':'failed','error':str(exc) if isinstance(exc,IntakeError) else 'Packet export failed; no complete packet was produced'},ensure_ascii=False))
        return 2

if __name__ == '__main__':
    raise SystemExit(main())
