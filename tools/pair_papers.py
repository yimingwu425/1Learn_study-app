#!/usr/bin/env python3
"""Validate provider-neutral agent proposals against immutable local PDF manifests.
No AI service is invoked. Coordinates are unrotated PDF points, 1-based pages.
CLI: pair_papers.py manifest.json proposal.json
"""
import hashlib
import math
import re
import json
import sys
from pathlib import Path
import pymupdf as fitz
import jsonschema
from inspect_pdf import open_pdf, IntakeError
from render_pdf_page import render_region, RENDER_VERSION

SCHEMA = Path(__file__).parent / 'schemas' / 'agent_intake.schema.json'

def norm(s):
    return ' '.join(s.split())

def schema_only(proposal):
    jsonschema.Draft202012Validator(json.loads(SCHEMA.read_text())).validate(proposal)
    seen = set()
    digests = []
    for q in proposal['questions']:
        key = (q['exam_key'], q['question_no'])
        if key in seen:
            raise IntakeError('Duplicate question identity')
        seen.add(key)
        if not math.isfinite(q['confidence']) or any(not math.isfinite(v) for k in ('question','answer','figures') for r in q[k] for v in r['bbox']):
            raise IntakeError('Non-finite number')
        digests.append({'exam_key':q['exam_key'],'question_no':q['question_no'],
                        'proposal_sha256':hashlib.sha256(json.dumps(q,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest()})
    return {'schema_version':'1','valid':True,'question_digests':digests}

# Specific omission language only; ordinary copyright notices are not omissions.
OMITTED = re.compile(r'(?:content|material|text|image|photograph|figure|extract).{0,60}(?:removed|omitted|redacted|not reproduced).{0,80}(?:copyright|permission)|(?:copyright restrictions)', re.I)

def validate(manifest, proposal):
    schema_only(proposal)
    if manifest.get('schema_version') != '1':
        raise IntakeError('Unsupported manifest version')
    docs = {}
    try:
        for source in manifest['sources']:
            sid = source['id']
            if sid in docs or source['role'] not in ('qp', 'ms'):
                raise IntakeError('Duplicate source or invalid role')
            path = Path(source['path'])
            doc = open_pdf(path)
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            if digest != source['sha256'] or len(doc) != source['pages']:
                doc.close()
                raise IntakeError('Source hash/page manifest mismatch')
            docs[sid] = (source, doc)
        auto, queue, seen = [], [], set()
        used_answer_regions = set()
        for q in proposal['questions']:
            flags = list(q['exception_flags'])
            for kind in ('question', 'answer'):
                if len({r['source_id'] for r in q[kind]}) > 1:
                    flags.append('multiple_' + ('qp' if kind == 'question' else 'ms') + '_sources')
            if not math.isfinite(q['confidence']):
                raise IntakeError('Non-finite confidence')
            key = (q['exam_key'], q['question_no'])
            if key in seen:
                raise IntakeError('Duplicate question identity')
            seen.add(key)
            evidence = []
            for kind in ('question', 'answer', 'figures'):
                for span in q[kind]:
                    sid = span['source_id']
                    if sid not in docs:
                        raise IntakeError('Unknown source reference')
                    source, doc = docs[sid]
                    if kind in ('question', 'answer') and source['role'] != ('qp' if kind == 'question' else 'ms'):
                        raise IntakeError('Wrong QP/MS role')
                    page_num = span['page']
                    if page_num > len(doc):
                        raise IntakeError('Page reference out of range')
                    page = doc[page_num - 1]
                    if not all(math.isfinite(v) for v in span['bbox']):
                        raise IntakeError('Non-finite rectangle')
                    region_key = (sid, page_num, tuple(span['bbox']))
                    if kind == 'answer':
                        if region_key in used_answer_regions:
                            raise IntakeError('Identical answer region reused across questions')
                        used_answer_regions.add(region_key)
                    box = fitz.Rect(span['bbox'])
                    bounds = fitz.Rect(0, 0, page.cropbox.width, page.cropbox.height)
                    if box.is_empty or box.is_infinite or not bounds.contains(box):
                        raise IntakeError('Invalid or out-of-bounds rectangle')
                    text = page.get_text('text', clip=box)
                    # Whole selected page is checked, even when the proposed crop avoids the omission notice.
                    if OMITTED.search(norm(page.get_text('text'))):
                        flags.append('copyright_content_omitted')
                    quote = span['quote']
                    native_matched = bool(norm(quote)) and norm(quote) in norm(text)
                    quote_digest = hashlib.sha256(quote.encode('utf-8')).hexdigest()
                    visual = False
                    image_digest = None
                    for v in manifest.get('visual_verifications', []):
                        if (v.get('source_id') != sid or v.get('page') != page_num or v.get('bbox') != span['bbox']
                            or v.get('quote_sha256') != quote_digest or v.get('render_version') != RENDER_VERSION
                            or not v.get('verifier_run_id') or v.get('verifier_run_id') == proposal['agent_run_id']
                            or not {'transcription_correct','region_complete'}.issubset(set(v.get('checks', [])))):
                            continue
                        if image_digest is None:
                            image_digest = hashlib.sha256(render_region(page, span['bbox'])).hexdigest()
                        if v.get('image_sha256') == image_digest:
                            visual = True
                            break
                    corrupt = text and sum(ord(c) < 32 and c not in '\n\r\t' for c in text) / len(text) > .02
                    if corrupt and not visual:
                        flags.append('corrupt_text_encoding_requires_vision')
                    matched = native_matched or visual
                    if kind != 'figures' and (not matched or not norm(quote)):
                        flags.append('unverified_source_quote')
                    if not norm(page.get_text()) and not visual:
                        flags.append('scan_requires_verified_ocr')
                    evidence.append({'kind':kind,'source_id':sid,'page':page_num,'bbox':span['bbox'],
                                     'sha256':source['sha256'],'quote_verified':matched,
                                     'verification_method':'independent_visual' if visual else 'native_text',
                                     'image_sha256':image_digest,'quote_sha256':quote_digest})
            if not q['answer']:
                flags.append('missing_answer')
            if not q['stem_text'].strip():
                flags.append('missing_stem_text')
            if q['answer'] and not q['answer_text'].strip():
                flags.append('missing_answer_text')
            # The manifest identity must be verified independently by the host, not supplied by the proposing agent.
            sources_used = {s['source_id'] for k in ('question','answer','figures') for s in q[k]}
            for role in ('qp', 'ms'):
                if len({sid for sid in sources_used if docs[sid][0]['role'] == role}) > 1:
                    flags.append('multiple_' + role + '_sources')
            if any(docs[s][0].get('verified_exam_key') != q['exam_key'] for s in sources_used):
                flags.append('exam_identity_unverified')
            # A second independent acceptance record is host-owned. Self-reported confidence is never sufficient.
            acceptance = next((a for a in manifest.get('verified_questions', [])
                               if a.get('exam_key') == q['exam_key'] and a.get('question_no') == q['question_no']
                               and a.get('proposal_sha256') == hashlib.sha256(json.dumps(q,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest()), None)
            required = {'boundaries_complete','answer_pair_correct','figures_complete','subparts_complete','transcription_complete'}
            if not acceptance or not required.issubset(set(acceptance.get('checks', []))):
                flags.append('independent_verification_required')
            if q['confidence'] < .9:
                flags.append('low_agent_confidence')
            out = dict(q, evidence=evidence, exception_flags=sorted(set(flags)), status='exception_queue' if flags else 'auto_ready')
            (queue if flags else auto).append(out)
        return {'schema_version':'1','auto_ready':auto,'exception_queue':queue,
                'metrics':{'ai_accuracy':None,'benchmark_status':'not_evaluated','auto_ready_count':len(auto),'exception_count':len(queue),'source_checks':{'source_count':len(docs),'hashes_verified':True,'page_counts_verified':True}},
                'warnings':['Provider-neutral validation only; AI recognition accuracy has not been evaluated.',
                            'Verified quotes establish provenance, not complete or correct question/answer boundaries.']}
    finally:
        for _, doc in docs.values():
            doc.close()

def main():
    try:
        if len(sys.argv) != 3:
            raise IntakeError('Usage: pair_papers.py manifest.json proposal.json')
        if sys.argv[1] == '--schema-only':
            path = Path(sys.argv[2])
            if path.stat().st_size > 4*1024*1024:
                raise IntakeError('JSON input exceeds 4 MiB')
            print(json.dumps(schema_only(json.loads(path.read_text())), ensure_ascii=False))
            return 0
        paths = [Path(p) for p in sys.argv[1:]]
        if any(p.stat().st_size > 4*1024*1024 for p in paths):
            raise IntakeError('JSON input exceeds 4 MiB')
        print(json.dumps(validate(*(json.loads(p.read_text()) for p in paths)),ensure_ascii=False))
        return 0
    except Exception as exc:
        print(json.dumps({'status':'failed','error':str(exc)}))
        return 2
if __name__ == '__main__':
    sys.exit(main())
