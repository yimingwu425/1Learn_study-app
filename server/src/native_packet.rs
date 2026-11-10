//! Full-source-page fallback, preserving the explicit distinction from exact crops.
use super::*;
use pdf_document::{step, text, wrap, write_text_pdf};
use std::io::Write;
pub fn zip_bytes(files: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>, ApiError> {
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in files {
        out.start_file(name, opts).map_err(internal)?;
        out.write_all(&data).map_err(internal)?;
    }
    Ok(out.finish().map_err(internal)?.into_inner())
}
fn pages(v: &Value, required: bool) -> Result<Vec<u64>, ApiError> {
    let a = v.as_array().ok_or_else(|| bad("原卷页码应为数组"))?;
    let p = a
        .iter()
        .map(|n| {
            n.as_u64()
                .filter(|n| *n > 0)
                .ok_or_else(|| bad("页码应为正整数"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if (required && p.is_empty())
        || p.iter().collect::<std::collections::HashSet<_>>().len() != p.len()
    {
        return Err(bad("原卷页码为空或重复"));
    }
    Ok(p)
}
pub async fn export(app: &App, manifest: &Value) -> Result<Vec<u8>, ApiError> {
    let mode = manifest["mode"].as_str().unwrap_or("");
    if !["practice", "study"].contains(&mode) {
        return Err(bad("导出模式无效"));
    }
    let qs = manifest["questions"]
        .as_array()
        .filter(|q| !q.is_empty() && q.len() <= 200)
        .ok_or_else(|| bad("需要 1–200 道题"))?;
    let work = tempfile::tempdir_in(app.data.join("exports")).map_err(internal)?;
    let mut sources = std::collections::HashMap::new();
    let mut total = 0;
    let mut seen = std::collections::HashSet::new();
    for q in qs {
        let id = q["id"].as_str().ok_or_else(|| bad("题号无效"))?;
        if !seen.insert(id) {
            return Err(bad("重复选题"));
        }
        for role in ["qp", "ms"] {
            let pp = pages(&q[format!("{role}_pages")], role == "qp")?;
            let path = q[format!("{role}_path")].as_str();
            if !pp.is_empty() && path.is_none() {
                return Err(bad("缺少原 PDF"));
            }
            if let Some(path) = path {
                if pp.is_empty() {
                    return Err(bad("缺少原页码"));
                }
                if !sources.contains_key(path) {
                    let info = pdf_document::info(app, std::path::Path::new(path)).await?;
                    sources.insert(
                        path.to_owned(),
                        info["pages"]
                            .as_array()
                            .ok_or_else(|| bad("无法读取页数"))?
                            .len(),
                    );
                }
                if pp.iter().any(|n| *n > sources[path] as u64) {
                    return Err(bad("所选页码超出原卷"));
                }
                total += 1 + pp.len();
            }
        }
    }
    if total > 1000 {
        return Err(bad("导出超过 1000 页上限"));
    }
    let mut provenance = vec![];
    let mut missing = vec![];
    let mut files = vec![];
    for role in ["qp", "ms"] {
        let mut parts = vec![];
        let mut page_count = 0;
        for (i, q) in qs.iter().enumerate() {
            let Some(source) = q[format!("{role}_path")].as_str() else {
                if role == "ms" {
                    missing.push(q["id"].clone())
                }
                continue;
            };
            let pp = pages(&q[format!("{role}_pages")], role == "qp")?;
            let cover = work.path().join(format!("{role}-{i}-divider.pdf"));
            let extract = work.path().join(format!("{role}-{i}-source.pdf"));
            let body=format!("{} {}\n\nQuestion: {}\nSource: {}\nOriginal page(s): {}\n\nThe following pages are complete pages from the original PDF.\nThey may contain adjacent questions, shared material or other answers.\nWork only on the identified question. These are not exact question crops.{}",if role=="qp"{"Practice question"}else{"Official answer reference"},i+1,q["question_no"].as_str().unwrap_or(""),q["source"].as_str().unwrap_or(""),pp.iter().map(u64::to_string).collect::<Vec<_>>().join(", "),if role=="qp"&&mode=="study"&&!q["note"].as_str().unwrap_or("").is_empty(){format!("\n\nMy note:\n{}",q["note"].as_str().unwrap())}else{String::new()});
            let lines = wrap(&body, 499., 11.);
            if lines.len() > 48 {
                return Err(bad("笔记或来源信息过长，无法放入来源页，请缩短"));
            }
            write_text_pdf(
                app,
                &cover,
                "Local study packet",
                &[lines
                    .iter()
                    .enumerate()
                    .map(|(i, l)| text(48., 60. + i as f32 * 15.4, 11., l))
                    .collect()],
            )?;
            pdf_engine::report(
                app,
                json!([
                    step("doc_open", json!({"path":source})),
                    step("page_extract", json!({"doc":1,"pages":pp,"out":extract}))
                ]),
            )
            .await?;
            pdf_document::strip_interactive(&extract)?;
            parts.extend([cover, extract]);
            if role == "qp" {
                provenance.push(json!({"id":q["id"],"source":q["source"],"question_no":q["question_no"],"qp_file":std::path::Path::new(source).file_name().unwrap().to_string_lossy(),"qp_pages":pp,"ms_file":q["ms_path"].as_str().and_then(|s|std::path::Path::new(s).file_name()).map(|s|s.to_string_lossy()),"ms_pages":q["ms_pages"],"qp_packet_pages":(page_count+2..page_count+2+pp.len()).collect::<Vec<_>>(),"page_mapping":"complete_source_pages"}));
            } else {
                provenance[i]["ms_packet_pages"] =
                    json!((page_count + 2..page_count + 2 + pp.len()).collect::<Vec<_>>());
            }
            page_count += 1 + pp.len();
        }
        if !parts.is_empty() {
            let out = work.path().join(format!("{role}.pdf"));
            let mut args = vec!["combine".into()];
            args.extend(parts.iter().map(|p| p.clone().into_os_string()));
            args.extend(["--out".into(), out.clone().into_os_string()]);
            pdf_engine::run(app, &args).await?;
            files.push((
                if role == "qp" {
                    "student.pdf"
                } else {
                    "answers.pdf"
                }
                .into(),
                std::fs::read(out).map_err(internal)?,
            ));
        }
    }
    files.push(("provenance.json".into(),serde_json::to_vec_pretty(&json!({"version":1,"mode":mode,"questions":provenance,"missing_answers":missing,"engine":"Rust + PdfCraft"})).map_err(internal)?));
    files.push(("README.txt".into(),format!("LOCAL STUDY PACKET\n\nFull source pages, with diagrams and original rotation. They may contain adjacent questions, shared material or other answers. Follow each divider. No answer text has been invented.\nstudent.pdf: questions. answers.pdf: supplied MS pages. provenance.json: source and output page mapping. Original PDFs unchanged.\nMode: {mode}; practice mode hides notes.\nMissing answers: {}\n",json!(missing)).into_bytes()));
    zip_bytes(files)
}
