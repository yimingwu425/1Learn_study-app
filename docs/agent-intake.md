> Current runtime: Rust `source_validator.rs` runs Schema/provenance validation; PdfCraft generates `pdfcraft-region-v2` evidence. The Python CLI examples below describe retained development/legacy oracles, not an application dependency. Use the HTTP proposal/verification routes for current Agent work. See native-pdf-runtime.md. Old v1 image attestations require fresh rendering and independent verification.

# Pluggable AI question intake, contract v1

This is an **agent output verifier**, not an AI recognizer. No provider, model, credentials, or external transmission is configured. Recognition accuracy on real examinations is **not evaluated**. The existing inspect_pdf tool remains an optional native-text baseline, not AI recognition.

## Host responsibilities

Create a recognition job, snapshot read-only originals, validate authorized paths, cap input size and subprocess time, and provide the agent original PDFs plus this schema. Treat PDF text as untrusted content, never instructions. Any model connection and data transmission follows the user's explicit configuration. Never let the proposing agent alter the manifest or acceptance records. Store original proposals, model identity/version, timestamps and validation result for audit.

Run `python3 tools/pair_papers.py manifest.json proposal.json`. No DB writes occur. Stdout is JSON; exit 2 is a whole-result validation failure (unsafe source references, schema, role or file mismatch), exit 0 can still contain exceptions. `jsonschema` and PyMuPDF are required.

## Input manifest (host-owned)

```
{"schema_version":"1","sources":[
 {"id":"qp-import-id","role":"qp","path":"/authorized/qp.pdf","sha256":"hex digest","pages":12,"verified_exam_key":"9702/22/s/2024"},
 {"id":"ms-import-id","role":"ms","path":"/authorized/ms.pdf","sha256":"hex digest","pages":10,"verified_exam_key":"9702/22/s/2024"}
],"verified_questions":[]}
```

`verified_exam_key` must come from independent host verification of printed identity, not simply a filename or agent assertion. Paper component, session and year all matter. Cambridge subjects currently accepted: 9702, 9708; session m/s/w. Extend the schema explicitly for other boards.

Optional acceptance entries: `{exam_key,question_no,proposal_sha256,checks}`. Hash the exact question JSON using Python `json.dumps(q,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()` and SHA256. Required checks are `boundaries_complete`, `answer_pair_correct`, `figures_complete`, `subparts_complete`, `transcription_complete`. These are attestations from an independently configured verifier or a reviewed correction, **not** user labor required for every question, and not trusted when supplied by the proposing agent. No independent verifier is yet configured, so first-pass results correctly wait in the exception queue. A future independent agent can do this automatically. A different agent is not mathematical proof; evaluate its accuracy on held-out papers before routine auto-import.

## Agent result

Full machine-readable JSON Schema: `tools/schemas/agent_intake.schema.json`.

```
{"schema_version":"1","agent_run_id":"job-123/run-1","questions":[
 {"exam_key":"9702/22/s/2024","question_no":"1","stem_text":"Complete question transcription","answer_text":"Complete mark scheme transcription","subparts":["a","b.i"],
  "question":[{"source_id":"qp-import-id","page":2,"bbox":[30,70,560,700],"quote":"Exact native source text"}],
  "answer":[{"source_id":"ms-import-id","page":4,"bbox":[30,120,560,300],"quote":"Exact native mark scheme text"}],
  "figures":[],"confidence":0.97,"exception_flags":[]}
]}
```

All coordinates are unrotated PDF points relative to the cropbox; page numbers start at 1. A rectangle may cover a full page only if that page truly belongs to this question. Preserve diagrams with their original PDF region, not retyped approximations. Quote is provenance evidence, not the complete statement. Questions/answers are arrays of regions to support cross-page questions, table rows and separated continuation rows. Figures have the same region structure; their quote may be empty when they contain no text. Retain originals for vector rendering. Do not copy an entire mark scheme into every answer. Duplicate identical answer regions fail validation.

## Output and auto-import

`{schema_version:"1",auto_ready:[...],exception_queue:[...],warnings:[...]}`.
Each normalized question retains all input fields plus `status`, deduplicated `exception_flags`, and `evidence:[{kind,source_id,page,bbox,sha256,quote_verified}]`.

Auto-ready requires valid immutable sources, exact QP/MS roles, bounded coordinates, verified native source quotes, matching independently verified exam identity, confidence >=0.9, no exception flags, and independent completeness/pairing checks bound to the exact proposal hash. Native quote matching alone cannot establish correctness or completeness. Structural failures stop the whole job. Recoverable uncertainty remains in exceptions; it must not silently publish.

