//! Existing PdfCraft engine, invoked by Rust; no custom PDF parser or renderer.
use super::*;
fn source(app: &App, id: &str) -> Result<PathBuf, ApiError> {
    uuid::Uuid::parse_str(id).map_err(|_| bad("文件标识无效"))?;
    let path = app.data.join("files").join(format!("{id}.pdf"));
    if !path.is_file() {
        return Err(err(StatusCode::NOT_FOUND, "原 PDF 不存在"));
    }
    Ok(path)
}
fn binary(app: &App) -> PathBuf {
    std::env::var_os("STUDY_PDFCRAFT_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            app.root.join(
                ".toolchain/pdfcraft-cli/bin/pdfcraft-cli-0.20.0-macos-universal/pdfcraft-cli",
            )
        })
}
pub(crate) async fn run(
    app: &App,
    args: &[std::ffi::OsString],
) -> Result<std::process::Output, ApiError> {
    let mut cmd = tokio::process::Command::new(binary(app));
    cmd.args(args).kill_on_drop(true);
    let out = tokio::time::timeout(std::time::Duration::from_secs(45), cmd.output())
        .await
        .map_err(|_| {
            err(
                StatusCode::REQUEST_TIMEOUT,
                "PDF 页面处理超时，请打开原文核对",
            )
        })?
        .map_err(|_| {
            err(
                StatusCode::SERVICE_UNAVAILABLE,
                "本地 PDF 引擎尚未就绪，可使用兼容预览",
            )
        })?;
    if !out.status.success() {
        eprintln!(
            "PdfCraft error: {} {}",
            String::from_utf8_lossy(&out.stderr)
                .chars()
                .take(1000)
                .collect::<String>(),
            String::from_utf8_lossy(&out.stdout)
                .chars()
                .take(1000)
                .collect::<String>()
        );
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "PDF 引擎无法处理该页，请打开原文核对",
        ));
    }
    Ok(out)
}
pub(crate) async fn report(app: &App, steps: Value) -> Result<Value, ApiError> {
    let dir = app.data.join("renders");
    std::fs::create_dir_all(&dir).map_err(internal)?;
    let script = dir.join(format!("inspect-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&script, serde_json::to_vec(&steps).map_err(internal)?).map_err(internal)?;
    let out = run(
        app,
        &[
            "run".into(),
            "--script".into(),
            script.clone().into_os_string(),
            "--report".into(),
        ],
    )
    .await;
    let _ = std::fs::remove_file(script);
    let out = out?;
    let v: Value = serde_json::from_slice(&out.stdout).map_err(internal)?;
    if v["ok"] != true
        || v["steps"]
            .as_array()
            .is_none_or(|s| s.iter().any(|s| s["status"] != "ok"))
    {
        return Err(bad("PDF 文字层无法完整读取，请查看原件"));
    }
    Ok(v)
}
/// Intake is a bounded draft inspection; these proposals are never published automatically.
pub(crate) async fn inspect(app: &App, path: &std::path::Path) -> Result<Value, ApiError> {
    let info = report(
        app,
        json!([{"tool":"doc_open","args":{"path":path}},{"tool":"doc_info","args":{"doc":1}}]),
    )
    .await?;
    let doc = &info["steps"][0]["result"][0];
    if doc["encrypted"] == true {
        return Err(bad("加密 PDF 请先提供可读取的副本"));
    }
    let count = doc["pages"].as_u64().unwrap_or(0) as usize;
    if !(1..=500).contains(&count) {
        return Err(bad("PDF 页数必须在 1–500 之间"));
    }
    let metadata = &info["steps"][1]["result"][0];
    let dims = metadata["pages"]
        .as_array()
        .ok_or_else(|| bad("PDF 页面信息无法读取"))?;
    let rotated: Vec<_> = dims
        .iter()
        .filter(|p| p["rotation"].as_i64().unwrap_or(0) != 0)
        .map(|p| p["page"].clone())
        .collect();
    let heading = regex::Regex::new(r"^(\d{1,3})([.)]?\s+|\()(.+)$").unwrap();
    let body_letters = regex::Regex::new(r"[A-Za-z\u{4e00}-\u{9fff}]").unwrap();
    let marks = regex::Regex::new(r"\[(\d{1,2})\]").unwrap();
    let mut candidates: Vec<Value> = Vec::new();
    let mut current: Option<usize> = None;
    let mut total = 0usize;
    let mut empty = Vec::new();
    let mut corrupt = Vec::new();
    let mut unassigned = Vec::new();
    let mut warnings = vec![
        "原生文字只用于低可信度草稿提示，题号、范围、分值与 QP/MS 对应关系仍需独立核对。"
            .to_owned(),
        "预览保留完整原页，可能含相邻题目或共用材料；尚未执行 OCR 或 AI 识别。".to_owned(),
    ];
    'pages: for start in (1..=count).step_by(20) {
        let end = (start + 19).min(count);
        let mut steps = vec![json!({"tool":"doc_open","args":{"path":path}})];
        for page in start..=end {
            steps.push(json!({"tool":"text_lines","args":{"doc":1,"page":page}}));
        }
        let data = report(app, json!(steps)).await?;
        for page in start..=end {
            if total >= 1_000_000 {
                warnings.push("已达到文字预检上限，其余页面请直接核对原卷。".into());
                unassigned.extend(page..=count);
                break 'pages;
            }
            let raw = &data["steps"][page - start + 1]["result"][0];
            let mut lines: Vec<(f64, f64, f64, String)> = raw["lines"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|l| {
                    Some((
                        l["rect"][1].as_f64()?,
                        l["rect"][0].as_f64()?,
                        l["rect"][3].as_f64()?,
                        l["text"].as_str()?.trim().to_owned(),
                    ))
                })
                .filter(|(_, _, _, s)| !s.is_empty())
                .collect();
            lines.sort_by(|a, b| {
                (a.0 / 3.0)
                    .round()
                    .total_cmp(&(b.0 / 3.0).round())
                    .then(a.1.total_cmp(&b.1))
            });
            let mut joined: Vec<(f64, f64, f64, String)> = Vec::new();
            for l in lines {
                if let Some(prev) = joined.last_mut().filter(|p| (p.0 - l.0).abs() < 2.0) {
                    prev.3.push(' ');
                    prev.3.push_str(&l.3);
                    prev.2 = prev.2.max(l.2);
                } else {
                    joined.push(l);
                }
            }
            let text = joined
                .iter()
                .map(|l| l.3.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let chars = text.chars().count();
            if chars == 0 {
                empty.push(page);
                unassigned.push(page);
                current = None;
                continue;
            }
            if text
                .chars()
                .filter(|c| c.is_control() && !['\n', '\r', '\t'].contains(c))
                .count() as f64
                / chars as f64
                > 0.02
            {
                corrupt.push(page);
            }
            let width = dims[page - 1]["width"].as_f64().unwrap_or(595.0);
            let height = dims[page - 1]["height"].as_f64().unwrap_or(842.0);
            let remaining = (1_000_000 - total).min(40_000);
            total += chars.min(remaining);
            let mut consumed = 0;
            for (y, x, _, content) in joined {
                if consumed >= remaining {
                    break;
                }
                let content = content
                    .chars()
                    .take(remaining - consumed)
                    .collect::<String>();
                consumed += content.chars().count() + 1;
                let matched = heading.captures(&content).filter(|m| {
                    x <= width * 0.30
                        && height * 0.045 <= y
                        && y < height * 0.94
                        && (body_letters.is_match(&m[3]) || &m[2] == "(")
                });
                if let Some(m) = matched {
                    let no = &m[1];
                    if current.is_none_or(|i| candidates[i]["question_no"] != no) {
                        if candidates.len() >= 2000 {
                            warnings.push("已达到草稿数量上限，请手动核对其余页面。".into());
                            break 'pages;
                        }
                        candidates.push(json!({"question_no":no,"qp_pages":[page],"ms_pages":[],"text":content,"marks":null,"topics":[],"confidence":"low","needs_review":true,"detection":"native_text_heading"}));
                        current = Some(candidates.len() - 1);
                        continue;
                    }
                }
                if let Some(i) = current {
                    if !candidates[i]["qp_pages"]
                        .as_array()
                        .unwrap()
                        .contains(&json!(page))
                    {
                        candidates[i]["qp_pages"]
                            .as_array_mut()
                            .unwrap()
                            .push(json!(page));
                    }
                    let mut t = candidates[i]["text"].as_str().unwrap().to_owned();
                    t.push('\n');
                    t.push_str(&content);
                    candidates[i]["text"] = json!(t);
                } else {
                    if !unassigned.contains(&page) {
                        unassigned.push(page);
                    }
                }
            }
        }
    }
    for c in &mut candidates {
        let found: Vec<i64> = marks
            .captures_iter(c["text"].as_str().unwrap_or(""))
            .filter_map(|m| m[1].parse().ok())
            .collect();
        if !found.is_empty() {
            c["marks"] = json!(found.iter().sum::<i64>());
        }
    }
    if candidates.is_empty() {
        for page in 1..=count {
            candidates.push(json!({"question_no":format!("page-{page}"),"qp_pages":[page],"ms_pages":[],"text":"","marks":null,"topics":[],"confidence":"none","needs_review":true,"detection":"manual_page_review"}));
        }
        warnings.push("未定位到可靠题号，页面占位条目不能直接当作题目发布。".into());
    } else {
        for page in &empty {
            candidates.push(json!({"question_no":format!("page-{page}"),"qp_pages":[page],"ms_pages":[],"text":"","marks":null,"topics":[],"confidence":"none","needs_review":true,"detection":"manual_page_review"}));
        }
    }
    if !corrupt.is_empty() {
        warnings.push(format!("第 {corrupt:?} 页文字层编码不可靠，需要视觉核对。"));
    }
    if !empty.is_empty() {
        warnings.push(format!("第 {empty:?} 页没有可用原生文字，尚未执行 OCR。"));
    }
    if !rotated.is_empty() {
        warnings.push(format!(
            "检测到旋转页 {rotated:?}；位置使用引擎的显示坐标，原卷保持不变。"
        ));
    }
    Ok(
        json!({"pages":count,"encrypted":false,"candidates":candidates,"warnings":warnings,"status":"needs_review","text_chars":total,"unassigned_pages":unassigned,"rotation_pages":rotated,"ocr_performed":false,"native_text_quality":if !corrupt.is_empty(){"corrupt"}else if !empty.is_empty(){"missing_or_partial"}else{"available_unverified"},"needs_visual":!corrupt.is_empty()||!empty.is_empty(),"corrupt_text_pages":corrupt,"engine":"Rust + PdfCraft 0.20.0"}),
    )
}
pub(crate) async fn stimulus_available(app: &App, id: &str) -> Result<bool, ApiError> {
    let path = source(app, id)?;
    let dir = app.data.join("renders");
    std::fs::create_dir_all(&dir).map_err(internal)?;
    let cache = dir.join(format!("stimulus-{id}-pdfcraft-0.20.0.json"));
    if let Ok(bytes) = std::fs::read(&cache) {
        if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
            if let Some(ok) = v["available"].as_bool() {
                return Ok(ok);
            }
        }
    }
    let output = run(app, &["text".into(), path.into_os_string()]).await?;
    let text = String::from_utf8_lossy(&output.stdout).to_lowercase();
    let available =
        text.len() > 300 && !text.contains("content removed") && !text.contains("uhpryhg");
    std::fs::write(cache,serde_json::to_vec(&json!({"available":available,"engine":"PdfCraft 0.20.0","basis":"Full source text checked for the original omitted-content markers; uncertain text excluded."})).map_err(internal)?).map_err(internal)?;
    Ok(available)
}
pub async fn focus(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let q = get_question(&app, &id)?;
    let number = q["question_no"]
        .as_str()
        .unwrap_or("")
        .split('(')
        .next()
        .unwrap_or("");
    let page = q["qp_pages"]
        .as_array()
        .and_then(|p| p.first())
        .and_then(Value::as_u64);
    let Some(page) = page else {
        return focus_fallback(&app, &q).await;
    };
    if number.parse::<u16>().is_err() || page == 0 || page > 500 {
        return focus_fallback(&app, &q).await;
    }
    let Some(file_id) = q["qp_file_id"].as_str() else {
        return focus_fallback(&app, &q).await;
    };
    let input = source(&app, file_id)?;
    let dir = app.data.join("renders");
    std::fs::create_dir_all(&dir).map_err(internal)?;
    let script = dir.join(format!("focus-{}.json", uuid::Uuid::new_v4()));
    let steps = json!([{"tool":"doc_open","args":{"path":input}},{"tool":"text_find","args":{"doc":1,"query":number,"start_page":page,"limit":500}}]);
    std::fs::write(&script, serde_json::to_vec(&steps).map_err(internal)?).map_err(internal)?;
    let result = run(
        &app,
        &[
            "run".into(),
            "--script".into(),
            script.clone().into_os_string(),
            "--report".into(),
        ],
    )
    .await;
    let _ = std::fs::remove_file(&script);
    let Ok(output) = result else {
        return focus_fallback(&app, &q).await;
    };
    let report: Value = serde_json::from_slice(&output.stdout).map_err(internal)?;
    let found = &report["steps"][1]["result"][0];
    if found["complete"] != true || found["warning_count"].as_u64().unwrap_or(1) > 0 {
        return focus_fallback(&app, &q).await;
    }
    let matches: Vec<&Value> = found["matches"]
        .as_array()
        .map(|ms| {
            ms.iter()
                .filter(|m| {
                    m["page"].as_u64() == Some(page)
                        && m["text"].as_str() == Some(number)
                        && m["rects"].as_array().is_some_and(|r| r.len() == 1)
                        && m["rects"][0][0]
                            .as_f64()
                            .is_some_and(|x| (20.0..=100.0).contains(&x))
                        && m["rects"][0][1].as_f64().is_some_and(|y| y > 36.0)
                })
                .collect()
        })
        .unwrap_or_default();
    if matches.len() != 1 {
        return focus_fallback(&app, &q).await;
    }
    // A preview can trim before the next unique question label on this same
    // page; otherwise retain the page below this start, including diagrams.
    let extra = report_page_geometry(&app, &input, page, number).await;
    let mut result = json!({"located":true,"page":page,"questionNo":number,"y":matches[0]["rects"][0][1],"renderDpi":192,"method":"unique_source_text_position"});
    if let Ok((width, height, end)) = extra {
        result["pageWidth"] = json!(width);
        result["pageHeight"] = json!(height);
        result["endY"] = json!(end);
    }
    Ok(Json(result))
}
async fn focus_fallback(app: &App, q: &Value) -> Result<Json<Value>, ApiError> {
    let fallback = async {
        let id = q["qp_file_id"].as_str().ok_or_else(|| bad("缺少原卷"))?;
        let path = source(app, id)?;
        native_assembler::preview_focus(app, q, &path).await
    };
    match tokio::time::timeout(std::time::Duration::from_secs(8), fallback).await {
        Ok(Ok(v)) => Ok(Json(v)),
        _ => Ok(Json(json!({"located":false}))),
    }
}
async fn report_page_geometry(
    app: &App,
    input: &std::path::Path,
    page: u64,
    number: &str,
) -> Result<(f64, f64, f64), ApiError> {
    let next = number.parse::<u32>().map_err(internal)? + 1;
    let r=report(app,json!([
        {"tool":"doc_open","args":{"path":input}},
        {"tool":"doc_info","args":{"doc":1}},
        {"tool":"text_find","args":{"doc":1,"query":next.to_string(),"start_page":page,"limit":500}}
    ])).await?;
    let p = &r["steps"][1]["result"][0]["pages"][page as usize - 1];
    let w = p["width"].as_f64().ok_or_else(|| bad("原页宽度缺失"))?;
    let h = p["height"].as_f64().ok_or_else(|| bad("原页高度缺失"))?;
    let found = &r["steps"][2]["result"][0];
    let ys = found["matches"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| {
            m["page"] == page
                && m["text"] == next.to_string()
                && m["rects"].as_array().is_some_and(|r| r.len() == 1)
                && m["rects"][0][0]
                    .as_f64()
                    .is_some_and(|x| (20.0..=100.0).contains(&x))
        })
        .filter_map(|m| m["rects"][0][1].as_f64())
        .collect::<Vec<_>>();
    let end =
        if found["complete"] == true && found["warning_count"].as_u64() == Some(0) && ys.len() == 1
        {
            (ys[0] - 7.).max(0.)
        } else {
            h
        };
    Ok((w, h, end))
}
pub async fn info(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    let path = source(&app, &id)?;
    let out = run(&app, &["info".into(), path.into_os_string()]).await?;
    let info: Value = serde_json::from_slice(&out.stdout).map_err(internal)?;
    Ok(Json(
        json!({"pages":info["pdf_pages"].as_u64().or_else(||info["pages"].as_u64()),"engine":"PdfCraft","version":"0.20.0"}),
    ))
}
pub async fn render(
    State(app): State<App>,
    Path((id, page)): Path<(String, u32)>,
) -> Result<Response, ApiError> {
    if !(1..=500).contains(&page) {
        return Err(bad("页码必须在 1–500 之间"));
    }
    let path = source(&app, &id)?;
    let cache = app.data.join("renders");
    std::fs::create_dir_all(&cache).map_err(internal)?;
    let output = cache.join(format!("{id}-{page}-pdfcraft-0.20.0-192dpi.png"));
    if !output.is_file() {
        let tmp = cache.join(format!("{}-{}.png", id, uuid::Uuid::new_v4()));
        let result = run(
            &app,
            &[
                "render".into(),
                path.into_os_string(),
                "--page".into(),
                page.to_string().into(),
                "--dpi".into(),
                "192".into(),
                "--out".into(),
                tmp.clone().into_os_string(),
            ],
        )
        .await;
        if let Err(error) = result {
            let _ = std::fs::remove_file(&tmp);
            return Err(error);
        }
        std::fs::rename(&tmp, &output).map_err(internal)?;
    }
    let data = std::fs::read(output).map_err(internal)?;
    Ok((
        [
            ("content-type", "image/png"),
            ("cache-control", "private, max-age=86400"),
        ],
        data,
    )
        .into_response())
}
