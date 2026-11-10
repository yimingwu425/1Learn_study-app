> Current runtime: imports use Rust + PdfCraft native draft inspection, and full-page packets use `native_packet.rs`. The Python commands in this historical contract remain development oracles. They are not invoked by the app. Coordinates and present runtime details: native-pdf-runtime.md.

# PDF intake contract (first vertical slice)

`python3 tools/inspect_pdf.py /absolute/source.pdf` returns JSON on stdout.
Exit 0 means inspection finished, **not** that questions are approved. Exit 2
returns `{error,status:"failed",pages:0,encrypted,candidates:[],warnings:[]}`.
PyMuPDF 1.26.6 was used in the cloud tests.

Success includes `pages`, `encrypted:false`, `candidates`, `warnings`,
`status:"needs_review"`, `text_chars`, `unassigned_pages`, `rotation_pages`, and
`ocr_performed:false`.

Every candidate has `question_no`, `qp_pages` (one-based), `ms_pages:[]`, `text`,
`marks` (nullable), `topics:[]`, `confidence`, `needs_review:true`, `detection`.
The tool only proposes native-text question headings; it does not assign exam
identity, topics, answer pairing, or verification status. The Rust import layer
owns file-role detection and the explicit user-confirmed pairing workflow.
When inspecting an MS, map pages to MS references in the application; the raw
inspector does not know which document role it received.

Page placeholders use `question_no:"page-N"`, `confidence:"none"`,
`detection:"manual_page_review"`. The user must replace them with actual
question numbers and confirm boundaries. They are not automatically publishable.
Marks are a sum of bracketed numeric marks found in proposed text, so repeated
mark tables or false headings can make them wrong. Require review.

## Limits and safety

- Source files are read-only, no OCR or network calls.
- Reject over 25 MiB, encrypted or malformed PDFs, and over 500 pages.
- Extract at most 40,000 chars per page, 1,000,000 per document.
- Call from Rust with a wall-time limit (suggest 30 seconds) and bounded stdout.
- Treat extracted text as untrusted content, not code or instructions.
- Candidate page ranges are entire pages. Never represent them as exact crops.
- Candidate heading confidence is low; rotated native text is processed using
  unrotated PyMuPDF text coordinates. The tool does not silently attach a blank
  scanned page to the previous question.
- Preview by rasterizing using `render_pdf_page.py file page out.png`; this avoids
  executing active PDF content. Output longest edge is capped at 1800 pixels.
- The consuming app must authorize all source/output paths, sanitize uploads,
  impose HTTP/body limits, and keep outputs inside its own storage directory.

## Known scope limits

There is no OCR or reliable semantic segmentation. Table cells can look like
question headings. Multiple columns and separately positioned question numbers
may require manual review. No claim of precise cropping or full QP/MS pairing
accuracy is made. References retain complete pages to preserve diagrams,
shared stimuli, and surrounding material while the user verifies boundaries.

Run `python3 -m unittest discover -s tests -p 'test_pdf_intake.py' -v`.
The 11 tests use original synthetic documents, not extracted commercial exams.

# Packet export

`python3 tools/export_packet.py manifest.json output.zip`

Manifest schema:

```json
{"mode":"practice","questions":[{"id":"q1","source":"Exam source label",
"question_no":"1(a)","qp_path":"/authorized/source-qp.pdf","qp_pages":[2,3],
"ms_path":"/authorized/source-ms.pdf","ms_pages":[2],"note":"Optional personal note"}]}
```

Pages are one-based; duplicate pages and invalid references are rejected.
`ms_path` may be absent/null only with an empty/absent `ms_pages`. An MS path with
no pages is rejected as ambiguous. Every supplied source must exist and pass the
same PDF safety limits as intake. The manifest is at most 2 MiB, with up to 200
questions and 1000 output pages including dividers. Call with a process timeout.

The ZIP contains student.pdf, optional answers.pdf, provenance.json and README.txt.
Each question is preceded by a divider identifying original question/source/pages;
whole source pages are then copied. Rotations, diagrams and source text are kept.
Other questions sharing the page may be visible: the PDF does not claim precise
question cropping. Page annotations, links and widgets are not copied.

Practice mode omits supplied notes; study mode adds them to student dividers.
Provenance contains source basenames and page mappings, not host filesystem paths
or personal notes. Missing unsupplied answers are explicitly listed. A supplied
but missing/bad answer file fails the entire export instead of silently skipping.
Output creation is atomic, with no partial ZIP left on failure; source PDFs are
never modified. Rust must authorize both input and output paths and must never
pass user-controlled paths without checking containment.

Run `python3 -m unittest discover -s tests -p 'test_export_packet.py' -v`.
11 tests cover native pages, rotation, provenance, missing answers, missing files,
out-of-range/duplicate/invalid pages, study/practice notes, atomic failure, CLI JSON.