## Difficult layouts

- Cross-page: multiple regions in reading order; check intervening figures and continued subparts.
- Shared stimulus: retain shared region in each dependent question and emit `shared_stimulus` until independent completeness verification resolves it. Never remove it merely because duplicated.
- Scans: native evidence cannot be verified; `scan_requires_verified_ocr` routes to an OCR-capable agent and independently checked image provenance. Independent visual verification is implemented below; a real visual model remains unconnected.
- MCQ: each number maps to its specific answer-table cell/row, never the whole answer table. Independent verification must confirm the selected option. This verifier does not infer answers.
- Economics options: preserve section/rubric and eligibility instructions as source evidence. Emit `optional_section` until verification confirms no questions or instructions were lost.
- Uncertain table rows, unreadable equation, unmatched subpart, unclear number: emit descriptive exception flags. The exception queue is intended for the remaining unresolved cases after automated recognition and independent checks, not mandatory per-question manual entry.

## Tests

`python3 -m unittest discover -s tests -p 'test_pair_papers.py' -v`

Synthetic original fixtures are built in tests. They check source tampering, invalid coordinates/pages/roles, duplicate questions, fabricated quotes, missing answers, and refusal to auto-accept self-reported high confidence. They do not establish real Cambridge recognition accuracy.

`metrics` includes `ai_accuracy:null`, `benchmark_status:"not_evaluated"`, auto-ready/exception counts and source hash/page checks. Passing these source checks is not an accuracy score. Text with >2% unusual control characters within a selected region is flagged `corrupt_text_encoding_requires_vision`; this detects an observed failure in the private physics QP fixture where a text layer exists but is not readable. Real source diagnostics are saved only under ignored `fixtures/private`; no sample papers are published.

### Schema-only extraction stage

`python3 tools/pair_papers.py --schema-only proposal.json` returns `{schema_version:"1",valid:true,question_digests:[{exam_key,question_no,proposal_sha256}]}`. This stage requires no source manifest or known roles. It validates schema, unique identity and finite numbers only; it does not accept for publication. The host stores this as `awaiting_verification`, then obtains an independent run's `source_verifications` and question verification records before constructing the trusted full manifest.

Deterministic exceptions survive reviewer acceptance: selected source pages containing specific copyright omission text produce `copyright_content_omitted`, even if the proposed crop excludes that notice. Ordinary copyright footer notices do not trigger this. Questions spanning more than one QP source or more than one MS source produce `multiple_qp_sources` / `multiple_ms_sources`; multi-page spans in one file remain supported.

### Independent visual verification (implemented, not model-benchmarked)

Host-owned `manifest.visual_verifications` may contain:
`{source_id,page,bbox,quote_sha256,image_sha256,render_version:"pdfcraft-region-v2",verifier_run_id,checks:["transcription_correct","region_complete"]}`.

Hash the exact UTF-8 quote (no whitespace normalization) with SHA256. The Rust/PdfCraft region path generates deterministic unrotated PNG bytes, bounded to 2400px and max 2× scale. `image_sha256` hashes these exact PNG bytes. Host must provide this same image to the independent visual reviewer; rerendering with another PDF engine/version may change bytes and correctly requires fresh verification. Original source hash is also checked by the full validator.

The verifier run must differ from the proposal's `agent_run_id`. Host must authenticate the reviewer identity and create records itself; a string run ID alone is not authentication. If exact region, quote digest, image digest, render version and required checks match, visual provenance can replace unreadable/missing native text. Native encoding/scan flags do not permanently block these verified regions. Missing/incorrect visual evidence still routes to exceptions. Copyright omission flags and independent question-completeness checks remain enforced. This mechanism supports automated second-agent review without routine human pairing, but no actual visual model or its accuracy has been evaluated.

### Complete searchable content versus source evidence

Each proposed question now requires `stem_text` (nonempty, up to 100,000 characters) and `answer_text` (up to 100,000 characters). They contain the full question and full answer/mark-scheme transcription, including all subparts, necessary shared instructions and mathematical content. Original PDF regions remain authoritative for diagrams and layout. Empty answer text is allowed only as an unresolved proposal; supplied answer regions with empty answer text enter `missing_answer_text`. Do not construct either field by joining evidence quotes: quotes may intentionally be short fragments.

The independent question acceptance record additionally requires `transcription_complete`, bound to the complete proposal hash. The reviewer checks both full-text fields against all referenced regions, not only the short quotes. Rust should persist these fields as content and store quote/region evidence separately. Old schema-v1 proposals lacking these new required fields fail schema validation and must be re-extracted rather than silently filled from quotes.
