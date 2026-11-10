use super::*;
use bank::{number, text};
use std::collections::{BTreeMap, BTreeSet, HashSet};
pub fn question_result(q: &Value) -> Value {
    let mut primary = bank::tags(q)
        .find(|t| t["tag_role"] == "primary")
        .cloned()
        .unwrap_or(json!({}));
    syllabus::overlay(text(q, "subject"), &mut primary);
    json!({"id":q["id"],"paperId":q["source"],"questionRef":q["question_no"],"marks":q["marks"],"topic":primary["topic_cn"].as_str().filter(|s|!s.is_empty()).unwrap_or(text(&primary,"topic")),"subtopic":primary["subtopic_cn"].as_str().filter(|s|!s.is_empty()).unwrap_or(text(&primary,"subtopic")),"knowledgePoint":primary["knowledge_point_cn"],"confidence":q["legacy_metadata"]["question"]["confidence"],"evidence":q["legacy_metadata"]["question"]["evidence"],"qpPdfPage":q["qp_pages"][0],"msPdfPage":q["ms_pages"][0],"pdfSliceUrl":q["qp_url"],"qpUrl":q["qp_url"],"msUrl":q["ms_url"]})
}
pub async fn catalog() -> Result<Json<Value>, ApiError> {
    let mut v: Value =
        serde_json::from_str(include_str!("../assets/agent-contract.json")).map_err(internal)?;
    v["service"] = json!("Study local Rust Agent gateway");
    v["version"] = json!("3.0.0");
    v["legacy_contract"] = json!("2.2.0");
    for tool in v["tools"].as_array_mut().unwrap() {
        tool["implementation"] = json!("rust");
        if tool["name"] == "semantic_vector_search" {
            tool["parameters"]["properties"]["filters"] = json!({"type":"object","description":"Optional structured question-bank filters, validated by the same Rust rules as the library."});
        }
    }
    v["recognition_model_configured"] = json!(false);
    Ok(Json(v))
}
pub async fn native(app: &App, tool: &str, args: &Value) -> Result<Option<Value>, ApiError> {
    let subject = args["subject"].as_str().unwrap_or("9708");
    if tool == "get_subjects" {
        return Ok(Some(
            serde_json::from_str(include_str!("../assets/subjects.json")).map_err(internal)?,
        ));
    }
    if tool == "semantic_vector_search" {
        return Ok(Some(semantic::search(app, args).await?));
    }
    if tool == "generate_exam_paper" {
        let mut input = args.clone();
        input["subject"] = json!(subject);
        if input["mode"].is_null() {
            input["mode"] = json!("paper");
        }
        let Json(mut result) = library_features::compose(State(app.clone()), Json(input)).await?;
        result["subject"] = json!(subject);
        result["paperType"] = json!(args["paperType"].as_str().unwrap_or("paper1"));
        result["totalMarks"] = result["actualMarks"].clone();
        result["totalQuestions"] = result["actualQuestions"].clone();
        result["exportBundleApi"] = result["downloadUrl"].clone();
        return Ok(Some(result));
    }
    if tool == "get_syllabus_tree" {
        let Json(v) = bank::options(
            State(app.clone()),
            Query(bank::Filters::from([("subject".into(), subject.into())])),
        )
        .await?;
        let mut sections = BTreeMap::new();
        let mut points = Vec::new();
        let db = app.db.lock().unwrap();
        let mut st = db
            .prepare("SELECT data FROM syllabus_points WHERE subject=?1 ORDER BY tag_id")
            .map_err(internal)?;
        for row in st
            .query_map([subject], |r| r.get::<_, String>(0))
            .map_err(internal)?
        {
            let mut p: Value = serde_json::from_str(&row.map_err(internal)?).map_err(internal)?;
            syllabus::overlay(subject, &mut p);
            let section = text(&p, "syllabus_ref")
                .split('.')
                .take(2)
                .collect::<Vec<_>>()
                .join(".");
            sections.entry(section.clone()).or_insert_with(
                || json!({"section":section,"label":p["subtopic"],"labelCn":p["subtopic_cn"]}),
            );
            points.push(p);
        }
        return Ok(Some(
            json!({"subject":subject,"topics":v["topics"].as_array().unwrap().iter().map(|v|v["value"].clone()).collect::<Vec<_>>(),"subtopics":v["subtopics"].as_array().unwrap().iter().map(|v|v["value"].clone()).collect::<Vec<_>>(),"sections":sections.into_values().collect::<Vec<_>>(),"points":points,"engine":"rust"}),
        ));
    }
    if tool == "get_syllabus_coverage" {
        if (subject == "9702"
            && ["paper2dr", "paper2essay"].contains(&args["paperType"].as_str().unwrap_or("")))
            || (subject == "9708" && args["paperType"] == "paper3")
        {
            return Err(bad("当前学科不支持这个卷别"));
        }
        let mut args = args.clone();
        args["subject"] = json!(subject);
        if args["paperType"]
            .as_str()
            .is_some_and(|p| p.starts_with("paper2"))
        {
            args["paperType"] = json!("paper2");
        }
        let query = serde_json::from_value::<library_features::CoverageQuery>(args.clone())
            .map_err(|_| bad("统计参数无效"))?;
        let Json(mut result) = library_features::coverage(State(app.clone()), Query(query)).await?;
        if args["includeEmpty"] == false {
            result["points"]
                .as_array_mut()
                .unwrap()
                .retain(|p| p["questionCount"].as_u64().unwrap_or(0) > 0);
        }
        return Ok(Some(result));
    }
    let mut f = bank::from_json(args)?;
    f.entry("subject".into()).or_insert(subject.into());
    if tool == "search_questions" {
        let qs = bank::selected(all_questions(app)?, &f)?;
        let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 100) as usize;
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let total = qs.len();
        let results: Vec<_> = qs
            .iter()
            .skip(offset)
            .take(limit)
            .map(question_result)
            .collect();
        return Ok(Some(
            json!({"subject":subject,"total":total,"returned":results.len(),"questions":results,"engine":"rust"}),
        ));
    }
    if tool == "get_kp_questions" || tool == "get_kp_trend" {
        let tag = args["tagId"].as_str().unwrap_or("");
        let reference = args["syllabusRef"].as_str().unwrap_or("");
        if tag.is_empty() && reference.is_empty() {
            return Err(bad("需要 syllabusRef 或 tagId"));
        }
        if !tag.is_empty() && !reference.is_empty() {
            return Err(bad("syllabusRef 与 tagId 请选择其中一个"));
        }
        if !reference.is_empty()
            && !regex::Regex::new(r"^\d{1,2}(\.\d{1,2}){0,2}$")
                .unwrap()
                .is_match(reference)
        {
            return Err(bad("考纲编号无效"));
        }
        if f.get("paperType").is_some_and(|p| p.starts_with("paper2")) {
            f.insert("paperType".into(), "paper2".into());
        }
        let mut sample_f = f.clone();
        for k in ["tagId", "syllabusRef", "role", "sort", "limit", "offset"] {
            sample_f.remove(k);
        }
        let sample_qs = bank::selected(all_questions(app)?, &sample_f)?;
        let matches = |t: &Value| {
            (!tag.is_empty() && text(t, "tag_id") == tag)
                || (tag.is_empty()
                    && (text(t, "syllabus_ref") == reference
                        || text(t, "syllabus_ref").starts_with(&format!("{reference}."))))
        };
        let mut qs = bank::selected(all_questions(app)?, &f)?;
        let total = qs.len();
        qs.sort_by(|a, b| {
            let sort = args["sort"].as_str().unwrap_or("year_desc");
            let year = number(a, "year").total_cmp(&number(b, "year"));
            let order = if sort == "year_asc" {
                year
            } else {
                year.reverse()
            };
            let marks = if sort == "marks_desc" {
                number(b, "marks").total_cmp(&number(a, "marks"))
            } else {
                std::cmp::Ordering::Equal
            };
            let season = text(&a["legacy_metadata"]["paper"], "season")
                .cmp(text(&b["legacy_metadata"]["paper"], "season"));
            let season = if sort == "year_asc" {
                season
            } else if sort == "marks_desc" {
                std::cmp::Ordering::Equal
            } else {
                season.reverse()
            };
            marks
                .then(order)
                .then(season)
                .then_with(|| text(a, "question_no").cmp(text(b, "question_no")))
                .then_with(|| {
                    number(&a["legacy_metadata"]["question"], "question_id")
                        .total_cmp(&number(&b["legacy_metadata"]["question"], "question_id"))
                })
        });
        let sample = json!({"paperType":args["paperType"],"yearFrom":args["yearFrom"],"yearTo":args["yearTo"],"papers":sample_qs.iter().map(|q|text(q,"source")).collect::<HashSet<_>>().len(),"questions":sample_qs.len(),"marks":sample_qs.iter().map(|q|number(q,"marks")).sum::<f64>()});
        if tool == "get_kp_trend" {
            let mut years: BTreeMap<i64, (HashSet<String>, BTreeSet<String>, f64)> =
                BTreeMap::new();
            for q in &qs {
                let row = years.entry(q["year"].as_i64().unwrap_or(0)).or_default();
                row.0.insert(text(q, "id").into());
                row.1.insert(text(q, "source").into());
                row.2 += bank::tags(q)
                    .filter(|t| matches(t))
                    .map(|t| number(t, "weighted_marks_for_tag"))
                    .sum::<f64>();
            }
            return Ok(Some(
                json!({"subject":subject,"tagId":args["tagId"],"syllabusRef":args["syllabusRef"],"sample":sample,"years":years.into_iter().map(|(year,(qs,ps,m))|json!({"year":year,"questionCount":qs.len(),"papers":ps.len(),"marksTotal":(m*1000.0).round()/1000.0})).collect::<Vec<_>>(),"engine":"rust"}),
            ));
        }
        let role = args["role"].as_str().unwrap_or("all");
        let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 100) as usize;
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let rows: Vec<_> = qs
            .iter()
            .flat_map(|q| {
                bank::tags(q)
                    .filter(|t| matches(t) && (role == "all" || text(t, "tag_role") == role))
                    .map(|t| {
                        let mut r = question_result(q);
                        r["year"] = q["year"].clone();
                        r["marksTotal"] = q["marks"].clone();
                        for (dest, src) in [
                            ("season", "season"),
                            ("component", "component"),
                            ("variant", "variant"),
                        ] {
                            r[dest] = q["legacy_metadata"]["paper"][src].clone();
                        }
                        r["tagRole"] = t["tag_role"].clone();
                        r["fullMarks"] = t["full_marks_for_tag"].clone();
                        r["weightedMarks"] = t["weighted_marks_for_tag"].clone();
                        r
                    })
            })
            .skip(offset)
            .take(limit)
            .collect();
        return Ok(Some(
            json!({"subject":subject,"tagId":args["tagId"],"syllabusRef":args["syllabusRef"],"sample":sample,"total":total,"limit":limit,"offset":offset,"questions":rows,"engine":"rust"}),
        ));
    }
    Err(err(StatusCode::NOT_FOUND, "未知工具"))
}
