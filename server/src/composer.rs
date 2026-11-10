//! Native selection, syllabus allocation and whole-question/choice-section rules.
//! PDF parsing and rendering stay in the existing engine adapters.
use super::*;
use bank::{natural, number, text};
use std::collections::{BTreeMap, HashMap, HashSet};
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Bucket {
    key: String,
    label: String,
    topic: String,
    historical_marks: f64,
    historical_questions: usize,
    historical_share: f64,
    requested_share: f64,
    target_marks: usize,
    selected_marks: usize,
    selected_questions: usize,
}
#[derive(Clone)]
struct Candidate {
    row: Value,
    members: Vec<Value>,
    keys: HashSet<String>,
    marks: usize,
    section: String,
}
#[derive(Clone)]
struct Node {
    allocation: Vec<usize>,
    deficit: usize,
    previous: Option<Arc<Node>>,
    chosen: Option<(usize, usize)>,
}
pub fn row(q: &Value) -> Value {
    let raw = &q["legacy_metadata"]["question"];
    let p = &q["legacy_metadata"]["paper"];
    let ts: Vec<_> = bank::tags(q).collect();
    let mut primary = ts
        .iter()
        .find(|t| t["tag_role"] == "primary")
        .copied()
        .cloned()
        .unwrap_or(json!({}));
    syllabus::overlay(text(q, "subject"), &mut primary);
    let mut v = json!({"paperId":q["source"],"paperType":match text(q,"paper_type"){"paper1"=>"Paper 1","paper2"=>"Paper 2","paper3"=>"Paper 3",_=>"Other"},"year":q["year"],"questionRef":q["question_no"],"questionOrder":raw["question_order"],"qpPdfPage":raw["qp_pdf_page"],"msPdfPage":q["ms_pages"][0].as_u64().map(Value::from).unwrap_or_else(||raw["ms_pdf_page"].clone()),"marksTotal":q["marks"],"tagCount":ts.len(),"primaryTagId":primary["tag_id"],"tagIds":ts.iter().map(|t|text(t,"tag_id")).collect::<Vec<_>>().join(";"),"allTagIds":ts.iter().map(|t|text(t,"tag_id")).collect::<Vec<_>>().join(";"),"syllabusRefs":ts.iter().map(|t|text(t,"syllabus_ref")).collect::<Vec<_>>().join(";"),"weightedMarksSum":ts.iter().map(|t|number(t,"weighted_marks_for_tag")).sum::<f64>(),"confidence":raw["confidence"],"confidenceScore":raw["confidence_score"],"needsReview":raw["needs_review"],"evidence":raw["evidence"]});
    for (to, from) in [
        ("season", "season"),
        ("component", "component"),
        ("variant", "variant"),
        ("sourceType", "source_type"),
        ("qpFile", "qp_file"),
        ("msFile", "ms_file"),
    ] {
        v[to] = p[from].clone();
    }
    for (to, from) in [
        ("topic", "topic"),
        ("topicCn", "topic_cn"),
        ("subtopic", "subtopic"),
        ("subtopicCn", "subtopic_cn"),
        ("knowledgePointCn", "knowledge_point_cn"),
        ("knowledgePointEn", "knowledge_point_en"),
    ] {
        v[to] = primary[from].clone();
    }
    v
}
fn key(q: &Value) -> String {
    format!("{}::{}", text(q, "paperId"), text(q, "questionRef"))
}
fn section(reference: &str) -> String {
    reference.split('.').take(2).collect::<Vec<_>>().join(".")
}
fn answer_section(q: &Value) -> String {
    if q["year"].as_i64().unwrap_or(0) < 2023 || text(q, "paperType") != "Paper 2" {
        return String::new();
    }
    match text(q, "questionRef").split('(').next().unwrap_or("") {
        "1" => "A",
        "2" | "3" => "B",
        "4" | "5" => "C",
        _ => "",
    }
    .into()
}
fn complete(c: &Candidate) -> bool {
    if c.marks != 20 {
        return false;
    }
    if c.section == "A" {
        c.members.len() > 2
    } else {
        c.members.len() == 2
            && c.members
                .iter()
                .any(|q| text(q, "questionRef").ends_with("(a)") && number(q, "marksTotal") == 8.0)
            && c.members
                .iter()
                .any(|q| text(q, "questionRef").ends_with("(b)") && number(q, "marksTotal") == 12.0)
    }
}
pub fn revise(plan: &mut Value) {
    let items = plan["questions"].as_array().unwrap();
    let printed: f64 = items
        .iter()
        .map(|q| number(&q["question"], "marksTotal"))
        .sum();
    let sections = plan["answerSections"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let complete_in = |id: &str| {
        items
            .iter()
            .filter(|item| {
                let row = &item["question"];
                let members = item["members"].as_array().cloned().unwrap_or_default();
                let candidate = Candidate {
                    row: row.clone(),
                    members,
                    marks: number(row, "marksTotal") as usize,
                    section: answer_section(row),
                    keys: HashSet::new(),
                };
                candidate.section == id && complete(&candidate)
            })
            .count()
    };
    let actual = if sections.is_empty() {
        printed
    } else {
        sections
            .iter()
            .map(|s| {
                complete_in(text(s, "id")).min(s["answerCount"].as_u64().unwrap() as usize) as f64
                    * number(s, "marksPerQuestion")
            })
            .sum()
    };
    let exact = actual == number(plan, "targetMarks")
        && (sections.is_empty()
            || sections.iter().all(|s| {
                complete_in(text(s, "id")) == s["questionCount"].as_u64().unwrap() as usize
            }));
    let count = items.len();
    let selected: Vec<_> = items
        .iter()
        .map(|q| {
            (
                text(q, "bucketKey").to_owned(),
                number(&q["question"], "marksTotal"),
            )
        })
        .collect();
    plan["actualMarks"] = json!(actual);
    plan["printedMarks"] = json!(printed);
    plan["actualQuestions"] = json!(count);
    plan["exactTarget"] = json!(exact);
    if let Some(bs) = plan["buckets"].as_array_mut() {
        for b in bs {
            let members: Vec<_> = selected
                .iter()
                .filter(|(key, _)| key == text(b, "key"))
                .collect();
            b["selectedMarks"] = json!(members.iter().map(|(_, m)| m).sum::<f64>());
            b["selectedQuestions"] = json!(members.len());
        }
    }
    let mut warnings: Vec<String> = plan["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|s| !s.starts_with("选题已调整"))
        .map(str::to_owned)
        .collect();
    warnings.push(if exact{"选题已调整，分值与整题／选答规则保持一致。".to_owned()}else{format!("选题已调整：可作答 {actual} 分，卷面 {printed} 分。专题练习可导出；模拟卷需满足目标分值和选答规则。")});
    plan["warnings"] = json!(warnings);
    plan["shortageAdjustments"]=json!(plan["buckets"].as_array().into_iter().flatten().filter(|b|number(b,"selectedMarks")<number(b,"targetMarks")).map(|b|json!({"bucketKey":b["key"],"bucketLabel":b["label"],"targetMarks":b["targetMarks"],"selectedMarks":b["selectedMarks"],"missingMarks":number(b,"targetMarks")-number(b,"selectedMarks"),"redistributedTo":[]})).collect::<Vec<_>>());
}
fn select(candidates: &[Candidate], buckets: &[Bucket], target: usize) -> Vec<(usize, usize)> {
    let mut ranked: Vec<usize> = (0..candidates.len()).collect();
    let weight = |c: &Candidate| {
        buckets
            .iter()
            .filter(|b| c.keys.contains(&b.key))
            .map(|b| b.target_marks)
            .max()
            .unwrap_or(0)
    };
    ranked.sort_by(|&a, &b| {
        weight(&candidates[b])
            .cmp(&weight(&candidates[a]))
            .then_with(|| {
                number(&candidates[b].row, "year").total_cmp(&number(&candidates[a].row, "year"))
            })
            .then_with(|| natural(&key(&candidates[a].row), &key(&candidates[b].row)))
    });
    let mut reachable: Vec<Vec<Arc<Node>>> = vec![Vec::new(); target + 1];
    reachable[0].push(Arc::new(Node {
        allocation: vec![0; buckets.len()],
        deficit: buckets.iter().map(|b| b.target_marks).sum(),
        previous: None,
        chosen: None,
    }));
    for ci in ranked {
        let c = &candidates[ci];
        let m = c.marks;
        if m == 0 || m > target {
            continue;
        }
        let matching: Vec<_> = buckets
            .iter()
            .enumerate()
            .filter(|(_, b)| c.keys.contains(&b.key))
            .map(|(i, _)| i)
            .collect();
        let matching = if matching.is_empty() {
            (0..buckets.len()).collect()
        } else {
            matching
        };
        if matching.is_empty() {
            continue;
        }
        for total in (0..=target - m).rev() {
            if reachable[total].is_empty() {
                continue;
            }
            let next = total + m;
            let previous = reachable[total].clone();
            let mut options = reachable[next].clone();
            let mut seen: HashSet<Vec<usize>> =
                options.iter().map(|n| n.allocation.clone()).collect();
            for state in previous {
                let mut choices = matching.clone();
                choices.sort_by(|&a, &b| {
                    let a_def = buckets[a].target_marks as i64 - state.allocation[a] as i64;
                    let b_def = buckets[b].target_marks as i64 - state.allocation[b] as i64;
                    b_def
                        .cmp(&a_def)
                        .then_with(|| natural(&buckets[a].key, &buckets[b].key))
                });
                let bi = choices[0];
                let mut allocation = state.allocation.clone();
                allocation[bi] += m;
                if !seen.insert(allocation.clone()) {
                    continue;
                }
                let deficit = state.deficit
                    - m.min(
                        buckets[bi]
                            .target_marks
                            .saturating_sub(state.allocation[bi]),
                    );
                options.push(Arc::new(Node {
                    allocation,
                    deficit,
                    previous: Some(state),
                    chosen: Some((ci, bi)),
                }));
            }
            options.sort_by_key(|s| s.deficit);
            options.truncate(8);
            reachable[next] = options;
        }
        if reachable[target].first().is_some_and(|s| s.deficit == 0) {
            break;
        }
    }
    let total = reachable.iter().rposition(|v| !v.is_empty()).unwrap_or(0);
    let mut chosen = Vec::new();
    let mut node = reachable[total].first().cloned();
    while let Some(n) = node {
        if let Some(choice) = n.chosen {
            chosen.push(choice);
        }
        node = n.previous.clone();
    }
    chosen.reverse();
    chosen
}
fn flag(input: &Value, key: &str) -> bool {
    input[key] == true || input[key] == "true"
}
pub fn manual(qs: &[Value], subject: &str, title: &str) -> Result<Value, ApiError> {
    let paper = qs.first().map(|q| text(q, "paper_type")).unwrap_or("");
    if !["paper1", "paper2", "paper3"].contains(&paper) {
        return Err(bad("所选题目尚未具有可靠的卷别映射"));
    }
    let members: Vec<_> = qs.iter().map(row).collect();
    let marks = members
        .iter()
        .map(|q| number(q, "marksTotal").round() as i64)
        .sum::<i64>();
    if marks <= 0 {
        return Err(bad("请先核对所选题目的分值"));
    }
    let items:Vec<_>=members.into_iter().map(|q|json!({"key":key(&q),"bucketKey":"selected","bucketLabel":"自选试题","members":[q.clone()],"question":q,"replacementCandidates":[]})).collect();
    Ok(
        json!({"title":if title.trim().is_empty(){"Selected practice"}else{title},"mode":"assignment","paperType":paper,"basis":"subchapter","targetMarks":marks,"actualMarks":marks,"printedMarks":marks,"actualQuestions":items.len(),"exactTarget":true,"filters":{"subject":subject},"questions":items,"buckets":[],"answerSections":[],"warnings":[],"shortageAdjustments":[],"excludedSummary":{"totalBeforeTeacherDefaults":qs.len(),"totalAfterTeacherDefaults":qs.len(),"excludedTotal":0,"lowConfidence":0,"needsReview":0,"paperSyllabusMismatch":0},"selectionEngine":"rust"}),
    )
}
pub async fn generate(app: &App, input: &Value) -> Result<Value, ApiError> {
    if !input.is_object() {
        return Err(bad("组卷参数必须是对象"));
    }
    for key in ["yearFrom", "yearTo"] {
        if !input[key].is_null() && input[key].as_i64().is_none() {
            return Err(bad("年份必须是整数"));
        }
    }
    let subject = input["subject"].as_str().unwrap_or("9702");
    if !["9702", "9708"].contains(&subject) {
        return Err(bad("学科无效"));
    }
    let paper = input["paperType"].as_str().unwrap_or("paper1");
    let subjects: Value =
        serde_json::from_str(include_str!("../assets/subjects.json")).map_err(internal)?;
    let rules = &subjects["subjects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == subject)
        .unwrap()["paperRules"];
    let rule = &rules[paper];
    if rule.is_null() {
        return Err(bad("当前学科不支持这个卷别"));
    }
    let mode = input["mode"].as_str().unwrap_or("assignment");
    if !["assignment", "paper"].contains(&mode) {
        return Err(bad("组卷方式无效"));
    }
    let basis = input["basis"].as_str().unwrap_or("subchapter");
    if !["subchapter", "knowledgePoint"].contains(&basis) {
        return Err(bad("比例依据无效"));
    }
    let fixed = mode == "paper" && paper != "paper1";
    let target = if fixed || input["targetMarks"].is_null() {
        rule["targetMarks"].as_u64().unwrap_or(30)
    } else {
        input["targetMarks"]
            .as_u64()
            .ok_or_else(|| bad("目标分值必须是整数"))?
    } as usize;
    if !(1..=300).contains(&target) {
        return Err(bad("每次组卷的目标分值需在 1–300 之间"));
    }
    let answer_sections = if subject == "9708" && mode == "paper" && paper.starts_with("paper2") {
        let sections = rules[if paper == "paper2essay" {
            "paper2essay"
        } else {
            "paper2"
        }]["answerSections"]
            .as_array()
            .unwrap();
        sections
            .iter()
            .filter(|s| paper != "paper2dr" || s["id"] == "A")
            .cloned()
            .collect::<Vec<_>>()
    } else {
        vec![]
    };
    let mut filters = bank::Filters::from([
        ("subject".into(), subject.into()),
        ("paperType".into(), paper.into()),
        (
            "yearFrom".into(),
            input["yearFrom"]
                .as_i64()
                .unwrap_or(2020)
                .max(if answer_sections.is_empty() {
                    1990
                } else {
                    2023
                })
                .to_string(),
        ),
        (
            "yearTo".into(),
            input["yearTo"].as_i64().unwrap_or(2026).to_string(),
        ),
        (
            "excludeReview".into(),
            (!flag(input, "includeReview")).to_string(),
        ),
        (
            "excludeLowConfidence".into(),
            (!flag(input, "includeLowConfidence")).to_string(),
        ),
        ("strictPaperSyllabus".into(), "true".into()),
    ]);
    if !flag(input, "includeSpecimen") {
        filters.insert("sourceType".into(), "past_paper".into());
    }
    for k in ["keyword", "knowledgePointText"] {
        if let Some(s) = input[k].as_str().filter(|s| !s.trim().is_empty()) {
            filters.insert(k.into(), s.trim().into());
        }
    }
    let all = all_questions(app)?;
    let matches = bank::selected(all.clone(), &filters)?;
    if matches.is_empty() {
        return Err(bad("这个范围内没有满足考纲和复核条件的题目，请调整筛选"));
    }
    let mut coverage: BTreeMap<String, (Bucket, HashSet<String>)> = BTreeMap::new();
    for q in &matches {
        for tag in bank::tags(q) {
            let k = if basis == "knowledgePoint" {
                text(tag, "tag_id").to_owned()
            } else {
                section(text(tag, "syllabus_ref"))
            };
            let mut t = tag.clone();
            syllabus::overlay(subject, &mut t);
            let label = if basis == "knowledgePoint" {
                text(&t, "knowledge_point_cn")
            } else {
                text(tag, "subtopic")
            };
            let entry = coverage.entry(k.clone()).or_insert_with(|| {
                (
                    Bucket {
                        key: k,
                        label: label.into(),
                        topic: text(tag, "subtopic").into(),
                        historical_marks: 0.0,
                        historical_questions: 0,
                        historical_share: 0.0,
                        requested_share: 0.0,
                        target_marks: 0,
                        selected_marks: 0,
                        selected_questions: 0,
                    },
                    HashSet::new(),
                )
            });
            entry.0.historical_marks += number(tag, "weighted_marks_for_tag");
            entry.1.insert(text(q, "id").into());
        }
    }
    let grand: f64 = coverage.values().map(|(b, _)| b.historical_marks).sum();
    let mut available: Vec<Bucket> = coverage
        .into_values()
        .map(|(mut b, ids)| {
            b.historical_questions = ids.len();
            b.historical_share = if grand > 0.0 {
                (b.historical_marks / grand * 10000.0).round() / 100.0
            } else {
                0.0
            };
            b
        })
        .collect();
    available.sort_by(|a, b| {
        b.historical_marks
            .total_cmp(&a.historical_marks)
            .then(b.historical_questions.cmp(&a.historical_questions))
            .then_with(|| natural(&a.key, &b.key))
    });
    let sections: Vec<String> = regex::Regex::new(r"[\s,;]+")
        .unwrap()
        .split(input["sections"].as_str().unwrap_or(""))
        .filter(|s| !s.is_empty())
        .map(section)
        .collect();
    let mut ratios = HashMap::new();
    for part in input["ratioText"]
        .as_str()
        .unwrap_or("")
        .split(['\n', ',', ';'])
        .filter(|s| !s.trim().is_empty())
    {
        let (k, v) = part
            .split_once([':', '='])
            .ok_or_else(|| bad("比例格式如 1.1:30, 2.1:70"))?;
        let share: f64 = v
            .trim()
            .trim_end_matches('%')
            .parse()
            .map_err(|_| bad("章节比例必须是正数"))?;
        if !share.is_finite() || share <= 0.0 {
            return Err(bad("章节比例必须是正数"));
        }
        let k = k.trim();
        if !available
            .iter()
            .any(|b| b.key == k || section(&b.topic.split_whitespace().next().unwrap_or("")) == k)
        {
            return Err(bad(format!(
                "当前范围没有比例键 {k} 的题目，请核对考纲编号"
            )));
        }
        ratios.insert(k.to_owned(), share);
    }
    let mut buckets: Vec<_> = available
        .into_iter()
        .filter(|b| {
            sections.is_empty()
                || sections.iter().any(|s| {
                    section(b.topic.split_whitespace().next().unwrap_or("")) == *s
                        || section(b.topic.split_whitespace().next().unwrap_or(""))
                            .starts_with(&format!("{s}."))
                })
        })
        .filter(|b| {
            !sections.is_empty()
                || ratios.is_empty()
                || ratios.contains_key(&b.key)
                || ratios.contains_key(&section(b.topic.split_whitespace().next().unwrap_or("")))
        })
        .map(|mut b| {
            b.requested_share = ratios
                .get(&b.key)
                .or_else(|| ratios.get(&section(b.topic.split_whitespace().next().unwrap_or(""))))
                .copied()
                .unwrap_or(0.0);
            b
        })
        .filter(|b| b.historical_marks > 0.0 || b.requested_share > 0.0)
        .collect();
    if buckets.is_empty() {
        return Err(bad("所选章节没有可用题目，请检查考纲编号和筛选范围"));
    }
    let printed_target = if answer_sections.is_empty() {
        target
    } else {
        answer_sections
            .iter()
            .map(|s| {
                s["questionCount"].as_u64().unwrap() as usize
                    * s["marksPerQuestion"].as_u64().unwrap() as usize
            })
            .sum()
    };
    let ratio_total: f64 = buckets.iter().map(|b| b.requested_share).sum();
    let history_total: f64 = buckets.iter().map(|b| b.historical_marks).sum();
    let mut remainders = Vec::new();
    for (i, b) in buckets.iter_mut().enumerate() {
        let share = if ratio_total > 0.0 {
            b.requested_share / ratio_total
        } else {
            b.historical_marks / history_total.max(1.0)
        };
        let exact = share * printed_target as f64;
        b.requested_share = (share * 10000.0).round() / 100.0;
        b.target_marks = exact.floor() as usize;
        remainders.push((i, exact - exact.floor(), b.key.clone()));
    }
    remainders.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| natural(&a.2, &b.2)));
    let remaining = printed_target - buckets.iter().map(|b| b.target_marks).sum::<usize>();
    for (i, _, _) in remainders.into_iter().take(remaining) {
        buckets[i].target_marks += 1;
    }
    let match_ids: HashSet<_> = matches.iter().map(|q| text(q, "id").to_owned()).collect();
    let mut eligible_filters = filters.clone();
    eligible_filters.remove("keyword");
    eligible_filters.remove("knowledgePointText");
    let eligible_ids: HashSet<_> = bank::selected(all.clone(), &eligible_filters)?
        .iter()
        .map(|q| text(q, "id").to_owned())
        .collect();
    let mut grouped: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for q in &all {
        if q["subject"] != subject {
            continue;
        }
        let k = if mode == "assignment" || paper == "paper1" {
            text(q, "id").to_owned()
        } else {
            bank::top(q)
        };
        grouped.entry(k).or_default().push(q);
    }
    let mut candidates = Vec::new();
    let mut omitted = 0;
    let mut stimulus_cache = HashMap::new();
    for group in grouped.values_mut() {
        if !group.iter().any(|q| match_ids.contains(text(q, "id")))
            || !group.iter().all(|q| eligible_ids.contains(text(q, "id")))
        {
            continue;
        }
        group.sort_by(|a, b| {
            number(&a["legacy_metadata"]["question"], "question_order")
                .total_cmp(&number(&b["legacy_metadata"]["question"], "question_order"))
        });
        let members: Vec<_> = group.iter().map(|q| row(q)).collect();
        let marks: usize = members
            .iter()
            .map(|m| number(m, "marksTotal").round() as usize)
            .sum();
        let mut row = members[0].clone();
        row["marksTotal"] = json!(marks);
        let mut keys = HashSet::new();
        for q in group.iter() {
            for t in bank::tags(q) {
                keys.insert(if basis == "knowledgePoint" {
                    text(t, "tag_id").to_owned()
                } else {
                    section(text(t, "syllabus_ref"))
                });
            }
        }
        if !buckets.iter().any(|b| keys.contains(&b.key)) {
            continue;
        }
        let c = Candidate {
            section: answer_section(&row),
            row,
            members,
            keys,
            marks,
        };
        if !answer_sections.is_empty()
            && (!complete(&c) || !answer_sections.iter().any(|s| s["id"] == c.section))
        {
            continue;
        }
        if c.section == "A" && answer_sections.iter().any(|s| s["id"] == "A") {
            let id = text(group[0], "qp_file_id");
            let valid = if let Some(&valid) = stimulus_cache.get(id) {
                valid
            } else {
                let valid = pdf_engine::stimulus_available(app, id).await?;
                stimulus_cache.insert(id.to_owned(), valid);
                valid
            };
            if !valid {
                omitted += 1;
                continue;
            }
        }
        candidates.push(c);
    }
    let mut chosen = Vec::new();
    if answer_sections.is_empty() {
        chosen = select(&candidates, &buckets, target);
    } else {
        for s in &answer_sections {
            let pool: Vec<_> = candidates
                .iter()
                .enumerate()
                .filter(|(_, c)| s["id"] == c.section)
                .map(|(i, c)| (i, c.clone()))
                .collect();
            let selected = select(
                &pool.iter().map(|(_, c)| c.clone()).collect::<Vec<_>>(),
                &buckets,
                s["questionCount"].as_u64().unwrap() as usize
                    * s["marksPerQuestion"].as_u64().unwrap() as usize,
            );
            for (ci, bi) in selected {
                chosen.push((pool[ci].0, bi));
            }
        }
    }
    if chosen.is_empty() {
        return Err(bad("没有完整且分值合适的题目，请扩大年份或调整章节"));
    }
    let selected_indices: HashSet<_> = chosen.iter().map(|(ci, _)| *ci).collect();
    let mut questions = Vec::new();
    for &(ci, bi) in &chosen {
        let c = &candidates[ci];
        let b = &mut buckets[bi];
        b.selected_marks += c.marks;
        b.selected_questions += 1;
        let replacements: Vec<_> = candidates
            .iter()
            .enumerate()
            .filter(|(i, r)| {
                !selected_indices.contains(i)
                    && r.marks == c.marks
                    && r.keys.contains(&b.key)
                    && (answer_sections.is_empty() || r.section == c.section)
            })
            .take(4)
            .map(|(_, r)| {
                let mut row = r.row.clone();
                row["__groupMembers"] = json!(r.members);
                row
            })
            .collect();
        questions.push(json!({"key":key(&c.row),"bucketKey":b.key,"bucketLabel":b.label,"question":c.row,"members":c.members,"replacementCandidates":replacements}));
    }
    let printed: usize = chosen.iter().map(|(ci, _)| candidates[*ci].marks).sum();
    let actual: usize = if answer_sections.is_empty() {
        printed
    } else {
        answer_sections
            .iter()
            .map(|s| {
                let complete = chosen
                    .iter()
                    .filter(|(ci, _)| s["id"] == candidates[*ci].section)
                    .count();
                complete.min(s["answerCount"].as_u64().unwrap() as usize)
                    * s["marksPerQuestion"].as_u64().unwrap() as usize
            })
            .sum()
    };
    let exact = actual == target
        && (answer_sections.is_empty()
            || answer_sections.iter().all(|s| {
                chosen
                    .iter()
                    .filter(|(ci, _)| s["id"] == candidates[*ci].section)
                    .count()
                    == s["questionCount"].as_u64().unwrap() as usize
            }));
    let shortages:Vec<_>=buckets.iter().filter(|b|b.selected_marks<b.target_marks).map(|b|json!({"bucketKey":b.key,"bucketLabel":b.label,"targetMarks":b.target_marks,"selectedMarks":b.selected_marks,"missingMarks":b.target_marks-b.selected_marks,"redistributedTo":[]})).collect();
    let mut warnings = Vec::new();
    if !exact {
        warnings.push(format!(
            "当前可作答 {actual} 分、卷面 {printed} 分，目标 {target} 分；未补入筛选范围外的题目。"
        ));
    }
    if !shortages.is_empty() {
        warnings.push("整题分值可能无法完全贴合各章节比例，请核对分配明细。".into());
    }
    if !answer_sections.is_empty() {
        warnings.push("使用 2023 年及以后的完整题组，按现行 A/B/C 选答规则计算可作答分值。".into());
    }
    if omitted > 0 {
        warnings.push(format!(
            "排除了 {omitted} 组背景材料已删减或文字层无法可靠核查的来源。"
        ));
    }
    let title = input["title"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.chars().take(120).collect::<String>())
        .unwrap_or_else(|| {
            format!(
                "{} · {} · {}",
                if subject == "9702" {
                    "Physics 9702"
                } else {
                    "Economics 9708"
                },
                rule["label"].as_str().unwrap_or(paper),
                if mode == "paper" {
                    "Mock paper"
                } else {
                    "Practice"
                }
            )
        });
    let mut base_filters = filters.clone();
    for k in [
        "excludeReview",
        "excludeLowConfidence",
        "strictPaperSyllabus",
    ] {
        base_filters.remove(k);
    }
    let before = bank::selected(all, &base_filters)?;
    let excluded = json!({"totalBeforeTeacherDefaults":before.len(),"totalAfterTeacherDefaults":matches.len(),"excludedTotal":before.len().saturating_sub(matches.len()),"lowConfidence":before.iter().filter(|q|q["legacy_metadata"]["question"]["confidence"]=="low").count(),"needsReview":before.iter().filter(|q|q["legacy_metadata"]["question"]["needs_review"]==1).count(),"paperSyllabusMismatch":before.iter().filter(|q|{let max=if subject=="9702"{11}else{6};let ts:Vec<_>=bank::tags(q).collect();ts.is_empty()||ts.iter().any(|t|!(1..=max).contains(&text(t,"syllabus_ref").split('.').next().unwrap_or("").parse::<i64>().unwrap_or(0)))}).count()});
    let mut json_filters = json!({"subject":subject});
    for (k, v) in &filters {
        json_filters[k] = if [
            "yearFrom", "yearTo", "year", "limit", "offset", "marksMin", "marksMax",
        ]
        .contains(&k.as_str())
        {
            json!(v.parse::<i64>().map_err(internal)?)
        } else if ["true", "false"].contains(&v.as_str()) {
            json!(v == "true")
        } else {
            json!(v)
        };
    }
    Ok(
        json!({"title":title,"mode":mode,"paperType":paper,"basis":basis,"targetMarks":target,"actualMarks":actual,"printedMarks":printed,"actualQuestions":questions.len(),"exactTarget":exact,"answerSections":answer_sections,"filters":json_filters,"buckets":buckets,"questions":questions,"warnings":warnings,"shortageAdjustments":shortages,"excludedSummary":excluded,"selectionEngine":"rust"}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editing_preserves_answer_marks_but_requires_the_full_choice_structure() {
        let question = |n: i64| {
            let a = json!({"paperId":"p","paperType":"Paper 2","year":2025,"questionRef":format!("{n}(a)"),"marksTotal":8});
            let b = json!({"questionRef":format!("{n}(b)"),"marksTotal":12});
            let mut row = a.clone();
            row["marksTotal"] = json!(20);
            json!({"question":row,"members":[a,b],"bucketKey":"x"})
        };
        let mut plan = json!({"targetMarks":40,"answerSections":[{"id":"B","questionCount":2,"answerCount":1,"marksPerQuestion":20},{"id":"C","questionCount":2,"answerCount":1,"marksPerQuestion":20}],"questions":[question(2),question(3),question(4),question(5)],"buckets":[],"warnings":["原始材料核对说明"]});
        revise(&mut plan);
        assert_eq!(plan["actualMarks"], 40.0);
        assert_eq!(plan["printedMarks"], 80.0);
        assert_eq!(plan["exactTarget"], true);
        plan["questions"].as_array_mut().unwrap().remove(0);
        revise(&mut plan);
        assert_eq!(plan["actualMarks"], 40.0);
        assert_eq!(plan["printedMarks"], 60.0);
        assert_eq!(plan["exactTarget"], false);
        assert_eq!(plan["warnings"][0], "原始材料核对说明");
    }
    #[test]
    fn subset_sum_never_reuses_a_question() {
        let bucket = Bucket {
            key: "1.1".into(),
            label: "a".into(),
            topic: "a".into(),
            historical_marks: 10.0,
            historical_questions: 3,
            historical_share: 100.0,
            requested_share: 100.0,
            target_marks: 10,
            selected_marks: 0,
            selected_questions: 0,
        };
        let c = |m: usize| Candidate {
            row: json!({"paperId":m.to_string(),"questionRef":"1","year":2025}),
            members: vec![],
            keys: HashSet::from(["1.1".into()]),
            marks: m,
            section: "".into(),
        };
        let cs = vec![c(6), c(4), c(3)];
        let result = select(&cs, &[bucket], 10);
        assert_eq!(result.iter().map(|(i, _)| cs[*i].marks).sum::<usize>(), 10);
        assert_eq!(
            result.iter().map(|(i, _)| i).collect::<HashSet<_>>().len(),
            result.len()
        );
    }
}
