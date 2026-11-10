//! Persistent manual selection. Authoritative questions are reloaded at every export.
use super::*;
use std::collections::HashSet;
fn subject(v: &Value) -> Result<&str, ApiError> {
    v["subject"]
        .as_str()
        .filter(|s| ["9702", "9708"].contains(s))
        .ok_or_else(|| bad("请选择物理或经济"))
}
fn ids(v: &Value) -> Result<Vec<String>, ApiError> {
    let ids = v["ids"].as_array().ok_or_else(|| bad("试卷篮格式无效"))?;
    if ids.len() > 200 {
        return Err(bad("每个学科最多选择 200 条小问"));
    }
    let ids = ids
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| bad("题目标识无效"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if ids.iter().collect::<HashSet<_>>().len() != ids.len() {
        return Err(bad("试卷篮含有重复题目"));
    }
    Ok(ids)
}
fn hydrate(app: &App, subject: &str, ids: &[String]) -> Result<Vec<Value>, ApiError> {
    ids.iter()
        .map(|id| {
            let q = get_question(app, id)?;
            if q["subject"] != subject {
                return Err(bad("请将不同学科分别组卷"));
            }
            if q["kind"] == "demo" {
                return Err(bad("演示题不能作为真题组卷"));
            }
            Ok(q)
        })
        .collect()
}
pub async fn get(
    State(app): State<App>,
    Query(query): Query<bank::Filters>,
) -> Result<Json<Value>, ApiError> {
    let s = query.get("subject").map(String::as_str).unwrap_or("9702");
    subject(&json!({"subject":s}))?;
    let raw: Option<String> = app
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT value FROM settings WHERE key=?1",
            [format!("basket:{s}")],
            |r| r.get(0),
        )
        .optional()
        .map_err(internal)?;
    let ids: Vec<String> = raw
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(internal)?
        .unwrap_or_default();
    let qs = hydrate(&app, s, &ids)?;
    Ok(Json(
        json!({"subject":s,"ids":ids,"questions":qs,"maxItems":200}),
    ))
}
pub async fn save(
    State(app): State<App>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let s = subject(&input)?;
    let ids = ids(&input)?;
    hydrate(&app, s, &ids)?;
    app.db.lock().unwrap().execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![format!("basket:{s}"),serde_json::to_string(&ids).map_err(internal)?]).map_err(internal)?;
    Ok(Json(json!({"saved":true,"ids":ids})))
}
pub async fn export(
    State(app): State<App>,
    Json(input): Json<Value>,
) -> Result<Response, ApiError> {
    let s = subject(&input)?;
    let ids = ids(&input)?;
    if ids.is_empty() {
        return Err(bad("先选几道题加入试卷篮"));
    }
    let qs = hydrate(&app, s, &ids)?;
    let kinds: HashSet<_> = qs.iter().map(|q| bank::text(q, "paper_type")).collect();
    if kinds.len() != 1 {
        return Err(bad("请将不同卷别分别组卷，选择题与结构题需要分开"));
    }
    if qs.iter().any(|q| q["legacy_metadata"].is_null()) {
        let Json(mut result) = export_packet(
            State(app.clone()),
            Json(ExportRequest {
                question_ids: ids,
                mode: Some("practice".into()),
            }),
        )
        .await?;
        result["path"] = json!(app
            .data
            .join("exports")
            .join(format!("{}.zip", result["id"].as_str().unwrap())));
        result["format"] = json!("complete_source_pages");
        return Ok(Json(result).into_response());
    }
    if qs
        .iter()
        .any(|q| q["source"].as_str().is_none() || q["question_no"].as_str().is_none())
    {
        return Err(bad("所选题目尚未具有可靠的原题映射，请先核对来源"));
    }
    let title = input["title"]
        .as_str()
        .unwrap_or("Selected practice")
        .chars()
        .take(120)
        .collect::<String>();
    let plan = composer::manual(&qs, s, &title)?;
    let bytes = native_assembler::bundle(&app, &plan).await?;
    let id = uuid::Uuid::new_v4().to_string();
    let dir = app.data.join("exports");
    std::fs::create_dir_all(&dir).map_err(internal)?;
    let path = dir.join(format!("{id}.zip"));
    std::fs::write(&path, bytes).map_err(internal)?;
    app.db
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO exports(id,path,created_at) VALUES(?1,?2,?3)",
            params![id, path.to_string_lossy(), now()],
        )
        .map_err(internal)?;
    Ok(Json(json!({"id":id,"url":format!("/api/exports/{id}"),"path":path})).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn persisted_order_and_invalid_updates_preserve_the_basket() {
        let dir = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        init_db(&db).unwrap();
        for (id, s) in [("a", "9702"), ("b", "9702"), ("c", "9708")] {
            db.execute(
                "INSERT INTO questions VALUES(?1,?2,?3)",
                params![
                    id,
                    s,
                    json!({"id":id,"subject":s,"kind":"real"}).to_string()
                ],
            )
            .unwrap();
        }
        let app = App {
            siwc: Arc::new(siwc::Service::new(dir.path())),
            db: Arc::new(Mutex::new(db)),
            data: dir.path().into(),
            root: dir.path().into(),
        };
        let _ = save(
            State(app.clone()),
            Json(json!({"subject":"9702","ids":["b","a"]})),
        )
        .await
        .unwrap();
        assert!(save(
            State(app.clone()),
            Json(json!({"subject":"9702","ids":["a","c"]}))
        )
        .await
        .is_err());
        assert!(save(
            State(app.clone()),
            Json(json!({"subject":"9702","ids":["a","a"]}))
        )
        .await
        .is_err());
        let Json(b) = get(
            State(app),
            Query(bank::Filters::from([("subject".into(), "9702".into())])),
        )
        .await
        .unwrap();
        assert_eq!(b["ids"], json!(["b", "a"]));
    }
}
