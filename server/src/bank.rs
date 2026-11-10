//! Question-bank rules operate on the preserved source metadata, never on PDF text fragments.
use super::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
pub type Filters = HashMap<String, String>;
pub fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
pub fn tags(q: &Value) -> impl Iterator<Item = &Value> {
    q["legacy_metadata"]["tags"]
        .as_array()
        .into_iter()
        .flatten()
}
pub fn number(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or(0.0)
}
pub fn top(q: &Value) -> String {
    format!(
        "{}::{}",
        text(q, "source"),
        text(q, "question_no").split('(').next().unwrap_or("")
    )
}
pub fn value(f: &Filters, k: &str) -> String {
    f.get(k).cloned().unwrap_or_default()
}
pub fn flag(f: &Filters, k: &str) -> bool {
    f.get(k)
        .is_some_and(|s| ["true", "1", "on"].contains(&s.as_str()))
}
fn num(f: &Filters, k: &str) -> Option<f64> {
    f.get(k)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
}
pub fn from_json(v: &Value) -> Result<Filters, ApiError> {
    let m = v.as_object().ok_or_else(|| bad("筛选参数必须是对象"))?;
    Ok(m.iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| {
            (
                k.clone(),
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string()),
            )
        })
        .collect())
}
pub fn normalize(mut q: Value) -> Value {
    if q["legacy_metadata"].is_null() {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = RE
            .get_or_init(|| regex::Regex::new(r"^(9702|9708)_([msw])(\d{2})_(\d{1,2})$").unwrap());
        let source = text(&q, "source").to_owned();
        if let Some(c) = re.captures(&source) {
            if q["year"].is_null() {
                q["year"] = json!(2000 + c[3].parse::<i64>().unwrap());
            }
            let component = &c[4];
            q["paper_type"] = json!(match component.chars().next() {
                Some('1') => "paper1",
                Some('2') => "paper2",
                Some('3') => "paper3",
                _ => "other",
            });
            q["source_metadata"] = json!({"season":&c[2],"component":component,"variant":component.chars().last().map(|c|c.to_string()),"source_type":"past_paper"});
        }
    }
    static CORRECTIONS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let corrections = CORRECTIONS.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/page-corrections.json"))
            .expect("reviewed page corrections")
    });
    let correction = &corrections[text(&q, "id")];
    if correction.is_object()
        && q["source"] == correction["source"]
        && q["question_no"] == correction["questionNo"]
        && q["ms_file_id"] == correction["fileId"]
    {
        q["original_ms_pages"] = correction["originalPages"].clone();
        q["ms_pages"] = correction["pages"].clone();
        q["ms_url"] = json!(format!(
            "/api/files/{}#page={}",
            text(&q, "ms_file_id"),
            correction["pages"][0].as_u64().unwrap()
        ));
        q["source_page_correction"] = correction.clone();
    }
    q
}
pub fn validate(f: &Filters) -> Result<(), ApiError> {
    let allowed = [
        "query",
        "q",
        "subject",
        "paperType",
        "sourceType",
        "component",
        "variant",
        "paperId",
        "paper",
        "year",
        "yearFrom",
        "yearTo",
        "season",
        "topic",
        "subtopic",
        "syllabusRef",
        "tagId",
        "tag",
        "knowledgePointText",
        "primaryOnly",
        "includeSecondary",
        "confidence",
        "needsReview",
        "excludeReview",
        "excludeLowConfidence",
        "strictPaperSyllabus",
        "marksMin",
        "marksMax",
        "keyword",
        "sortKey",
        "sortDirection",
        "limit",
        "offset",
        "role",
        "sort",
    ];
    for (k, v) in f {
        if !v.is_empty() && !allowed.contains(&k.as_str()) {
            return Err(bad(format!("不支持筛选参数 {k}，未扩大搜索范围")));
        }
    }
    for (k, options) in [
        ("subject", vec!["9702", "9708"]),
        (
            "paperType",
            vec!["paper1", "paper2", "paper2dr", "paper2essay", "paper3"],
        ),
        ("confidence", vec!["high", "medium", "low"]),
        ("sourceType", vec!["past_paper", "specimen"]),
        ("role", vec!["primary", "secondary", "all"]),
        ("sort", vec!["year_desc", "year_asc", "marks_desc"]),
        ("sortDirection", vec!["asc", "desc"]),
        (
            "sortKey",
            vec![
                "section",
                "paper",
                "question",
                "marks",
                "chapter",
                "confidence",
                "review",
            ],
        ),
    ] {
        if f.get(k)
            .is_some_and(|s| !s.is_empty() && !options.contains(&s.as_str()))
        {
            return Err(bad(format!("{k} 无效")));
        }
    }
    for k in [
        "year", "yearFrom", "yearTo", "marksMin", "marksMax", "limit", "offset",
    ] {
        if f.get(k).is_some_and(|s| {
            !s.is_empty()
                && s.parse::<f64>()
                    .ok()
                    .is_none_or(|n| !n.is_finite() || n < 0.0)
        }) {
            return Err(bad(format!("{k} 必须是非负数")));
        }
    }
    for k in ["year", "yearFrom", "yearTo"] {
        if num(f, k).is_some_and(|n| n.fract() != 0.0 || !(1990.0..=2100.0).contains(&n)) {
            return Err(bad("年份范围无效"));
        }
    }
    for (a, b) in [("yearFrom", "yearTo"), ("marksMin", "marksMax")] {
        if num(f, a).zip(num(f, b)).is_some_and(|(a, b)| a > b) {
            return Err(bad("筛选范围的起点不能大于终点"));
        }
    }
    if (value(f, "subject") == "9708" && value(f, "paperType") == "paper3")
        || (value(f, "subject") == "9702"
            && ["paper2dr", "paper2essay"].contains(&value(f, "paperType").as_str()))
    {
        return Err(bad("当前学科不支持这个卷别"));
    }
    for k in [
        "primaryOnly",
        "includeSecondary",
        "needsReview",
        "excludeReview",
        "excludeLowConfidence",
        "strictPaperSyllabus",
    ] {
        if f.get(k).is_some_and(|s| {
            !s.is_empty() && !["true", "false", "1", "0", "on", "off"].contains(&s.as_str())
        }) {
            return Err(bad(format!("{k} 必须是布尔值")));
        }
    }
    Ok(())
}
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"\d+|\D+").unwrap());
    let aa: Vec<_> = re.find_iter(a).map(|m| m.as_str()).collect();
    let bb: Vec<_> = re.find_iter(b).map(|m| m.as_str()).collect();
    for (a, b) in aa.iter().zip(&bb) {
        let c = match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            _ => a.to_lowercase().cmp(&b.to_lowercase()),
        };
        if !c.is_eq() {
            return c;
        }
    }
    aa.len().cmp(&bb.len())
}
pub fn selected(qs: Vec<Value>, f: &Filters) -> Result<Vec<Value>, ApiError> {
    validate(f)?;
    let essays: HashSet<String> = qs
        .iter()
        .filter(|q| number(q, "marks") >= 8.0)
        .map(top)
        .collect();
    let primary = flag(f, "primaryOnly")
        || f.get("includeSecondary")
            .is_some_and(|s| s == "false" || s == "0");
    let keyword = value(f, "keyword");
    // Match SQLite LIKE wildcard semantics used by the previous bank.
    let pattern = format!(
        "(?is){}",
        keyword
            .chars()
            .map(|c| match c {
                '%' => ".*".into(),
                '_' => ".".into(),
                c => regex::escape(&c.to_string()),
            })
            .collect::<String>()
    );
    let matcher = regex::Regex::new(&pattern).map_err(internal)?;
    let mut rows:Vec<Value>=qs.into_iter().filter(|q|{
        let raw=&q["legacy_metadata"]["question"];let p=if q["legacy_metadata"]["paper"].is_object(){&q["legacy_metadata"]["paper"]}else{&q["source_metadata"]};
        let eq=|key:&str,v:&str| f.get(key).is_none_or(|s|s.is_empty()||s==v);
        if !eq("subject",text(q,"subject"))||!eq("sourceType",text(p,"source_type"))||!eq("component",text(p,"component"))||!eq("variant",text(p,"variant"))||!eq("paperId",text(q,"source"))||!eq("paper",text(q,"source"))||!eq("season",text(p,"season"))||!eq("confidence",text(raw,"confidence")){return false;}
        if let Some(pt)=f.get("paperType").filter(|s|!s.is_empty()){
            let base=if pt.starts_with("paper2"){"paper2"}else{pt};if text(q,"paper_type")!=base{return false;}
            if ["paper2dr","paper2essay"].contains(&pt.as_str())&&text(q,"subject")!="9708"{return false;}
            if text(q,"subject")=="9708"&&((pt=="paper2dr"&&essays.contains(&top(q)))||(pt=="paper2essay"&&!essays.contains(&top(q)))){return false;}
        }
        for (key,n,op) in [("year",number(q,"year"),0),("yearFrom",number(q,"year"),1),("yearTo",number(q,"year"),2),("marksMin",number(q,"marks"),1),("marksMax",number(q,"marks"),2)]{if num(f,key).is_some_and(|v|match op{0=>n!=v,1=>n<v,_=>n>v}){return false;}}
        if flag(f,"needsReview")&&raw["needs_review"]!=1{return false;}
        if flag(f,"excludeReview")&&raw["needs_review"]!=0{return false;}
        if flag(f,"excludeLowConfidence")&&raw["confidence"]=="low"{return false;}
        if flag(f,"strictPaperSyllabus"){
            let max=if text(q,"subject")=="9702"{11}else{6};
            let chapters:Vec<i64>=tags(q).map(|t|text(t,"syllabus_ref").split('.').next().unwrap_or("").parse().unwrap_or(0)).collect();
            if chapters.is_empty()||chapters.iter().any(|c|!(1..=max).contains(c)){return false;}
        }
        let eligible=||tags(q).filter(|t|(!primary||t["tag_role"]=="primary")&&match value(f,"role").as_str(){"primary"=>t["tag_role"]=="primary","secondary"=>t["tag_role"]=="secondary",_=>true});
        for (fk,tk) in [("topic","topic"),("subtopic","subtopic"),("tagId","tag_id"),("tag","tag_id")] {if f.get(fk).filter(|s|!s.is_empty()).is_some_and(|s|!eligible().any(|t|text(t,tk)==s)&&!(fk=="topic"&&q["legacy_metadata"].is_null()&&q["topics"].as_array().is_some_and(|ts|ts.iter().any(|t|t==s)))){return false;}}
        if f.get("syllabusRef").filter(|s|!s.is_empty()).is_some_and(|s|!eligible().any(|t|text(t,"syllabus_ref")==s||text(t,"syllabus_ref").starts_with(&format!("{s}.")))){return false;}
        if f.get("knowledgePointText").filter(|s|!s.is_empty()).is_some_and(|s|!eligible().any(|t|["knowledge_point_cn","knowledge_point_en","keywords"].iter().any(|k|text(t,k).to_lowercase().contains(&s.to_lowercase())))){return false;}
        if !keyword.is_empty()&&!([text(q,"source"),text(p,"qp_file"),text(p,"ms_file"),text(q,"question_no"),text(raw,"evidence")].iter().any(|s|matcher.is_match(s))||tags(q).any(|t|["tag_id","topic","topic_cn","subtopic","subtopic_cn","knowledge_point_cn","knowledge_point_en","keywords"].iter().any(|k|matcher.is_match(text(t,k))))||(q["legacy_metadata"].is_null()&&matcher.is_match(&json!({"text":q["text"],"answer":q["answer"],"title":q["title"],"topics":q["topics"]}).to_string()))){return false;}
        let query=f.get("query").or_else(||f.get("q")).map(|s|s.to_lowercase()).unwrap_or_default();
        query.is_empty()||q.to_string().to_lowercase().contains(&query)
    }).collect();
    rows.sort_by(|a, b| {
        let custom = match value(f, "sortKey").as_str() {
            "marks" => number(a, "marks").total_cmp(&number(b, "marks")),
            "paper" => natural(text(a, "source"), text(b, "source")),
            "question" => natural(text(a, "question_no"), text(b, "question_no")),
            "confidence" => number(&a["legacy_metadata"]["question"], "confidence_score")
                .total_cmp(&number(
                    &b["legacy_metadata"]["question"],
                    "confidence_score",
                )),
            "review" => number(&a["legacy_metadata"]["question"], "needs_review")
                .total_cmp(&number(&b["legacy_metadata"]["question"], "needs_review")),
            "section" | "chapter" => natural(
                a["syllabus_refs"][0].as_str().unwrap_or(""),
                b["syllabus_refs"][0].as_str().unwrap_or(""),
            ),
            _ => std::cmp::Ordering::Equal,
        };
        let custom = if value(f, "sortDirection") == "desc" {
            custom.reverse()
        } else {
            custom
        };
        if !custom.is_eq() {
            return custom;
        }
        if value(f, "sort") == "marks_desc" {
            let c = number(b, "marks").total_cmp(&number(a, "marks"));
            if !c.is_eq() {
                return c;
            }
        }
        let year = number(b, "year").total_cmp(&number(a, "year"));
        let year = if value(f, "sort") == "year_asc" {
            year.reverse()
        } else {
            year
        };
        year.then_with(|| {
            text(&b["legacy_metadata"]["paper"], "season")
                .cmp(text(&a["legacy_metadata"]["paper"], "season"))
        })
        .then_with(|| natural(text(a, "source"), text(b, "source")))
        .then_with(|| {
            number(&a["legacy_metadata"]["question"], "question_order")
                .total_cmp(&number(&b["legacy_metadata"]["question"], "question_order"))
        })
        .then_with(|| natural(text(a, "id"), text(b, "id")))
    });
    Ok(rows)
}
pub async fn questions(
    State(app): State<App>,
    Query(f): Query<Filters>,
) -> Result<Json<Value>, ApiError> {
    let rows = selected(all_questions(&app)?, &f)?;
    let total = rows.len();
    let marks: f64 = rows.iter().map(|q| number(q, "marks")).sum();
    let offset = num(&f, "offset").unwrap_or(0.0) as usize;
    let limit = (num(&f, "limit").unwrap_or(100.0) as usize).clamp(1, 500);
    Ok(Json(
        json!({"questions":rows.into_iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":total,"marks":marks,"offset":offset,"limit":limit,"engine":"rust"}),
    ))
}
pub async fn options(
    State(app): State<App>,
    Query(f): Query<Filters>,
) -> Result<Json<Value>, ApiError> {
    let mut only = Filters::new();
    if let Some(s) = f.get("subject") {
        only.insert("subject".into(), s.clone());
    }
    let qs = selected(all_questions(&app)?, &only)?;
    let mut fields: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut points = BTreeMap::new();
    let mut topics = BTreeMap::new();
    let mut subs = BTreeMap::new();
    for q in &qs {
        for (key, val) in [
            ("paperIds", text(q, "source")),
            (
                "years",
                q["year"]
                    .as_i64()
                    .map(|n| n.to_string())
                    .as_deref()
                    .unwrap_or(""),
            ),
            ("seasons", text(&q["legacy_metadata"]["paper"], "season")),
            (
                "components",
                text(&q["legacy_metadata"]["paper"], "component"),
            ),
            ("variants", text(&q["legacy_metadata"]["paper"], "variant")),
        ] {
            if !val.is_empty() {
                fields.entry(key).or_default().insert(val.into());
            }
        }
        for t in tags(q) {
            let mut translated = t.clone();
            syllabus::overlay(text(q, "subject"), &mut translated);
            let id = format!("{}:{}", text(q, "subject"), text(t, "tag_id"));
            points.entry(id).or_insert_with(||json!({"tagId":t["tag_id"],"subject":q["subject"],"syllabusRef":t["syllabus_ref"],"en":translated["knowledge_point_en"],"cn":translated["knowledge_point_cn"]}));
            for (map, key, cn) in [
                (&mut topics, "topic", "topic_cn"),
                (&mut subs, "subtopic", "subtopic_cn"),
            ] {
                map.entry(text(t, key).to_owned())
                    .or_insert_with(|| json!({"value":t[key],"en":t[key],"cn":t[cn]}));
            }
        }
    }
    {
        let db = app.db.lock().unwrap();
        let mut st=db.prepare("SELECT subject,data FROM syllabus_points WHERE (?1='' OR subject=?1) ORDER BY subject,tag_id").map_err(internal)?;
        let subject = only.get("subject").map(String::as_str).unwrap_or("");
        let rs = st
            .query_map([subject], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(internal)?;
        for r in rs {
            let (s, raw) = r.map_err(internal)?;
            let mut t: Value = serde_json::from_str(&raw).map_err(internal)?;
            syllabus::overlay(&s, &mut t);
            points.insert(format!("{}:{}",s,text(&t,"tag_id")),json!({"tagId":t["tag_id"],"subject":s,"syllabusRef":t["syllabus_ref"],"en":t["knowledge_point_en"],"cn":t["knowledge_point_cn"]}));
            topics.insert(
                text(&t, "topic").into(),
                json!({"value":t["topic"],"en":t["topic"],"cn":t["topic_cn"]}),
            );
        }
    }
    let mut result = serde_json::to_value(fields).map_err(internal)?;
    let mut topics: Vec<_> = topics.into_values().collect();
    topics.sort_by(|a, b| natural(text(a, "value"), text(b, "value")));
    let mut subs: Vec<_> = subs.into_values().collect();
    subs.sort_by(|a, b| natural(text(a, "value"), text(b, "value")));
    result["topics"] = json!(topics);
    result["subtopics"] = json!(subs);
    result["tags"] = json!(points.into_values().collect::<Vec<_>>());
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_and_secondary_are_not_silently_broadened() {
        let q = |id: &str, r: &str, m: i64| json!({"id":id,"subject":"9708","source":"p","question_no":r,"marks":m,"paper_type":"paper2","year":2025,"legacy_metadata":{"question":{"confidence":"high","needs_review":0},"paper":{"season":"s"},"tags":[{"tag_id":"a","tag_role":"primary","syllabus_ref":"1.1","topic":"X"},{"tag_id":"b","tag_role":"secondary","syllabus_ref":"2.1","topic":"Y"}]}});
        let qs = vec![
            q("1", "1(a)", 2),
            q("2", "1(b)", 6),
            q("3", "2(a)", 8),
            q("4", "2(b)", 12),
        ];
        let mut f = Filters::from([("paperType".into(), "paper2dr".into())]);
        assert_eq!(selected(qs.clone(), &f).unwrap().len(), 2);
        f.insert("paperType".into(), "paper2essay".into());
        assert_eq!(selected(qs.clone(), &f).unwrap().len(), 2);
        f.insert("primaryOnly".into(), "true".into());
        f.insert("tagId".into(), "b".into());
        assert_eq!(selected(qs, &f).unwrap().len(), 0);
        f.insert("yearFrom".into(), "2026".into());
        f.insert("yearTo".into(), "2020".into());
        assert!(validate(&f).is_err());
    }
}
