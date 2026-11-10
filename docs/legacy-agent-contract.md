# Legacy Agent compatibility contract

Verified by reading `app/api/agent/tools/route.ts` in the original Mac project. Do not replace the original endpoint yet. The old endpoint is a JSON HTTP tool dispatcher, not a complete MCP transport.

## Envelope
GET /api/agent/tools returns `{service:"Cambridge AS Level Paper Maker - Agent Gateway",version:"2.2.0",protocol:"FunctionCalling / MCP Tools compatible",tools:[{name,description,parameters}]}`.
POST accepts `{tool?:string,name?:string,arguments?:object,args?:object}`. Resolution: tool || name; arguments || args || {}. subject defaults to 9708, supported subjects 9708 and 9702. Missing tool 400, unsupported subject 400, unknown tool 404, invalid query 400, unhandled error 500.

## Tool contracts
- get_subjects: no parameters. Response `{subjects:[{id,name,shortName,level,defaultYearRange,paperRules}]}`.
- get_syllabus_tree: subject required. Response `{subject,topics,subtopics,sections}`.
- search_questions: subject required; paperType paper1/paper2/paper3; syllabusRef; yearFrom; yearTo; keyword; limit default20 max100. Response `{subject,total,questions:[{paperId,questionRef,marks,topic,subtopic,knowledgePoint,confidence,evidence,qpPdfPage,msPdfPage,pdfSliceUrl}]}`.
- semantic_vector_search: subject and query required; paperType; limit default10 max30. Response `{query,subject,total,results:[{paperId,questionRef,marks,matchScorePct,topic,subtopic,knowledgePoint,evidence,pdfSliceUrl}]}`. Do not silently substitute keyword matching under this tool name and claim vector scores.
- generate_exam_paper: subject and paperType required; paperType paper1/paper2/paper2dr/paper2essay/paper3; mode paper/assignment defaultpaper; targetMarks positive integer optional; sections comma-separated string. Response `{title,subject,paperType,totalMarks,printedMarks,answerSections,totalQuestions,exportBundleApi:"/api/teacher-export",questions:[{index,paperId,questionRef,marks,knowledgePoint,subtopic}]}`. totalMarks maps plan.actualMarks; totalQuestions maps plan.actualQuestions. This response omits the full export plan the separate legacy export endpoint needs, so compatibility is not equivalent to end-to-end export support.
- get_syllabus_coverage: subject required; paperType; yearFrom; yearTo; includeEmpty defaulttrue. Response forwarded from getSyllabusCoverage.
- get_kp_questions: subject and either syllabusRef or tagId required; paperType; yearFrom; yearTo; role primary/secondary/all defaultall; sort year_desc/year_asc/marks_desc defaultyear_desc; limit default20 max100; offset default0. Response forwarded from getKpQuestions.
- get_kp_trend: subject and either syllabusRef or tagId required; paperType; yearFrom; yearTo. Response forwarded from getKpTrend.

Coverage helper validates syllabusRef with `^\d{1,2}(\.\d{1,2}){0,2}$`; years 1990–2100. Full nested return types of the three coverage functions were not read before Mac disconnected. They must not be advertised as precisely verified. Existing type source is `lib/queries/types.ts` near lines206–300.

## Source PDFs
Legacy `/api/source-pdf?paperId=...&kind=qp|ms` accepts optional subject. Returns whole PDF, not question crop, despite `pdfSliceUrl` naming. Missing paperId or invalid kind400; absent paper/file404; read error503. It tries another subject when paper missing in chosen subject. Data paths must reject absolute paths, `..`, URLs and non-PDFs.

## New migration
`python3 tools/migrate_legacy.py --db /path/legacy.sqlite --subject 9702 --pdf-root /path/AS\ Phys\ 9702 --dest /path/v2/data`

Dry-run is default. Add `--apply` to import. Destination is `study.sqlite`; original database is readonly and hashed before/after. Repeated question IDs are skipped, not updated. Missing source PDFs remain explicitly unavailable. All imported rows remain legacy_unreviewed. Whole-page extraction does not equal question segmentation or answer validation.

### Normalized migration fields
- `topics` is a deduplicated string array, selected in order: knowledge_point_cn, knowledge_point_en, subtopic_cn, subtopic, topic_cn, topic. Full records remain in legacy_metadata.tags.
- `syllabus_refs` and `tag_ids` are dedicated deduplicated string arrays.
- `year` comes from the legacy paper. `paper_type` derives from component (2 or22 → paper2), with paper_id suffix fallback. Unknown values remain null, not guessed.
- Missing/invalid QP page mappings stay empty; raw values remain in legacy_metadata. A PDF found but out-of-range page detected by extraction produces no qp_url and no usable qp_pages.
- Export protection verified: missing qp_file_id is rejected by Rust export endpoint; empty/null/zero qp_pages is rejected by export_packet.normalized_questions. Source-page bounds are rechecked during PDF assembly. legacy_unreviewed status itself currently creates a warning, not a hard export prohibition.
- Re-running migration never updates an existing row. If an earlier migration produced a wrong normalized field, use a fresh destination for validation rather than silently replacing an existing question.
