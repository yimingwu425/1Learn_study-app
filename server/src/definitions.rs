//! Read-only, source-backed definition catalogue. No inference or user-record mutation.
use super::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};
const CATALOG_JSON: &str = include_str!("../assets/definitions/catalog.json");
fn catalog() -> &'static Value {
    static C: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    C.get_or_init(|| serde_json::from_str(CATALOG_JSON).expect("definition catalogue"))
}
#[derive(Default)]
struct Filters {
    subject: String,
    chapter: Option<u64>,
    from: Option<u64>,
    to: Option<u64>,
    query: String,
    sort: String,
    offset: usize,
    limit: usize,
}
fn filters(f: &HashMap<String, String>) -> Result<Filters, ApiError> {
    for k in f.keys() {
        if ![
            "subject",
            "chapter",
            "year_from",
            "year_to",
            "query",
            "sort",
            "offset",
            "limit",
        ]
        .contains(&k.as_str())
        {
            return Err(bad("未识别的定义库筛选参数"));
        }
    }
    let number = |key: &str| -> Result<Option<u64>, ApiError> {
        f.get(key)
            .filter(|v| !v.is_empty())
            .map(|v| v.parse().map_err(|_| bad("定义库筛选数值无效")))
            .transpose()
    };
    let subject = f.get("subject").cloned().unwrap_or_else(|| "9702".into());
    if !["9702", "9708"].contains(&subject.as_str()) {
        return Err(bad("当前定义库仅有物理9702与经济9708的实际来源"));
    }
    let chapter = number("chapter")?;
    if chapter.is_some_and(|c| c == 0 || c > 30) {
        return Err(bad("章节编号无效"));
    }
    let from = number("year_from")?;
    let to = number("year_to")?;
    if from.is_some_and(|v| !(1900..=2100).contains(&v))
        || to.is_some_and(|v| !(1900..=2100).contains(&v))
        || from.zip(to).is_some_and(|(a, b)| a > b)
    {
        return Err(bad("请检查定义库年份范围"));
    }
    let sort = f.get("sort").cloned().unwrap_or_else(|| "syllabus".into());
    if !["syllabus", "frequency"].contains(&sort.as_str()) {
        return Err(bad("定义排序无效"));
    }
    let offset = number("offset")?.unwrap_or(0) as usize;
    let limit = number("limit")?.unwrap_or(8) as usize;
    if limit == 0 || limit > 24 {
        return Err(bad("每页定义数量须为1–24"));
    }
    Ok(Filters {
        subject,
        chapter,
        from,
        to,
        query: f.get("query").cloned().unwrap_or_default().to_lowercase(),
        sort,
        offset,
        limit,
    })
}
fn source_ok(app: &App, id: &str, expected: &str) -> bool {
    type Cache = HashMap<PathBuf, (std::time::SystemTime, u64, String)>;
    static HASHES: std::sync::OnceLock<Mutex<Cache>> = std::sync::OnceLock::new();
    let path = app.data.join("files").join(format!("{id}.pdf"));
    let Ok(m) = std::fs::metadata(&path) else {
        return false;
    };
    let Ok(time) = m.modified() else { return false };
    let mut cache = HASHES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    if let Some((old, len, hash)) = cache.get(&path) {
        if *old == time && *len == m.len() {
            return hash == expected;
        }
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return false;
    };
    let hash = format!("{:x}", Sha256::digest(bytes));
    let valid = hash == expected;
    cache.insert(path, (time, m.len(), hash));
    valid
}
fn numeric_ref(t: &Value) -> Vec<u64> {
    t["syllabusRef"]
        .as_str()
        .unwrap_or("")
        .split('.')
        .filter_map(|n| n.parse().ok())
        .collect()
}
fn occurrence_key(o: &Value) -> String {
    format!(
        "{}:{}",
        o["qp"]["sha256"].as_str().unwrap_or(""),
        o["questionRef"].as_str().unwrap_or("")
    )
}
fn select(c: &Value, f: &Filters, available: impl Fn(&Value) -> bool) -> Value {
    let mut terms = vec![];
    let mut years: BTreeMap<u64, usize> = BTreeMap::new();
    let mut papers = BTreeSet::new();
    let mut count = 0;
    let mut missing = 0;
    let mut chapters: BTreeMap<u64, Value> = BTreeMap::new();
    for t in c["terms"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["subject"] == f.subject)
    {
        let n = t["chapter"].as_u64().unwrap();
        chapters.entry(n).or_insert_with(
            || json!({"chapter":n,"titleEn":t["chapterEn"],"titleCn":t["chapterCn"]}),
        );
        if f.chapter.is_some_and(|ch| n != ch) {
            continue;
        }
        if !f.query.is_empty()
            && !format!(
                "{} {} {} {}",
                t["termEn"], t["termCn"], t["reciteEn"], t["syllabusRef"]
            )
            .to_lowercase()
            .contains(&f.query)
        {
            continue;
        }
        let mut seen = BTreeSet::new();
        let occurrences = t["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| {
                let y = o["year"].as_u64().unwrap();
                f.from.is_none_or(|a| y >= a) && f.to.is_none_or(|a| y <= a)
            })
            .filter(|o| seen.insert(occurrence_key(o)))
            .filter(|o| {
                if available(o) {
                    true
                } else {
                    missing += 1;
                    false
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        if occurrences.is_empty() {
            continue;
        }
        let mut term = t.clone();
        let mut term_papers = BTreeSet::new();
        let mut trend = BTreeMap::new();
        for o in &occurrences {
            let y = o["year"].as_u64().unwrap();
            *years.entry(y).or_default() += 1;
            *trend.entry(y).or_insert(0usize) += 1;
            papers.insert(o["qp"]["sha256"].as_str().unwrap().to_owned());
            term_papers.insert(o["qp"]["sha256"].as_str().unwrap().to_owned());
        }
        count += occurrences.len();
        term["occurrences"] = json!(occurrences);
        term["occurrenceCount"] = json!(term["occurrences"].as_array().unwrap().len());
        term["paperCount"] = json!(term_papers.len());
        term["trend"] = json!(trend
            .into_iter()
            .map(|(year, count)| json!({"year":year,"count":count}))
            .collect::<Vec<_>>());
        terms.push(term);
    }
    terms.sort_by(|a, b| {
        numeric_ref(a)
            .cmp(&numeric_ref(b))
            .then_with(|| a["termEn"].as_str().cmp(&b["termEn"].as_str()))
    });
    let mut ranking=terms.iter().map(|t|json!({"id":t["id"],"termEn":t["termEn"],"termCn":t["termCn"],"count":t["occurrenceCount"],"papers":t["paperCount"]})).collect::<Vec<_>>();
    ranking.sort_by(|a, b| {
        b["count"]
            .as_u64()
            .cmp(&a["count"].as_u64())
            .then_with(|| a["termEn"].as_str().cmp(&b["termEn"].as_str()))
    });
    if f.sort == "frequency" {
        terms.sort_by(|a, b| {
            b["occurrenceCount"]
                .as_u64()
                .cmp(&a["occurrenceCount"].as_u64())
                .then_with(|| numeric_ref(a).cmp(&numeric_ref(b)))
        })
    }
    let total = terms.len();
    let selected = terms
        .into_iter()
        .skip(f.offset)
        .take(f.limit)
        .collect::<Vec<_>>();
    let scanned = c["scannedPapers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["subject"] == f.subject)
        .count();
    json!({"catalogSha256":format!("{:x}",Sha256::digest(CATALOG_JSON.as_bytes())),"catalogVersion":1,"subject":f.subject,"definitions":selected,"total":total,"offset":f.offset,"limit":f.limit,"chapters":chapters.into_values().collect::<Vec<_>>(),"ranking":ranking.into_iter().take(8).collect::<Vec<_>>(),"trend":years.into_iter().map(|(year,count)|json!({"year":year,"count":count})).collect::<Vec<_>>(),"sample":{"terms":total,"occurrences":count,"papers":papers.len(),"unavailableSourceRecords":missing,"scannedPapers":scanned,"sourceScanTotal":c["scannedPapers"].as_array().unwrap().len(),"candidateCount":c["candidateCount"],"pendingSourceMatches":c["pendingCandidates"].as_array().unwrap().len()},"scope":c["scope"],"dedupRule":c["dedupRule"],"frequencyInterpretation":"次数是当前收录范围内已确认的定义考查记录，不是全库穷尽考频或未来出题概率；未收录不等于没考过。","answerVersion":"背诵版为整理答案；逐字来源、官方选择题答案与原卷页面分开保留。"})
}
pub fn routes() -> Router<App> {
    Router::new().route("/api/definitions", get(list))
}
async fn list(
    State(app): State<App>,
    Query(f): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let f = filters(&f)?;
    Ok(Json(select(catalog(), &f, |o| {
        source_ok(
            &app,
            o["qp"]["fileId"].as_str().unwrap(),
            o["qp"]["sha256"].as_str().unwrap(),
        ) && source_ok(
            &app,
            o["ms"]["fileId"].as_str().unwrap(),
            o["ms"]["sha256"].as_str().unwrap(),
        )
    })))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_catalogue_has_sources_and_never_counts_duplicate_rows() {
        let mut c = catalog().clone();
        let f = Filters {
            subject: "9702".into(),
            sort: "syllabus".into(),
            limit: 24,
            ..Default::default()
        };
        let before = select(&c, &f, |_| true);
        let duplicate = c["terms"][0]["occurrences"][0].clone();
        c["terms"][0]["occurrences"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        let after = select(&c, &f, |_| true);
        assert_eq!(before["sample"], after["sample"]);
        assert!(before["sample"]["occurrences"].as_u64().unwrap() > 0);
        for t in c["terms"].as_array().unwrap() {
            assert!(!t["reciteEn"].as_str().unwrap().is_empty());
            assert!(!t["occurrences"].as_array().unwrap().is_empty());
            for o in t["occurrences"].as_array().unwrap() {
                assert!(!o["ms"]["pages"].as_array().unwrap().is_empty());
                assert!(!o["originalMarkSchemeText"].as_str().unwrap().contains('©'));
            }
        }
    }
    #[test]
    fn years_chapters_and_missing_files_do_not_invent_zero_exam_claims() {
        let f = Filters {
            subject: "9708".into(),
            chapter: Some(2),
            from: Some(2026),
            to: Some(2026),
            sort: "syllabus".into(),
            limit: 24,
            ..Default::default()
        };
        let result = select(catalog(), &f, |_| true);
        for t in result["definitions"].as_array().unwrap() {
            assert_eq!(t["chapter"], 2);
            for o in t["occurrences"].as_array().unwrap() {
                assert_eq!(o["year"], 2026)
            }
        }
        let missing = select(catalog(), &f, |_| false);
        assert_eq!(missing["total"], 0);
        assert!(
            missing["sample"]["unavailableSourceRecords"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(filters(&HashMap::from([("subject".into(), "0000".into())])).is_err());
        assert!(filters(&HashMap::from([
            ("year_from".into(), "2026".into()),
            ("year_to".into(), "2020".into())
        ]))
        .is_err());
    }
}
