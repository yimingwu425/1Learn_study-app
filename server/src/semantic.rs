//! Compatibility with the original builtin-hybrid-v1 embeddings. This is retrieval, not an OCR model.
use super::*;
use std::collections::{HashMap, HashSet};
pub fn embedding(query: &str) -> Vec<f32> {
    let concepts: Vec<Vec<String>> =
        serde_json::from_str(include_str!("../assets/concepts.json")).unwrap();
    let mut vector = vec![0f32; 128];
    let lower = query.to_lowercase();
    for (i, group) in concepts.iter().enumerate() {
        let mut strength: f64 = 0.0;
        for term in group {
            let t = term.to_lowercase();
            let matched = if term.is_ascii() {
                regex::Regex::new(&format!("(^|[^a-z0-9]){}([^a-z0-9]|$)", regex::escape(&t)))
                    .unwrap()
                    .is_match(&lower)
            } else {
                lower.contains(&t)
            };
            if matched {
                strength += if term.chars().count() > 3 { 1.5 } else { 1.0 };
            }
        }
        if strength > 0.0 {
            vector[i] = (vector[i] as f64 + strength.ln_1p() * 2.0) as f32;
            let j = (i + 32) % 64;
            vector[j] = (vector[j] as f64 + strength.ln_1p() * 0.8) as f32;
        }
    }
    let en = regex::Regex::new(r"[a-zA-Z0-9_\-\.\$]+").unwrap();
    let zh = regex::Regex::new(r"[一-龥]{1,4}").unwrap();
    let mut seen = HashSet::new();
    for token in en
        .find_iter(&lower)
        .map(|m| m.as_str())
        .chain(zh.find_iter(query).map(|m| m.as_str()))
    {
        if !seen.insert(token) {
            continue;
        }
        let hash = md5::Md5::digest(token.as_bytes());
        let h = u32::from_be_bytes(hash[..4].try_into().unwrap());
        let a = 64 + (h % 32) as usize;
        let b = 96 + (((h >> 8) & 0xffff) % 32) as usize;
        let sign = if h & 1 == 1 { 1.0 } else { -1.0 };
        vector[a] = (vector[a] as f64 + sign) as f32;
        vector[b] = (vector[b] as f64 - sign * 0.7) as f32;
    }
    let norm = vector
        .iter()
        .map(|x| (*x as f64) * (*x as f64))
        .sum::<f64>()
        .sqrt();
    if norm > 1e-8 {
        for v in &mut vector {
            *v = (*v as f64 / norm) as f32
        }
    } else {
        vector[0] = 1.0;
    }
    vector
}
pub async fn search(app: &App, args: &Value) -> Result<Value, ApiError> {
    let subject = args["subject"].as_str().unwrap_or("9708");
    let query = args["query"].as_str().unwrap_or("").trim();
    if query.len() > 4000 {
        return Err(bad("检索语句过长"));
    }
    if query.is_empty() {
        return Ok(json!({"query":query,"subject":subject,"total":0,"results":[]}));
    }
    let path = app
        .root
        .join("legacy-snapshots")
        .join(format!("{subject}.sqlite"));
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| err(StatusCode::SERVICE_UNAVAILABLE, "原题库向量快照尚未迁入"))?;
    let (model,dims):(String,usize)=db.query_row("SELECT model_name,dimensions FROM question_embeddings GROUP BY model_name,dimensions ORDER BY count(*) DESC,model_name ASC LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(internal)?;
    let vector = if model == "builtin-hybrid-v1" {
        embedding(query)
    } else {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE,format!("本题库使用 {model}；Rust 查询端尚未配置对应嵌入服务，未回退到不同向量空间。请使用保留的旧 Agent 服务。")));
    };
    if dims != vector.len() {
        return Err(bad("查询向量与原题库维度不一致，无法可靠检索"));
    }
    let mut filters = if args["filters"].is_object() {
        bank::from_json(&args["filters"])?
    } else {
        bank::Filters::new()
    };
    filters.insert("subject".into(), subject.into());
    for key in ["paperType", "yearFrom", "yearTo"] {
        if let Some(v) = args.get(key).filter(|v| !v.is_null()) {
            filters.insert(
                key.into(),
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string()),
            );
        }
    }
    let qs = bank::selected(all_questions(app)?, &filters)?;
    let by_id: HashMap<i64, &Value> = qs
        .iter()
        .filter_map(|q| {
            q["legacy_metadata"]["question"]["question_id"]
                .as_i64()
                .map(|id| (id, q))
        })
        .collect();
    let mut st=db.prepare("SELECT question_id,vector FROM question_embeddings WHERE model_name=?1 AND dimensions=?2 ORDER BY question_id").map_err(internal)?;
    let rows = st
        .query_map(params![model, dims], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
        })
        .map_err(internal)?;
    let mut scored = Vec::new();
    for r in rows {
        let (id, bytes) = r.map_err(internal)?;
        if !by_id.contains_key(&id) {
            continue;
        }
        if bytes.len() != dims * 4 {
            return Err(bad("题库中有无效维度的向量，请先核对向量库"));
        }
        let sim: f64 = bytes
            .chunks_exact(4)
            .zip(&vector)
            .map(|(b, q)| f32::from_le_bytes(b.try_into().unwrap()) as f64 * (*q as f64))
            .sum();
        if sim >= 0.08 {
            scored.push((id, sim));
        }
    }
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let limit = args["limit"].as_u64().unwrap_or(10).clamp(1, 30) as usize;
    let results: Vec<_> = scored
        .into_iter()
        .take(limit)
        .map(|(id, sim)| {
            let q = by_id[&id];
            let mut item = crate::agent::question_result(q);
            item["similarity"] = json!(sim);
            item["matchScorePct"] = json!((sim * 100.0).round());
            item["question"] = q.clone();
            item
        })
        .collect();
    Ok(
        json!({"query":query,"subject":subject,"total":results.len(),"results":results,"engine":"rust","model":"builtin-hybrid-v1","notice":"本地双语概念与散列检索；不是 AI 识别模型或准确率。"}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalized_and_ascii_boundaries() {
        let a = embedding("shadow has");
        let b = embedding("ad as");
        assert_ne!(a, b);
        let q = embedding("内阻 internal resistance");
        let n: f64 = q.iter().map(|v| (*v as f64).powi(2)).sum();
        assert!((n - 1.0).abs() < 1e-6);
    }
}
