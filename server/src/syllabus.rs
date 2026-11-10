use super::*;
fn catalog() -> &'static Value {
    static C: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/syllabus/catalog.json"))
            .expect("bundled bilingual catalog")
    })
}
pub fn overlay(subject: &str, p: &mut Value) {
    let id = p["tag_id"].as_str().unwrap_or("").to_owned();
    let entry = &catalog()[subject]["points"][&id];
    if entry.is_null() {
        return;
    }
    let chapter = p["syllabus_ref"]
        .as_str()
        .unwrap_or("")
        .split('.')
        .next()
        .unwrap_or("")
        .to_owned();
    if let Some(cn) = catalog()[subject]["chapters"][&chapter].as_str() {
        p["original_topic_cn"] = p["topic_cn"].clone();
        p["topic_cn"] = json!(format!("{chapter} {cn}"));
    }
    p["original_knowledge_point_en"] = p["knowledge_point_en"].clone();
    p["original_knowledge_point_cn"] = p["knowledge_point_cn"].clone();
    p["knowledge_point_en"] = entry["en"].clone();
    p["knowledge_point_cn"] = entry["cn"].clone();
    p["syllabus_page"] = entry["page"].clone();
    p["syllabus_source"] = catalog()[subject]["source"].clone();
    p["syllabus_sha256"] = catalog()[subject]["sourceSha256"].clone();
    p["translation_note"] = catalog()[subject]["translationNote"].clone();
}
pub async fn source(
    State(app): State<App>,
    Path(subject): Path<String>,
) -> Result<Response, ApiError> {
    if !["9702", "9708"].contains(&subject.as_str()) {
        return Err(bad("学科无效"));
    }
    let bytes = tokio::fs::read(
        app.root
            .join("server/assets/syllabus")
            .join(format!("{subject}.pdf")),
    )
    .await
    .map_err(|_| err(StatusCode::NOT_FOUND, "本地考纲副本不存在"))?;
    Ok((
        [
            ("content-type", "application/pdf"),
            ("content-disposition", "inline"),
        ],
        bytes,
    )
        .into_response())
}
