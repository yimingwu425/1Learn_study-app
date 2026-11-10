//! Provider-neutral provenance gates; no model calls or model accuracy claims.
use super::*;
use pdf_document::{Document, Rect};
use std::collections::{BTreeMap, BTreeSet};
fn normalized(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
// Preserve the existing provider-neutral Python JSON canonicalization contract.
// Numbers keep integer/float identity, Python's decimal/scientific thresholds,
// and its two-digit exponent; no interpreter is involved.
fn canonical(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut keys = m.keys().collect::<Vec<_>>();
            keys.sort();
            format!(
                "{{{}}}",
                keys.iter()
                    .map(|k| format!(
                        "{}:{}",
                        serde_json::to_string(*k).unwrap(),
                        canonical(&m[*k])
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        Value::Number(n) if n.is_f64() => {
            let raw = n.to_string();
            let neg = raw.starts_with('-');
            let unsigned = raw.trim_start_matches('-');
            if n.as_f64().unwrap() == 0.0 {
                return if neg { "-0.0" } else { "0.0" }.into();
            }
            let (mantissa, power) = unsigned
                .split_once('e')
                .map(|(m, e)| (m, e.parse::<i32>().unwrap()))
                .unwrap_or((unsigned, 0));
            let dot = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
            let digits = mantissa.replace('.', "");
            let leading = digits.bytes().take_while(|c| *c == b'0').count();
            let sig = digits[leading..].trim_end_matches('0');
            let exponent = power + dot - leading as i32 - 1;
            let sign = if neg { "-" } else { "" };
            if (-4..16).contains(&exponent) {
                let pos = exponent + 1;
                let body = if pos <= 0 {
                    format!("0.{}{}", "0".repeat((-pos) as usize), sig)
                } else if pos as usize >= sig.len() {
                    format!("{}{}.0", sig, "0".repeat(pos as usize - sig.len()))
                } else {
                    format!("{}.{}", &sig[..pos as usize], &sig[pos as usize..])
                };
                format!("{sign}{body}")
            } else {
                let mantissa = if sig.len() == 1 {
                    sig.into()
                } else {
                    format!("{}.{}", &sig[..1], &sig[1..])
                };
                format!(
                    "{sign}{mantissa}e{}{:02}",
                    if exponent < 0 { "-" } else { "+" },
                    exponent.abs()
                )
            }
        }
        _ => serde_json::to_string(v).unwrap(),
    }
}
fn digest(q: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(q).as_bytes()))
}
pub fn schema_only(proposal: &Value) -> Result<Value, ApiError> {
    static SCHEMA: std::sync::OnceLock<jsonschema::Validator> = std::sync::OnceLock::new();
    let validator = SCHEMA.get_or_init(|| {
        let schema: Value =
            serde_json::from_str(include_str!("../../tools/schemas/agent_intake.schema.json"))
                .expect("schema JSON");
        jsonschema::validator_for(&schema).expect("local embedded schema")
    });
    validator
        .validate(proposal)
        .map_err(|e| bad(format!("识别结果格式无效：{e}")))?;
    let mut seen = BTreeSet::new();
    let mut digests = vec![];
    for q in proposal["questions"].as_array().unwrap() {
        if !seen.insert((
            q["exam_key"].as_str().unwrap(),
            q["question_no"].as_str().unwrap(),
        )) {
            return Err(bad("重复题目身份"));
        }
        digests.push(json!({"exam_key":q["exam_key"],"question_no":q["question_no"],"proposal_sha256":digest(q)}));
    }
    Ok(json!({"schema_version":"1","valid":true,"question_digests":digests}))
}
fn checks(v: &Value, required: &[&str]) -> bool {
    required.iter().all(|s| {
        v["checks"]
            .as_array()
            .is_some_and(|a| a.contains(&json!(s)))
    })
}
fn omitted(s: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(||regex::Regex::new(r"(?i)(?:content|material|text|image|photograph|figure|extract).{0,60}(?:removed|omitted|redacted|not reproduced).{0,80}(?:copyright|permission)|copyright restrictions").unwrap()).is_match(s)
}
pub async fn validate(
    app: &App,
    proposal: &Value,
    manifest: Option<&Value>,
) -> Result<Value, ApiError> {
    let result = schema_only(proposal)?;
    let Some(manifest) = manifest else {
        return Ok(result);
    };
    if manifest["schema_version"] != "1" {
        return Err(bad("不支持的来源清单版本"));
    }
    let mut docs = BTreeMap::new();
    for s in manifest["sources"]
        .as_array()
        .ok_or_else(|| bad("缺少来源清单"))?
    {
        let sid = s["id"].as_str().ok_or_else(|| bad("缺少来源标识"))?;
        if docs.contains_key(sid) || !["qp", "ms"].contains(&s["role"].as_str().unwrap_or("")) {
            return Err(bad("重复来源或无效角色"));
        }
        let path = PathBuf::from(s["path"].as_str().ok_or_else(|| bad("缺少原文件"))?);
        let bytes = std::fs::read(&path).map_err(internal)?;
        if format!("{:x}", Sha256::digest(bytes)) != s["sha256"].as_str().unwrap_or("") {
            return Err(bad("原文件哈希与清单不符"));
        }
        let doc = Document::load(app, &path, true).await?;
        if Some(doc.pages.len() as u64) != s["pages"].as_u64() {
            return Err(bad("原文件页数与清单不符"));
        }
        docs.insert(sid.to_owned(), (s.clone(), doc));
    }
    let mut ready = vec![];
    let mut queue = vec![];
    let mut used_answers = BTreeSet::new();
    let render_dir = app.data.join("recognition").join("validation-renders");
    for q in proposal["questions"].as_array().unwrap() {
        let mut flags = q["exception_flags"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let mut evidence = vec![];
        let mut used_sources = BTreeSet::new();
        for kind in ["question", "answer", "figures"] {
            for span in q[kind].as_array().unwrap() {
                let sid = span["source_id"].as_str().unwrap();
                let (s, doc) = docs.get(sid).ok_or_else(|| bad("未知来源文件"))?;
                used_sources.insert(sid.to_owned());
                if (kind == "question" && s["role"] != "qp")
                    || (kind == "answer" && s["role"] != "ms")
                {
                    return Err(bad("QP/MS 来源角色错误"));
                }
                let page_num = span["page"].as_u64().unwrap() as usize;
                let page = doc
                    .pages
                    .get(page_num - 1)
                    .ok_or_else(|| bad("原卷页码越界"))?;
                let rect = Rect::from(&span["bbox"])?;
                if !page.bounds().contains(rect) {
                    return Err(bad("区域超出原卷范围"));
                }
                let box_key =
                    [rect.0, rect.1, rect.2, rect.3]
                        .map(|n| if n == 0.0 { 0 } else { n.to_bits() });
                if kind == "answer" && !used_answers.insert((sid.to_owned(), page_num, box_key)) {
                    return Err(bad("不同题目复用了完全相同的答案区域"));
                }
                let text = page.text(Some(rect));
                let whole = page.text(None);
                if omitted(&normalized(&whole)) {
                    flags.insert("copyright_content_omitted".into());
                }
                let quote = span["quote"].as_str().unwrap();
                let quote_digest = format!("{:x}", Sha256::digest(quote.as_bytes()));
                let native =
                    !normalized(quote).is_empty() && normalized(&text).contains(&normalized(quote));
                let mut visual = false;
                let mut image_digest = None;
                for v in manifest["visual_verifications"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    if v["source_id"] != sid
                        || v["page"] != span["page"]
                        || v["bbox"] != span["bbox"]
                        || v["quote_sha256"] != quote_digest
                        || v["render_version"] != pdf_document::RENDER_VERSION
                        || v["verifier_run_id"].as_str().is_none_or(str::is_empty)
                        || v["verifier_run_id"] == proposal["agent_run_id"]
                        || !checks(v, &["transcription_correct", "region_complete"])
                    {
                        continue;
                    }
                    if image_digest.is_none() {
                        image_digest = Some(
                            pdf_document::render_region(
                                app,
                                &doc.path,
                                page_num,
                                rect,
                                &render_dir,
                            )
                            .await?["image_sha256"]
                                .as_str()
                                .unwrap()
                                .to_owned(),
                        );
                    }
                    if v["image_sha256"].as_str() == image_digest.as_deref() {
                        visual = true;
                        break;
                    }
                }
                let chars = text.chars().count();
                let corrupt = chars > 0
                    && text
                        .chars()
                        .filter(|c| c.is_control() && !['\n', '\r', '\t'].contains(c))
                        .count() as f64
                        / chars as f64
                        > 0.02;
                if corrupt && !visual {
                    flags.insert("corrupt_text_encoding_requires_vision".into());
                }
                if kind != "figures" && (!(native || visual) || normalized(quote).is_empty()) {
                    flags.insert("unverified_source_quote".into());
                }
                if normalized(&whole).is_empty() && !visual {
                    flags.insert("scan_requires_verified_ocr".into());
                }
                evidence.push(json!({"kind":kind,"source_id":sid,"page":page_num,"bbox":span["bbox"],"sha256":s["sha256"],"quote_verified":native||visual,"verification_method":if visual{"independent_visual"}else{"native_text"},"image_sha256":image_digest,"quote_sha256":quote_digest}));
            }
        }
        if q["answer"].as_array().unwrap().is_empty() {
            flags.insert("missing_answer".into());
        }
        if q["stem_text"].as_str().unwrap().trim().is_empty() {
            flags.insert("missing_stem_text".into());
        }
        if !q["answer"].as_array().unwrap().is_empty()
            && q["answer_text"].as_str().unwrap().trim().is_empty()
        {
            flags.insert("missing_answer_text".into());
        }
        for role in ["qp", "ms"] {
            if used_sources
                .iter()
                .filter(|sid| docs[*sid].0["role"] == role)
                .count()
                > 1
            {
                flags.insert(format!("multiple_{role}_sources"));
            }
        }
        if used_sources
            .iter()
            .any(|sid| docs[sid].0["verified_exam_key"] != q["exam_key"])
        {
            flags.insert("exam_identity_unverified".into());
        }
        let acceptance = manifest["verified_questions"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|a| {
                a["exam_key"] == q["exam_key"]
                    && a["question_no"] == q["question_no"]
                    && a["proposal_sha256"] == digest(q)
            });
        if acceptance.is_none_or(|v| {
            !checks(
                v,
                &[
                    "boundaries_complete",
                    "answer_pair_correct",
                    "figures_complete",
                    "subparts_complete",
                    "transcription_complete",
                ],
            )
        }) {
            flags.insert("independent_verification_required".into());
        }
        if q["confidence"].as_f64().unwrap() < 0.9 {
            flags.insert("low_agent_confidence".into());
        }
        let mut out = q.clone();
        out["evidence"] = json!(evidence);
        out["exception_flags"] = json!(flags);
        out["status"] = json!(if flags.is_empty() {
            "auto_ready"
        } else {
            "exception_queue"
        });
        if flags.is_empty() {
            ready.push(out)
        } else {
            queue.push(out)
        }
    }
    Ok(
        json!({"schema_version":"1","auto_ready":ready,"exception_queue":queue,"metrics":{"ai_accuracy":null,"benchmark_status":"not_evaluated","auto_ready_count":ready.len(),"exception_count":queue.len(),"source_checks":{"source_count":docs.len(),"hashes_verified":true,"page_counts_verified":true}},"warnings":["Provider-neutral validation only; AI recognition accuracy has not been evaluated.","Verified quotes establish provenance, not complete or correct question/answer boundaries."],"engine":"Rust + PdfCraft","render_version":pdf_document::RENDER_VERSION}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_digest_matches_retained_protocol_numbers() {
        for (raw, expected) in [
            ("1.0", "1.0"),
            ("0.00001", "1e-05"),
            ("0.0001", "0.0001"),
            ("1e16", "1e+16"),
            ("1e15", "1000000000000000.0"),
            ("-0.0", "-0.0"),
            ("1.234567e-6", "1.234567e-06"),
            ("15.23", "15.23"),
        ] {
            let v: Value = serde_json::from_str(raw).unwrap();
            assert_eq!(canonical(&v), expected);
        }
        let v: Value =
            serde_json::from_str(r#"{"z":0.00001,"中文":"保留原文","a":[1,1.0]}"#).unwrap();
        assert_eq!(
            canonical(&v),
            r#"{"a":[1,1.0],"z":1e-05,"中文":"保留原文"}"#
        );
    }
    #[test]
    fn strict_schema_digest_and_duplicates() {
        let q = json!({"exam_key":"9702/22/s/2024","question_no":"1","subparts":[],"question":[{"source_id":"qp","page":1,"bbox":[0,0,595,842],"quote":"1 Explain acceleration."}],"answer":[],"figures":[],"confidence":0.99,"exception_flags":[],"stem_text":"1 Explain acceleration.","answer_text":""});
        let mut p = json!({"schema_version":"1","agent_run_id":"test","questions":[q.clone()]});
        assert!(schema_only(&p).is_ok());
        p["questions"].as_array_mut().unwrap().push(q);
        assert!(schema_only(&p).is_err());
        p["questions"].as_array_mut().unwrap().pop();
        p["unexpected"] = json!(true);
        assert!(schema_only(&p).is_err());
        assert!(omitted("Content removed due to copyright restrictions."));
        assert!(!omitted(
            "Copyright Cambridge. Permission to reproduce items has been sought."
        ));
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;
    fn setup(root: &std::path::Path, data: &std::path::Path) -> App {
        std::fs::create_dir_all(data.join("exports")).unwrap();
        App {
            siwc: Arc::new(siwc::Service::new(data)),
            root: root.into(),
            data: data.into(),
            db: Arc::new(Mutex::new(Connection::open_in_memory().unwrap())),
        }
    }
    #[tokio::test]
    async fn provenance_visual_tamper_rotation_and_stale_render_gates() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let app = setup(root, tmp.path());
        let mut sources = vec![];
        for role in ["qp", "ms"] {
            let path = tmp.path().join(format!("{role}.pdf"));
            pdf_document::write_text_pdf(
                &app,
                &path,
                "Fixture",
                &[vec![pdf_document::text(
                    60.,
                    80.,
                    11.,
                    "1 Explain acceleration.",
                )]],
            )
            .unwrap();
            sources.push(json!({"id":role,"role":role,"path":path,"sha256":format!("{:x}",Sha256::digest(std::fs::read(path).unwrap())),"pages":1,"verified_exam_key":"9702/22/s/2024"}));
        }
        let span = |role: &str| json!({"source_id":role,"page":1,"bbox":[50,50,500,100],"quote":"1 Explain acceleration."});
        let q = json!({"exam_key":"9702/22/s/2024","question_no":"1","subparts":[],"question":[span("qp")],"answer":[span("ms")],"figures":[],"confidence":0.99,"exception_flags":[],"stem_text":"1 Explain acceleration.","answer_text":"1 Explain acceleration."});
        let p = json!({"schema_version":"1","agent_run_id":"extractor","questions":[q.clone()]});
        let mut m = json!({"schema_version":"1","sources":sources});
        let result = validate(&app, &p, Some(&m)).await.unwrap();
        assert_eq!(
            result["exception_queue"][0]["exception_flags"],
            json!(["independent_verification_required"])
        );
        m["verified_questions"] = json!([{"exam_key":q["exam_key"],"question_no":"1","proposal_sha256":digest(&q),"checks":["boundaries_complete","answer_pair_correct","figures_complete","subparts_complete","transcription_complete"]}]);
        let r = validate(&app, &p, Some(&m)).await.unwrap();
        assert_eq!(r["auto_ready"].as_array().unwrap().len(), 1);
        assert!(r["metrics"]["ai_accuracy"].is_null());
        let mut bad_quote = p.clone();
        bad_quote["questions"][0]["answer"][0]["quote"] = json!("Fabricated answer");
        assert!(
            validate(&app, &bad_quote, Some(&m)).await.unwrap()["auto_ready"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let mut bad_box = p.clone();
        bad_box["questions"][0]["question"][0]["bbox"] = json!([-2, 50, 500, 100]);
        assert!(validate(&app, &bad_box, Some(&m)).await.is_err());
        let mut bad_hash = m.clone();
        bad_hash["sources"][0]["sha256"] = json!("tampered");
        assert!(validate(&app, &p, Some(&bad_hash)).await.is_err());
        let mut bad_role = p.clone();
        bad_role["questions"][0]["answer"][0]["source_id"] = json!("qp");
        assert!(validate(&app, &bad_role, Some(&m)).await.is_err());
        let mut duplicate = p.clone();
        let mut other = q.clone();
        other["question_no"] = json!("2");
        duplicate["questions"].as_array_mut().unwrap().push(other);
        assert!(validate(&app, &duplicate, Some(&m)).await.is_err());
        // Bake a scan through the mature engine, so quotes cannot pass from a text layer.
        let qp = PathBuf::from(m["sources"][0]["path"].as_str().unwrap());
        let image = tmp.path().join("scan.png");
        pdf_engine::run(
            &app,
            &[
                "render".into(),
                qp.clone().into_os_string(),
                "--page".into(),
                "1".into(),
                "--dpi".into(),
                "96".into(),
                "--out".into(),
                image.clone().into_os_string(),
            ],
        )
        .await
        .unwrap();
        let scan = tmp.path().join("scan.pdf");
        pdf_engine::report(
            &app,
            json!([
                pdf_document::step(
                    "doc_create",
                    json!({"from":"images","paths":[image],"dpi":96})
                ),
                pdf_document::step("doc_save", json!({"doc":1,"path":scan,"full":true}))
            ]),
        )
        .await
        .unwrap();
        m["sources"][0]["path"] = json!(scan);
        m["sources"][0]["sha256"] = json!(format!(
            "{:x}",
            Sha256::digest(std::fs::read(&scan).unwrap())
        ));
        assert!(validate(&app, &p, Some(&m)).await.unwrap()["auto_ready"]
            .as_array()
            .unwrap()
            .is_empty());
        let r = pdf_document::render_region(
            &app,
            &scan,
            1,
            Rect(50., 50., 500., 100.),
            &tmp.path().join("regions"),
        )
        .await
        .unwrap();
        m["visual_verifications"] = json!([{"source_id":"qp","page":1,"bbox":[50,50,500,100],"quote_sha256":format!("{:x}",Sha256::digest(b"1 Explain acceleration.")),"image_sha256":r["image_sha256"],"render_version":pdf_document::RENDER_VERSION,"verifier_run_id":"independent-verifier","checks":["transcription_correct","region_complete"]}]);
        let accepted = validate(&app, &p, Some(&m)).await.unwrap();
        assert_eq!(accepted["auto_ready"].as_array().unwrap().len(), 1);
        assert_eq!(
            accepted["auto_ready"][0]["evidence"][0]["verification_method"],
            "independent_visual"
        );
        for (key, value) in [
            ("image_sha256", json!("tampered")),
            ("render_version", json!("pymupdf-region-v1")),
            ("verifier_run_id", json!("extractor")),
        ] {
            let mut stale = m.clone();
            stale["visual_verifications"][0][key] = value;
            assert!(
                validate(&app, &p, Some(&stale)).await.unwrap()["auto_ready"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }
        // Unrotated-coordinate contract remains valid for /Rotate=90 source pages.
        let rotated = tmp.path().join("rotated.pdf");
        pdf_engine::report(
            &app,
            json!([
                pdf_document::step("doc_open", json!({"path":qp})),
                pdf_document::step("page_rotate", json!({"doc":1,"degrees":90})),
                pdf_document::step("doc_save", json!({"doc":1,"path":rotated,"full":true}))
            ]),
        )
        .await
        .unwrap();
        m["sources"][0]["path"] = json!(rotated);
        m["sources"][0]["sha256"] = json!(format!(
            "{:x}",
            Sha256::digest(std::fs::read(&rotated).unwrap())
        ));
        m["visual_verifications"] = json!([]);
        assert_eq!(
            validate(&app, &p, Some(&m)).await.unwrap()["auto_ready"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(&rotated).unwrap())),
            m["sources"][0]["sha256"].as_str().unwrap()
        );
    }
}
