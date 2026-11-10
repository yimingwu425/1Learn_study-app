//! Vector assembly delegates parsing, redaction, cropping, stamping and flattening to PdfCraft.
use super::*;
use bank::{number as num, text as s};
use pdf_document::{step, text, wrap, write_text_pdf, Document, Page, Rect};
use std::collections::{HashMap, HashSet};
fn anchors(p: &Page, kind: &str) -> Vec<(u32, Rect, String)> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"^(\d+)((?:\([a-z0-9]+\))*)\.?$").unwrap());
    let (top, bottom) = bounds(p);
    let mut a = p
        .lines
        .iter()
        .filter_map(|l| {
            let c = re.captures(l.text.trim())?;
            let n = c[1].parse().ok()?;
            let suffix = c[2].to_owned();
            (l.rect.0 >= 30.
                && l.rect.0 <= if kind == "ms" { 110. } else { 85. }
                && l.rect.1 > top
                && l.rect.1 < bottom
                && (kind != "qp" || suffix.is_empty()))
            .then_some((n, l.rect, suffix))
        })
        .collect::<Vec<_>>();
    a.sort_by(|a, b| a.1 .1.total_cmp(&b.1 .1).then(a.1 .0.total_cmp(&b.1 .0)));
    a
}
fn bounds(p: &Page) -> (f64, f64) {
    let mut bottom = p.height - 55.;
    for l in &p.lines {
        if [
            "©",
            "Permission to reproduce",
            "BLANK PAGE",
            "Copyright Acknowledgements",
        ]
        .iter()
        .any(|s| l.text.starts_with(s))
            && l.rect.1 > 100.
        {
            bottom = bottom.min(l.rect.1 - 8.);
        }
    }
    (50., bottom)
}
fn regions(
    doc: &Document,
    m: &Value,
    kind: &str,
    mcq: bool,
) -> Result<Vec<(usize, Rect)>, ApiError> {
    let reference = s(m, "questionRef");
    let n = reference
        .split('(')
        .next()
        .unwrap_or("")
        .parse::<u32>()
        .map_err(|_| bad("原题号不是有效数字"))?;
    let hint = m[if kind == "qp" {
        "qpPdfPage"
    } else {
        "msPdfPage"
    }]
    .as_u64()
    .filter(|n| *n > 0)
    .ok_or_else(|| bad("缺少可靠原页码"))? as usize
        - 1;
    if hint >= doc.pages.len() {
        return Err(bad(format!(
            "{} {reference}: {kind} 原页码越界",
            s(m, "paperId")
        )));
    }
    let hits = doc
        .pages
        .iter()
        .enumerate()
        .skip(1)
        .flat_map(|(p, page)| {
            anchors(page, kind)
                .into_iter()
                .filter(move |a| a.0 == n)
                .map(move |(_, r, suffix)| (p, r, suffix))
        })
        .collect::<Vec<_>>();
    let mut chosen = if kind == "ms" {
        let exact = hits
            .iter()
            .filter(|(_, _, sf)| format!("{n}{sf}") == reference)
            .cloned()
            .collect::<Vec<_>>();
        let structured = hits
            .iter()
            .filter(|(_, _, sf)| !sf.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        if !exact.is_empty() {
            exact
        } else if !structured.is_empty() {
            structured
        } else {
            hits.into_iter().filter(|(p, _, _)| *p >= hint).collect()
        }
    } else {
        hits
    };
    if kind == "qp" {
        chosen.sort_by_key(|(p, _, _)| (*p != hint, p.abs_diff(hint), *p));
    }
    let (start, first, _) = chosen.first().cloned().ok_or_else(|| {
        bad(format!(
            "{} Q{n}: 无法可靠定位 {kind} 题目边界",
            s(m, "paperId")
        ))
    })?;
    let mut out = vec![];
    for pi in start..doc.pages.len() {
        let page = &doc.pages[pi];
        let (mut top, mut bottom) = bounds(page);
        if pi == start {
            top = first.1 - 4.;
        }
        let following = anchors(page, kind)
            .into_iter()
            .filter(|(nn, r, _)| *nn != n && r.1 > top)
            .map(|(_, r, _)| r.1 - 5.)
            .reduce(f64::min);
        if let Some(y) = following {
            bottom = bottom.min(y)
        }
        for l in &page.lines {
            if l.rect.1 > top + 12.
                && ((["OR", "EITHER"].contains(&l.text.as_str()) && l.rect.0 < 85.)
                    || (l.text.starts_with("Section ") && l.rect.0 < 350.))
            {
                bottom = bottom.min(l.rect.1 - 5.);
            }
        }
        if bottom > top + 10. {
            let lines = page
                .lines
                .iter()
                .filter(|l| l.rect.1 >= top && l.rect.3 <= bottom)
                .collect::<Vec<_>>();
            if !lines.is_empty() {
                if {
                    let t = lines
                        .iter()
                        .map(|l| l.text.to_lowercase())
                        .collect::<Vec<_>>()
                        .join(" ");
                    t.contains("content removed") || t.contains("uhpryhg")
                } {
                    return Err(bad("原卷共用材料已被删除，不能生成完整试题"));
                }
                if mcq {
                    bottom =
                        bottom.min(lines.iter().map(|l| l.rect.3).reduce(f64::max).unwrap() + 8.);
                }
                let left = if first.0 > 65. { 65. } else { 28. };
                let right = (page.width - 60.)
                    .max(lines.iter().map(|l| l.rect.2).reduce(f64::max).unwrap() + 2.)
                    .min(page.width - 20.);
                out.push((pi, Rect(left, top, right, bottom)));
            }
        }
        if following.is_some() || (bottom < bounds(page).1 - 20. && pi != start) {
            break;
        }
        if let Some(next) = doc.pages.get(pi + 1) {
            if anchors(next, kind).first().is_some_and(|a| a.0 != n) {
                break;
            }
        }
    }
    if out.is_empty() {
        return Err(bad("原题区域为空"));
    }
    Ok(out)
}
fn selected_regions(
    doc: &Document,
    members: &[Value],
    kind: &str,
    complete: bool,
    mcq: bool,
) -> Result<Vec<(usize, Rect)>, ApiError> {
    let slices = regions(doc, &members[0], kind, mcq)?;
    let refs = members
        .iter()
        .map(|m| s(m, "questionRef").to_owned())
        .collect::<HashSet<_>>();
    let n = s(&members[0], "questionRef")
        .split('(')
        .next()
        .unwrap()
        .to_owned();
    if complete || refs.contains(&n) {
        return Ok(slices);
    }
    let ms = regex::Regex::new(&format!(r"^{}((?:\([a-z0-9]+\))+)$", regex::escape(&n))).unwrap();
    let qp = regex::Regex::new(&format!(
        r"^(?:{}\s+)?\(([a-z]+)\)(?:\s*\(([ivx]+)\))?(?:\s|$)",
        regex::escape(&n)
    ))
    .unwrap();
    let mut current = n.clone();
    let mut alpha = String::new();
    let mut found = HashSet::new();
    let mut result: Vec<(usize, Rect)> = vec![];
    for (pi, rect) in slices {
        let page = &doc.pages[pi];
        let mut labels = vec![];
        for l in &page.lines {
            if !rect.contains(l.rect) || l.rect.0 >= 130. {
                continue;
            }
            let reference = if kind == "ms" {
                ms.captures(l.text.trim())
                    .map(|c| format!("{}{}", n, &c[1]))
            } else {
                qp.captures(l.text.trim()).map(|c| {
                    let token = &c[1];
                    if let Some(roman) = c.get(2) {
                        alpha = format!("({token})");
                        return format!("{n}{alpha}({})", roman.as_str());
                    }
                    if token.len() == 1 && !["i", "v", "x"].contains(&token) {
                        alpha = format!("({token})");
                        format!("{n}{alpha}")
                    } else {
                        format!("{n}{alpha}({token})")
                    }
                })
            };
            if let Some(r) = reference {
                labels.push((l.rect.1 - 2., r));
            }
        }
        labels.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        labels.dedup();
        let mut start = rect.1;
        let mut cuts = labels
            .into_iter()
            .map(|(y, r)| (y, Some(r)))
            .collect::<Vec<_>>();
        cuts.push((rect.3, None));
        for (y, reference) in cuts {
            if y > start
                && refs
                    .iter()
                    .any(|r| r.starts_with(&current) || current.starts_with(r))
            {
                let segment = Rect(rect.0, start, rect.2, y);
                let body = page
                    .lines
                    .iter()
                    .filter(|l| segment.contains(l.rect))
                    .filter(|l| {
                        kind != "ms" || !["Question", "Answer", "Marks"].contains(&l.text.as_str())
                    })
                    .map(|l| l.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                if !body.trim().is_empty() {
                    if let Some((old_pi, r)) = result
                        .last_mut()
                        .filter(|(p, r)| *p == pi && (r.3 - segment.1).abs() < 0.1)
                    {
                        let _ = old_pi;
                        r.3 = segment.3;
                    } else {
                        result.push((pi, segment));
                    }
                }
            }
            if let Some(r) = reference {
                current = r.clone();
                found.insert(r);
                start = rect.1.max(y);
            }
        }
    }
    if !refs.is_subset(&found) {
        return Err(bad(format!(
            "{}: 无法可靠定位所选 {kind} 小问 {:?}",
            s(&members[0], "paperId"),
            refs.difference(&found).collect::<Vec<_>>()
        )));
    }
    Ok(result)
}
fn mcq_answer(doc: &Document, m: &Value) -> Result<String, ApiError> {
    let combined = regex::Regex::new(&format!(
        r"^{}\s+([ABCD])(?:\s+1)?$",
        regex::escape(s(m, "questionRef"))
    ))
    .unwrap();
    for p in &doc.pages {
        for l in &p.lines {
            if l.rect.1 > 50. && l.rect.1 < p.height - 55. {
                if let Some(c) = combined.captures(l.text.trim()) {
                    return Ok(c[1].to_owned());
                }
            }

            if l.text.trim() == s(m, "questionRef") && l.rect.1 > 50. && l.rect.1 < p.height - 55. {
                let ans = p
                    .lines
                    .iter()
                    .filter(|a| {
                        ["A", "B", "C", "D"].contains(&a.text.trim())
                            && (a.rect.1 - l.rect.1).abs() < 3.
                            && a.rect.0 > l.rect.2
                            && a.rect.0 < l.rect.2 + 180.
                    })
                    .collect::<Vec<_>>();
                if ans.len() == 1 {
                    return Ok(ans[0].text.trim().into());
                }
            }
        }
    }
    Err(bad(format!(
        "{} Q{}: 不能可靠确定原选择题答案",
        s(m, "paperId"),
        s(m, "questionRef")
    )))
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
/// Rehydrate every source reference from authoritative local records, not plan paths.
fn hydrate(app: &App, plan: &Value) -> Result<Value, ApiError> {
    let subject = s(&plan["filters"], "subject");
    if !["9702", "9708"].contains(&subject) {
        return Err(bad("组卷学科无效"));
    }
    let mut plan = plan.clone();
    let all = all_questions(app)?
        .into_iter()
        .filter(|q| q["subject"] == subject && !q["legacy_metadata"].is_null())
        .map(|q| (format!("{}::{}", s(&q, "source"), s(&q, "question_no")), q))
        .collect::<HashMap<_, _>>();
    let complete = plan["mode"] == "paper";
    let paper = s(&plan, "paperType").to_owned();
    let items = plan["questions"]
        .as_array_mut()
        .filter(|a| !a.is_empty() && a.len() <= 200)
        .ok_or_else(|| bad("组卷需要 1–200 道题"))?;
    let mut seen = HashSet::new();
    for item in items {
        let selected = item["members"]
            .as_array()
            .filter(|a| !a.is_empty())
            .ok_or_else(|| bad("组卷缺少原题成员"))?;
        let first = &selected[0];
        let source = s(first, "paperId");
        let top = s(first, "questionRef").split('(').next().unwrap();
        if selected.iter().any(|m| {
            s(m, "paperId") != source || s(m, "questionRef").split('(').next() != Some(top)
        }) {
            return Err(bad("题组成员不属于同一道原题"));
        }
        let mut keys = if complete {
            all.iter()
                .filter(|(_, q)| {
                    s(q, "source") == source && s(q, "question_no").split('(').next() == Some(top)
                })
                .map(|(k, _)| k.clone())
                .collect::<Vec<_>>()
        } else {
            selected
                .iter()
                .map(|m| format!("{}::{}", s(m, "paperId"), s(m, "questionRef")))
                .collect()
        };
        keys.sort_by(|a, b| bank::natural(a, b));
        let mut members = vec![];
        for k in keys {
            if !seen.insert(k.clone()) {
                return Err(bad("重复选题"));
            }
            let q = all.get(&k).ok_or_else(|| bad("所选原题已不存在"))?;
            let base = if paper.starts_with("paper2") {
                "paper2"
            } else {
                &paper
            };
            if q["paper_type"] != base {
                return Err(bad("所选题目不属于当前卷别"));
            }
            let mut row = composer::row(q);
            for (key, filekey) in [("qpFile", "qp_file_id"), ("msFile", "ms_file_id")] {
                let id = q[filekey].as_str().ok_or_else(|| bad("题目缺少原卷映射"))?;
                uuid::Uuid::parse_str(id).map_err(|_| bad("原卷标识无效"))?;
                let path = app.data.join("files").join(format!("{id}.pdf"));
                if !path.is_file() {
                    return Err(bad("原卷文件缺失"));
                }
                row[key] = json!(path);
            }
            members.push(row)
        }
        if members.is_empty() {
            return Err(bad("未找到完整原题"));
        }
        let marks = members.iter().map(|q| num(q, "marksTotal")).sum::<f64>();
        let mut q = members[0].clone();
        q["marksTotal"] = json!(marks);
        item["members"] = json!(members);
        item["question"] = q;
    }
    composer::revise(&mut plan);
    if complete && plan["exactTarget"] != true {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "题目分值或选答分区不完整，请调整选题",
        ));
    }
    Ok(plan)
}
fn add_heading(pages: &mut Vec<Vec<pdf_document::Text>>, heading: &str) -> usize {
    let index = pages.len();
    pages.push(vec![
        text(44., 35., 10., heading),
        text(290., 818., 9., index + 1),
    ]);
    index
}
async fn prepare(
    app: &App,
    doc: &mut Document,
    members: &[Value],
    kind: &str,
) -> Result<(), ApiError> {
    let reference = s(&members[0], "questionRef");
    let number = reference
        .split('(')
        .next()
        .unwrap()
        .parse::<u32>()
        .map_err(|_| bad("原题号无效"))?;
    let field = if kind == "qp" {
        "qpPdfPage"
    } else {
        "msPdfPage"
    };
    let hint = members
        .iter()
        .filter_map(|m| m[field].as_u64())
        .min()
        .filter(|n| *n > 0)
        .ok_or_else(|| bad("缺少原页码"))? as usize
        - 1;
    if hint >= doc.pages.len() {
        return Err(bad("原页码越界"));
    }
    let mut located = None;
    for distance in 0..doc.pages.len() {
        let candidates = if distance == 0 {
            vec![hint]
        } else {
            [
                hint.checked_add(distance).filter(|p| *p < doc.pages.len()),
                hint.checked_sub(distance).filter(|p| *p >= 1),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
        };
        for page in candidates {
            doc.prepare_page(app, page, kind).await?;
            if anchors(&doc.pages[page], kind)
                .iter()
                .any(|a| a.0 == number)
            {
                located = Some(page);
                break;
            }
        }
        if located.is_some() {
            break;
        }
    }
    let start = located.ok_or_else(|| {
        bad(format!(
            "{} Q{}: 无法可靠定位 {} 题号",
            s(&members[0], "paperId"),
            number,
            kind
        ))
    })?;
    let here = anchors(&doc.pages[start], kind);
    if let Some((_, first, _)) = here.iter().find(|a| a.0 == number) {
        if here.iter().any(|a| a.0 != number && a.1 .1 > first.1) {
            return Ok(());
        }
    }
    // Retain the whole continuation until the next real question, not just the hinted part.
    for page in start + 1..doc.pages.len() {
        doc.prepare_page(app, page, kind).await?;
        if anchors(&doc.pages[page], kind)
            .iter()
            .any(|a| a.0 != number)
        {
            break;
        }
    }
    Ok(())
}
async fn assembled(
    app: &App,
    plan: &Value,
    kind: &str,
    work: &std::path::Path,
    docs: &mut HashMap<PathBuf, Document>,
) -> Result<Vec<u8>, ApiError> {
    let mut pages = vec![];
    let mut stamps: Vec<(usize, PathBuf, Rect)> = vec![];
    let mut numbering: Vec<(usize, pdf_document::Text)> = vec![];
    let mut layout: Option<(usize, f64)> = None;
    let mcq = plan["paperType"] == "paper1";
    let title = s(plan, "title");
    if kind == "qp" {
        let p = add_heading(&mut pages, "Practice examination");
        let title_lines = wrap(title, 507., 22.);
        if title_lines.len() > 4 {
            return Err(bad("试卷标题过长，请缩短"));
        }
        for (i, l) in title_lines.iter().enumerate() {
            pages[p].push(text(44., 132. + i as f32 * 28., 22., l))
        }
        pages[p].extend([
            text(
                44.,
                260.,
                12.,
                format!(
                    "Subject: {}    Questions: {}    Answer marks: {}",
                    s(&plan["filters"], "subject"),
                    plan["questions"].as_array().unwrap().len(),
                    plan["actualMarks"]
                ),
            ),
            text(
                44.,
                285.,
                12.,
                format!("Printed marks: {}", plan["printedMarks"]),
            ),
            text(
                44.,
                330.,
                12.,
                "Candidate name: ___________________________________",
            ),
            text(
                44.,
                370.,
                12.,
                "Centre number: __________    Candidate number: __________",
            ),
            text(
                44.,
                540.,
                11.,
                "Use a separate answer booklet where answer space is not provided.",
            ),
        ]);
        let sections = plan["answerSections"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if sections.is_empty() {
            pages[p].push(text(
                44.,
                460.,
                12.,
                "Answer all questions. Marks are shown in brackets.",
            ));
            if mcq {
                pages[p].push(text(
                    44.,
                    485.,
                    11.,
                    format!(
                        "{} multiple-choice questions. Each correct answer scores 1 mark.",
                        plan["questions"].as_array().unwrap().len()
                    ),
                ))
            }
        } else {
            let mut first = 1;
            for (i, sec) in sections.iter().enumerate() {
                let last = first + sec["questionCount"].as_u64().unwrap() - 1;
                pages[p].push(text(
                    44.,
                    430. + i as f32 * 25.,
                    11.,
                    format!(
                        "Section {} ({}): answer {} of Questions {}-{}.",
                        s(sec, "id"),
                        s(sec, "label"),
                        sec["answerCount"],
                        first,
                        last
                    ),
                ));
                first = last + 1;
            }
        }
    }
    let mut answer_layout: Option<(usize, f32)> = None;
    for (index, item) in plan["questions"].as_array().unwrap().iter().enumerate() {
        let members = item["members"].as_array().unwrap();
        let m = &members[0];
        let number = index + 1;
        let marks = num(&item["question"], "marksTotal");
        let path = PathBuf::from(s(m, if kind == "qp" { "qpFile" } else { "msFile" }));
        if !docs.contains_key(&path) {
            docs.insert(path.clone(), Document::load(app, &path, false).await?);
        }
        if kind == "ms" && mcq {
            let doc = docs.get_mut(&path).unwrap();
            for page in 0..doc.pages.len() {
                doc.prepare_page(app, page, kind).await?;
            }
        } else {
            prepare(app, docs.get_mut(&path).unwrap(), members, kind).await?;
        }
        let src = &docs[&path];
        if kind == "ms" && mcq {
            let (p, y) = match answer_layout {
                Some((p, y)) if y <= 760. => (p, y),
                _ => (
                    add_heading(&mut pages, "Mark scheme - multiple choice"),
                    80.,
                ),
            };
            pages[p].push(text(
                44.,
                y,
                11.,
                format!(
                    "{number:>2}    {}    [{marks}]     {} Q{}",
                    mcq_answer(src, m)?,
                    s(m, "paperId"),
                    s(m, "questionRef")
                )
                .replace(":g", ""),
            ));
            answer_layout = Some((p, y + 22.));
            continue;
        }
        let mut slices = selected_regions(src, members, kind, plan["mode"] == "paper", mcq)?;
        let selected_text = slices
            .iter()
            .map(|(p, r)| src.pages[*p].text(Some(*r)))
            .collect::<Vec<_>>()
            .join("\n");
        let mut extras = vec![];
        if kind == "ms" {
            let re = regex::Regex::new(r"Table [A-Z]").unwrap();
            let tables = re
                .find_iter(&selected_text)
                .map(|m| m.as_str().to_owned())
                .collect::<HashSet<_>>();
            for table in tables {
                for (p, page) in src.pages.iter().enumerate() {
                    if page.lines.iter().any(|l| {
                        l.text.starts_with(&format!("{table}:")) && l.rect.0 < 80. && l.rect.1 < 80.
                    }) {
                        extras.push((
                            p,
                            Rect(65., 115., page.width - 60., bounds(page).1),
                            table.clone(),
                        ));
                    }
                }
            }
        }
        for (part, (pi, rect)) in slices.drain(..).enumerate() {
            let scale = (507. / rect.width()).min(725. / rect.height()).min(1.1);
            let height = rect.height() * scale;
            let (page, top) = if kind == "qp" && part == 0 {
                match layout {
                    Some((p, y)) if y + height <= 800. => {
                        pages[p].push(text(
                            44.,
                            (y - 30.) as f32,
                            10.,
                            format!("Question {number}   [{marks} marks]"),
                        ));
                        (p, y)
                    }
                    _ => (
                        add_heading(&mut pages, &format!("Question {number}   [{marks} marks]")),
                        65.,
                    ),
                }
            } else {
                (
                    add_heading(
                        &mut pages,
                        &format!(
                            "Question {number}{}",
                            if part > 0 { " (continued)" } else { "" }
                        ),
                    ),
                    65.,
                )
            };
            if kind == "qp" {
                layout = Some((page, top + height + 48.));
            }
            if kind == "ms" {
                pages[page].push(text(
                    44.,
                    50.,
                    8.,
                    format!(
                        "Source: {} Q{}",
                        s(m, "paperId"),
                        s(m, "questionRef").split('(').next().unwrap()
                    ),
                ))
            }
            let clip = work.join(format!("{kind}-{index}-{part}-clip.pdf"));
            clip_source(app, src, pi, rect, kind, &clip).await?;
            for (_, ar, suffix) in anchors(&src.pages[pi], kind) {
                if rect.contains(ar) {
                    numbering.push((
                        page,
                        text(
                            (44. + (ar.0 - rect.0) * scale) as f32,
                            (top + (ar.3 - rect.1) * scale - 3.) as f32,
                            (10. * scale) as f32,
                            format!("{number}{suffix}"),
                        ),
                    ));
                }
            }
            stamps.push((
                page,
                clip,
                Rect(44., top, 44. + rect.width() * scale, top + height),
            ));
        }
        for (extra, (pi, rect, table)) in extras.into_iter().enumerate() {
            let p = add_heading(&mut pages, &format!("Question {number} - {table}"));
            pages[p].push(text(
                44.,
                50.,
                8.,
                format!("{} - {table}: levels for this question", s(m, "paperId")),
            ));
            let clip = work.join(format!("{kind}-{index}-table{extra}.pdf"));
            clip_source(app, src, pi, rect, "table", &clip).await?;
            let scale = (507. / rect.width()).min(725. / rect.height()).min(1.1);
            stamps.push((
                p,
                clip,
                Rect(
                    44.,
                    65.,
                    44. + rect.width() * scale,
                    65. + rect.height() * scale,
                ),
            ));
        }
    }
    let base = work.join(format!("{kind}-headings.pdf"));
    write_text_pdf(app, &base, title, &pages)?;
    let result = work.join(format!("{kind}.pdf"));
    let mut steps = vec![step("doc_open", json!({"path":base}))];
    for (page, path, rect) in stamps {
        steps.push(step(
            "stamp_custom",
            json!({"doc":1,"page":page+1,"path":path,"rect":rect.value()}),
        ));
    }
    steps.push(step("doc_flatten", json!({"doc":1})));
    for (page, t) in numbering {
        steps.push(step("page_add_text",json!({"doc":1,"page":page+1,"at":[t.x,t.y-t.size*0.8],"width":80,"text":t.text,"size":t.size})));
    }
    steps.push(step("doc_save", json!({"doc":1,"path":result,"full":true})));
    pdf_engine::report(app, json!(steps)).await?;
    Ok(std::fs::read(result).map_err(internal)?)
}
async fn clip_source(
    app: &App,
    src: &Document,
    page: usize,
    rect: Rect,
    kind: &str,
    out: &std::path::Path,
) -> Result<(), ApiError> {
    let p = &src.pages[page];
    let mut steps = vec![step("doc_open", json!({"path":src.path}))];
    let whole = p.bounds();
    for r in [
        Rect(0., 0., p.width, rect.1),
        Rect(0., rect.3, p.width, p.height),
        Rect(0., rect.1, rect.0, rect.3),
        Rect(rect.2, rect.1, p.width, rect.3),
    ] {
        if r.width() > 0. && r.height() > 0. && whole.contains(r) {
            steps.push(step(
                "redact_mark",
                json!({"doc":1,"page":page+1,"rect":r.value(),"fill":"white"}),
            ));
        }
    }
    if kind != "table" {
        for (_, r, _) in anchors(p, kind) {
            if rect.contains(r) {
                steps.push(step(
                    "redact_mark",
                    json!({"doc":1,"page":page+1,"rect":r.value(),"fill":"white"}),
                ));
            }
        }
    }
    steps.extend([
        step("redact_apply", json!({"doc":1,"pages":[page+1]})),
        step(
            "page_set_box",
            json!({"doc":1,"pages":[page+1],"box":"crop","rect":rect.value()}),
        ),
        step("page_extract", json!({"doc":1,"pages":[page+1],"out":out})),
    ]);
    pdf_engine::report(app, json!(steps)).await?;
    pdf_document::strip_interactive(out)?;
    pdf_document::tighten_slice_media(out)?;
    Ok(())
}
pub async fn bundle(app: &App, plan: &Value) -> Result<Vec<u8>, ApiError> {
    static SLOTS: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    let _slot = SLOTS
        .get_or_init(|| tokio::sync::Semaphore::new(2))
        .acquire()
        .await
        .map_err(internal)?;
    tokio::time::timeout(std::time::Duration::from_secs(150), build_bundle(app, plan))
        .await
        .map_err(|_| {
            err(
                StatusCode::REQUEST_TIMEOUT,
                "PDF 导出超时，请减少选题或核对原卷",
            )
        })?
}
async fn build_bundle(app: &App, plan: &Value) -> Result<Vec<u8>, ApiError> {
    let plan = hydrate(app, plan)?;
    let work = tempfile::tempdir_in(app.data.join("exports")).map_err(internal)?;
    let mut docs = HashMap::new();
    let qp = assembled(app, &plan, "qp", work.path(), &mut docs).await?;
    let ms = assembled(app, &plan, "ms", work.path(), &mut docs).await?;
    let questions = plan["questions"].as_array().unwrap();
    let manifest = json!({"subject":plan["filters"]["subject"],"mode":plan["mode"],"paperType":plan["paperType"],"targetMarks":plan["targetMarks"],"actualMarks":plan["actualMarks"],"printedMarks":plan["printedMarks"],"actualQuestions":questions.len(),"answerSections":plan["answerSections"],"questions":questions.iter().enumerate().map(|(i,q)|json!({"number":i+1,"marks":q["question"]["marksTotal"],"members":q["members"].as_array().unwrap().iter().map(|m|json!({"paperId":m["paperId"],"questionRef":m["questionRef"]})).collect::<Vec<_>>()})).collect::<Vec<_>>(),"pdfEngine":"Rust + PdfCraft 0.20.0","sourceFilesUnchanged":true,"boundaryDetection":"Native source geometry; local Tesseract where needed, not AI recognition","warnings":plan["warnings"]});
    let rows = questions
        .iter()
        .enumerate()
        .map(|(i, q)| {
            format!(
                "<tr><td>Q{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                i + 1,
                q["question"]["marksTotal"],
                q["members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| escape(s(m, "knowledgePointCn")))
                    .collect::<Vec<_>>()
                    .join("<br>"),
                q["members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| format!(
                        "{} Q{}",
                        escape(s(m, "paperId")),
                        escape(s(m, "questionRef"))
                    ))
                    .collect::<Vec<_>>()
                    .join("<br>")
            )
        })
        .collect::<String>();
    let html=format!("<!doctype html><meta charset=\"utf-8\"><title>知识点与答案报告</title><style>body{{font-family:Arial,'PingFang SC',sans-serif;margin:32px}}table{{border-collapse:collapse;width:100%}}th,td{{border:1px solid #bbb;padding:8px;text-align:left}}</style><h1>知识点与答案报告</h1><p>答案详见教师评分卷。Source PDFs are unchanged.</p><table><tr><th>题号</th><th>分值</th><th>知识点</th><th>答案来源</th></tr>{rows}</table>");
    let sources = questions
        .iter()
        .enumerate()
        .map(|(i, q)| {
            format!(
                "Q{} [{}] {}",
                i + 1,
                q["question"]["marksTotal"],
                q["members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| format!("{} {}", s(m, "paperId"), s(m, "questionRef")))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    native_packet::zip_bytes(vec![
        ("study-student-exam-paper.pdf".into(), qp),
        ("study-teacher-mark-scheme.pdf".into(), ms),
        (
            "study-manifest.json".into(),
            serde_json::to_vec_pretty(&manifest).map_err(internal)?,
        ),
        ("study-knowledge-report.html".into(), html.into_bytes()),
        ("study-sources.txt".into(), sources.into_bytes()),
    ])
}

/// Read-only picture positioning reuses the tested source geometry, never the
/// legacy full-page text fragment. Ambiguous labels return to the complete page.
pub async fn preview_focus(
    app: &App,
    q: &Value,
    path: &std::path::Path,
) -> Result<Value, ApiError> {
    if q["legacy_metadata"].is_null() {
        return Err(bad("此题需从原页核对"));
    }
    let member = composer::row(q);
    let number = s(&member, "questionRef")
        .split('(')
        .next()
        .unwrap_or("")
        .parse::<u32>()
        .map_err(|_| bad("题号无效"))?;
    let mut doc = Document::load(app, path, false).await?;
    prepare(app, &mut doc, &[member.clone()], "qp").await?;
    let slices = regions(&doc, &member, "qp", q["paper_type"] == "paper1")?;
    let (page, rect) = slices[0];
    let p = &doc.pages[page];
    if anchors(p, "qp").iter().filter(|a| a.0 == number).count() != 1 {
        return Err(bad("原页题号存在歧义"));
    }
    Ok(
        json!({"located":true,"page":page+1,"questionNo":q["question_no"],"y":rect.1,"endY":rect.3,"pageWidth":p.width,"pageHeight":p.height,"renderDpi":192,"method":"source_geometry_position","needsReview":true,"continuationPages":slices.iter().map(|(p,_)|p+1).collect::<Vec<_>>()}),
    )
}
