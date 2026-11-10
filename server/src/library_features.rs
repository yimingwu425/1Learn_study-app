use super::*;
use std::collections::{BTreeMap, HashSet};

pub async fn memory(State(app): State<App>) -> Result<Json<Value>, ApiError> {
    let raw: Option<String> = app
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT value FROM settings WHERE key='study_memory'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(internal)?;
    let value = raw
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or_else(|| json!({"questions":{}}));
    Ok(Json(json!({"memory":value})))
}
pub async fn save_memory(
    State(app): State<App>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    if !value["questions"].is_object() {
        return Err(bad("学习草稿格式无效"));
    }
    let raw = serde_json::to_string(&value).map_err(internal)?;
    if raw.len() > 1024 * 1024 {
        return Err(bad("学习草稿过大，请先整理笔记"));
    }
    app.db.lock().unwrap().execute("INSERT INTO settings(key,value) VALUES('study_memory',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[raw]).map_err(internal)?;
    Ok(Json(json!({"saved":true})))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CoverageQuery {
    subject: Option<String>,
    paper_type: Option<String>,
    year_from: Option<i64>,
    year_to: Option<i64>,
}
struct Bucket {
    meta: Value,
    questions: HashSet<String>,
    years: HashSet<i64>,
    marks: f64,
}
fn rounded(x: f64, digits: i32) -> f64 {
    let m = 10_f64.powi(digits);
    (x * m).round() / m
}
fn chapter(v: &Value) -> i64 {
    v["syllabus_ref"]
        .as_str()
        .unwrap_or("")
        .split('.')
        .next()
        .unwrap_or("")
        .parse()
        .unwrap_or(0)
}
fn title(v: &Value) -> &str {
    v["topic_cn"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| v["topic"].as_str())
        .unwrap_or("未归类")
}

fn summarize(subject: &str, qs: &[Value], catalog: &[Value], filter: &CoverageQuery) -> Value {
    let mut points: BTreeMap<String, Bucket> = BTreeMap::new();
    for p in catalog {
        if let Some(id) = p["tag_id"].as_str() {
            points.insert(
                id.into(),
                Bucket {
                    meta: p.clone(),
                    questions: HashSet::new(),
                    years: HashSet::new(),
                    marks: 0.0,
                },
            );
        }
    }
    let mut chapter_questions: BTreeMap<i64, HashSet<String>> = BTreeMap::new();
    let mut sample_questions = 0;
    let mut sample_marks = 0.0;
    let mut papers = HashSet::new();
    let mut unmatched = 0;
    let mut sample_years = Vec::new();
    for q in qs {
        let year = q["year"].as_i64();
        if filter
            .paper_type
            .as_ref()
            .is_some_and(|p| q["paper_type"].as_str() != Some(p.as_str()))
        {
            continue;
        }
        if filter.year_from.is_some_and(|a| year.is_none_or(|y| y < a))
            || filter.year_to.is_some_and(|b| year.is_none_or(|y| y > b))
        {
            continue;
        }
        sample_questions += 1;
        sample_marks += q["marks"].as_f64().unwrap_or(0.0);
        if let Some(p) = q["source"].as_str() {
            papers.insert(p.to_owned());
        }
        if let Some(y) = year {
            sample_years.push(y);
        }
        let id = q["id"].as_str().unwrap_or("").to_owned();
        let mut has_tag = false;
        if let Some(tags) = q["legacy_metadata"]["tags"].as_array() {
            for tag in tags {
                if let Some(b) = tag["tag_id"].as_str().and_then(|key| points.get_mut(key)) {
                    has_tag = true;
                    b.questions.insert(id.clone());
                    if let Some(y) = year {
                        b.years.insert(y);
                    }
                    b.marks += tag["weighted_marks_for_tag"].as_f64().unwrap_or(0.0);
                    chapter_questions
                        .entry(chapter(&b.meta))
                        .or_default()
                        .insert(id.clone());
                }
            }
        }
        if !has_tag {
            unmatched += 1;
        }
    }
    let grand: f64 = points.values().map(|p| p.marks).sum();
    let mut rows:Vec<Value>=points.iter().map(|(id,b)|json!({
        "tagId":id,"syllabusRef":b.meta["syllabus_ref"],"chapter":chapter(&b.meta),
        "topic":b.meta["topic"],"topicCn":b.meta["topic_cn"],"knowledgePointCn":b.meta["knowledge_point_cn"],"knowledgePointEn":b.meta["knowledge_point_en"],"originalKnowledgePointEn":b.meta["original_knowledge_point_en"],"originalKnowledgePointCn":b.meta["original_knowledge_point_cn"],"syllabusPage":b.meta["syllabus_page"],"syllabusSource":b.meta["syllabus_source"],"translationNote":b.meta["translation_note"],
        "questionCount":b.questions.len(),"marksTotal":rounded(b.marks,3),"marksPct":if grand>0.0{rounded(b.marks/grand*100.0,2)}else{0.0},
        "yearFirst":b.years.iter().min(),"yearLast":b.years.iter().max(),"yearsPresent":b.years.len()
    })).collect();
    rows.sort_by(|a, b| {
        b["marksTotal"]
            .as_f64()
            .partial_cmp(&a["marksTotal"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a["tagId"].as_str().cmp(&b["tagId"].as_str()))
    });
    let mut chapters: BTreeMap<i64, Value> = BTreeMap::new();
    for b in points.values() {
        let c = chapter(&b.meta);
        let v=chapters.entry(c).or_insert_with(||json!({"chapter":c,"title":title(&b.meta),"titleEn":b.meta["topic"],"titleCn":b.meta["topic_cn"],"pointCount":0,"seenPointCount":0,"questionCount":chapter_questions.get(&c).map_or(0,|x|x.len()),"marksTotal":0.0}));
        v["pointCount"] = json!(v["pointCount"].as_u64().unwrap() + 1);
        if !b.questions.is_empty() {
            v["seenPointCount"] = json!(v["seenPointCount"].as_u64().unwrap() + 1);
        }
        v["marksTotal"] = json!(v["marksTotal"].as_f64().unwrap() + b.marks);
    }
    let mut chapters: Vec<Value> = chapters
        .into_values()
        .map(|mut c| {
            let m = c["marksTotal"].as_f64().unwrap();
            c["marksTotal"] = json!(rounded(m, 3));
            c["marksPct"] = json!(if grand > 0.0 {
                rounded(m / grand * 100.0, 2)
            } else {
                0.0
            });
            c
        })
        .collect();
    chapters.sort_by(|a, b| {
        b["marksTotal"]
            .as_f64()
            .partial_cmp(&a["marksTotal"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    json!({"subject":subject,"sample":{"questions":sample_questions,"papers":papers.len(),"marks":rounded(sample_marks,3),"yearFirst":sample_years.iter().min(),"yearLast":sample_years.iter().max()},"chapters":chapters,"points":rows,"taggedMarks":rounded(grand,3),"unclassifiedQuestions":unmatched,"basis":"原题库标签的加权分值；同一道题可关联多个考点，章节题数不相加。占比是样本分布，不是未来出题概率。"})
}

pub async fn coverage(
    State(app): State<App>,
    Query(filter): Query<CoverageQuery>,
) -> Result<Json<Value>, ApiError> {
    let subject = filter.subject.as_deref().unwrap_or("9702");
    if !["9702", "9708"].contains(&subject) {
        return Err(bad("请选择物理或经济"));
    }
    if filter
        .paper_type
        .as_ref()
        .is_some_and(|p| !["paper1", "paper2", "paper3"].contains(&p.as_str()))
    {
        return Err(bad("卷别无效"));
    }
    if [filter.year_from, filter.year_to]
        .into_iter()
        .flatten()
        .any(|y| !(1990..=2100).contains(&y))
        || filter
            .year_from
            .zip(filter.year_to)
            .is_some_and(|(a, b)| a > b)
    {
        return Err(bad("年份范围无效"));
    }
    let db = app.db.lock().unwrap();
    fn read(db: &Connection, sql: &str, subject: &str) -> Result<Vec<Value>, ApiError> {
        let mut st = db.prepare(sql).map_err(internal)?;
        let rows = st
            .query_map([subject], |r| r.get::<_, String>(0))
            .map_err(internal)?;
        rows.map(|s| serde_json::from_str(&s.map_err(internal)?).map_err(internal))
            .collect()
    }
    let qs = read(&db, "SELECT data FROM questions WHERE subject=?1", subject)?;
    let mut catalog = read(
        &db,
        "SELECT data FROM syllabus_points WHERE subject=?1",
        subject,
    )?;
    for p in &mut catalog {
        syllabus::overlay(subject, p);
    }
    if catalog.is_empty() {
        return Err(err(
            StatusCode::SERVICE_UNAVAILABLE,
            "考纲目录尚未迁入，请完成本地迁移",
        ));
    }
    Ok(Json(summarize(subject, &qs, &catalog, &filter)))
}

pub async fn compose(
    State(app): State<App>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let plan = composer::generate(&app, &input).await?;
    store_plan(&app, plan)
}

fn store_plan(app: &App, plan: Value) -> Result<Json<Value>, ApiError> {
    if plan["questions"].as_array().is_none_or(|q| q.is_empty()) {
        return Err(bad("没有满足条件的题目，请扩大范围"));
    }
    let mut ids = BTreeMap::new();
    {
        let db = app.db.lock().unwrap();
        let mut st = db
            .prepare("SELECT id,data FROM questions WHERE subject=?1")
            .map_err(internal)?;
        let rs = st
            .query_map(
                [plan["filters"]["subject"].as_str().unwrap_or("9702")],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .map_err(internal)?;
        for r in rs {
            let (id, s) = r.map_err(internal)?;
            let q: Value = serde_json::from_str(&s).map_err(internal)?;
            ids.insert(
                format!(
                    "{}::{}",
                    q["source"].as_str().unwrap_or(""),
                    q["question_no"].as_str().unwrap_or("")
                ),
                id,
            );
        }
    }
    let brief = |q: &Value| json!({"id":ids.get(&format!("{}::{}",q["paperId"].as_str().unwrap_or(""),q["questionRef"].as_str().unwrap_or(""))),"paperId":q["paperId"],"questionRef":q["questionRef"],"marks":q["marksTotal"],"topic":q["knowledgePointCn"]});
    let questions: Vec<Value> = plan["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            let mut q = brief(&item["question"]);
            q["key"] = item["key"].clone();
            q["members"] = json!(item["members"].as_array().map_or(0, |m| m.len()));
            q["replacementCandidates"] = json!(item["replacementCandidates"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|q| brief(q))
                .collect::<Vec<_>>());
            q
        })
        .collect();
    let id = uuid::Uuid::new_v4().to_string();
    let dir = app.data.join("compositions");
    std::fs::create_dir_all(&dir).map_err(internal)?;
    std::fs::write(
        dir.join(format!("{id}.json")),
        serde_json::to_vec(&plan).map_err(internal)?,
    )
    .map_err(internal)?;
    Ok(Json(
        json!({"id":id,"mode":plan["mode"],"title":plan["title"],"questions":questions,"actualQuestions":plan["actualQuestions"],"actualMarks":plan["actualMarks"],"printedMarks":plan["printedMarks"],"targetMarks":plan["targetMarks"],"exactTarget":plan["exactTarget"],"answerSections":plan["answerSections"],"warnings":plan["warnings"],"buckets":plan["buckets"],"excludedSummary":plan["excludedSummary"],"selectionEngine":"rust","downloadUrl":format!("/api/compositions/{id}/download")}),
    ))
}
pub async fn download(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    uuid::Uuid::parse_str(&id).map_err(|_| bad("试卷标识无效"))?;
    let bytes = std::fs::read(app.data.join("compositions").join(format!("{id}.json")))
        .map_err(|_| err(StatusCode::NOT_FOUND, "组卷记录不存在，请重新生成"))?;
    let plan: Value = serde_json::from_slice(&bytes).map_err(internal)?;
    let bytes = native_assembler::bundle(&app, &plan).await?;
    Ok((
        [
            ("content-type", "application/zip"),
            (
                "content-disposition",
                "attachment; filename=study-paper.zip",
            ),
        ],
        bytes,
    )
        .into_response())
}

/// Edits create another immutable plan, so the reviewed original can always be restored.
pub async fn edit(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    uuid::Uuid::parse_str(&id).map_err(|_| bad("试卷标识无效"))?;
    let bytes = std::fs::read(app.data.join("compositions").join(format!("{id}.json")))
        .map_err(|_| err(StatusCode::NOT_FOUND, "选题方案不存在"))?;
    let mut plan: Value = serde_json::from_slice(&bytes).map_err(internal)?;
    let index = input["index"]
        .as_u64()
        .ok_or_else(|| bad("缺少有效选题位置"))? as usize;
    let items = plan["questions"]
        .as_array_mut()
        .ok_or_else(|| bad("方案格式无效"))?;
    if index >= items.len() {
        return Err(bad("选题位置超出范围"));
    }
    match input["action"].as_str().unwrap_or("") {
        "remove" => {
            if items.len() <= 1 {
                return Err(bad("练习至少保留一道题"));
            }
            items.remove(index);
        }
        "replace" => {
            let q = get_question(
                &app,
                input["replacementId"]
                    .as_str()
                    .ok_or_else(|| bad("请选择替换题目"))?,
            )?;
            let replacement = items[index]["replacementCandidates"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|r| r["paperId"] == q["source"] && r["questionRef"] == q["question_no"])
                .cloned()
                .ok_or_else(|| bad("这道题不在同分值、同范围的替换列表中"))?;
            let members = replacement["__groupMembers"]
                .as_array()
                .cloned()
                .unwrap_or_else(|| vec![replacement.clone()]);
            let keys: HashSet<String> = items
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .flat_map(|(_, item)| {
                    item["members"].as_array().into_iter().flatten().map(|m| {
                        format!(
                            "{}::{}",
                            m["paperId"].as_str().unwrap_or(""),
                            m["questionRef"].as_str().unwrap_or("")
                        )
                    })
                })
                .collect();
            if members.iter().any(|m| {
                keys.contains(&format!(
                    "{}::{}",
                    m["paperId"].as_str().unwrap_or(""),
                    m["questionRef"].as_str().unwrap_or("")
                ))
            }) {
                return Err(bad("替换题目已在当前方案中"));
            }
            let old = items[index].clone();
            let mut candidates = old["replacementCandidates"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            candidates.retain(|r| {
                !(r["paperId"] == replacement["paperId"]
                    && r["questionRef"] == replacement["questionRef"])
            });
            let mut previous = old["question"].clone();
            previous["__groupMembers"] = old["members"].clone();
            candidates.push(previous);
            items[index]["question"] = replacement.clone();
            items[index]["members"] = json!(members);
            items[index]["key"] = json!(format!(
                "{}::{}",
                replacement["paperId"].as_str().unwrap_or(""),
                replacement["questionRef"].as_str().unwrap_or("")
            ));
            items[index]["replacementCandidates"] = json!(candidates);
        }
        _ => return Err(bad("请选择移除或同条件换题")),
    }
    composer::revise(&mut plan);
    plan["parentId"] = json!(id);
    store_plan(&app, plan)
}

pub async fn save_export(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let response = download(State(app.clone()), Path(id)).await?;
    let bytes = axum::body::to_bytes(response.into_body(), 128 * 1024 * 1024)
        .await
        .map_err(internal)?;
    let id = uuid::Uuid::new_v4().to_string();
    let path = app.data.join("exports").join(format!("{id}.zip"));
    tokio::fs::write(&path, bytes).await.map_err(internal)?;
    app.db
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO exports(id,path,created_at) VALUES(?1,?2,?3)",
            params![id, path.to_string_lossy(), now()],
        )
        .map_err(internal)?;
    Ok(Json(
        json!({"id":id,"url":format!("/api/exports/{id}"),"path":path}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weights_and_unique_chapter_counts() {
        let catalog = vec![
            json!({"tag_id":"a","syllabus_ref":"1.1","topic":"Mechanics"}),
            json!({"tag_id":"b","syllabus_ref":"1.2","topic":"Mechanics"}),
            json!({"tag_id":"c","syllabus_ref":"2.1","topic":"Waves"}),
        ];
        let qs = vec![
            json!({"id":"q1","source":"paper","marks":4,"year":2025,"legacy_metadata":{"tags":[{"tag_id":"a","weighted_marks_for_tag":3},{"tag_id":"b","weighted_marks_for_tag":1}]}}),
        ];
        let r = summarize("9702", &qs, &catalog, &CoverageQuery::default());
        assert_eq!(r["sample"]["questions"], 1);
        assert_eq!(r["chapters"][0]["questionCount"], 1);
        assert_eq!(r["points"][0]["marksPct"], 75.0);
        assert_eq!(r["chapters"][1]["marksPct"], 0.0);
        assert_eq!(r["points"].as_array().unwrap().len(), 3);
    }
}
