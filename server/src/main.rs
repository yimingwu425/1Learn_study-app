mod agent;
mod bank;
mod basket;
mod composer;
mod definitions;
mod library_features;
mod luna;
mod native_assembler;
mod native_packet;
mod pdf_document;
mod pdf_engine;
mod recognition;
mod semantic;
mod siwc;
mod source_validator;
mod syllabus;
use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use study_core::{identify_paper, valid_attempt_status, validate_candidate};
use tower_http::services::ServeDir;

#[derive(Clone)]
struct App {
    db: Arc<Mutex<Connection>>,
    data: PathBuf,
    root: PathBuf,
    siwc: Arc<siwc::Service>,
}
type ApiError = (StatusCode, Json<Value>);
fn err(code: StatusCode, s: impl ToString) -> ApiError {
    (code, Json(json!({"error":s.to_string()})))
}
fn bad(s: impl ToString) -> ApiError {
    err(StatusCode::BAD_REQUEST, s)
}
fn internal(s: impl ToString) -> ApiError {
    eprintln!("{}", s.to_string());
    err(
        StatusCode::INTERNAL_SERVER_ERROR,
        "操作失败，请查看本地日志",
    )
}
fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
fn init_db(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS questions(id TEXT PRIMARY KEY,subject TEXT NOT NULL,data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS attempts(id TEXT PRIMARY KEY,question_id TEXT NOT NULL REFERENCES questions(id),status TEXT NOT NULL,note TEXT NOT NULL,created_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS imports(id TEXT PRIMARY KEY,hash TEXT NOT NULL UNIQUE,data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS exports(id TEXT PRIMARY KEY,path TEXT NOT NULL,created_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS recognition_jobs(id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL UNIQUE,data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS syllabus_points(subject TEXT NOT NULL,tag_id TEXT NOT NULL,data TEXT NOT NULL,PRIMARY KEY(subject,tag_id));
")
}
fn get_question(app: &App, id: &str) -> Result<Value, ApiError> {
    let db = app.db.lock().unwrap();
    let d: String = db
        .query_row("SELECT data FROM questions WHERE id=?1", [id], |r| r.get(0))
        .map_err(|_| err(StatusCode::NOT_FOUND, "题目不存在"))?;
    serde_json::from_str(&d)
        .map(bank::normalize)
        .map_err(internal)
}
fn get_import(app: &App, id: &str) -> Result<Value, ApiError> {
    let db = app.db.lock().unwrap();
    let d: String = db
        .query_row("SELECT data FROM imports WHERE id=?1", [id], |r| r.get(0))
        .map_err(|_| err(StatusCode::NOT_FOUND, "导入记录不存在"))?;
    serde_json::from_str(&d).map_err(internal)
}
fn all_questions(app: &App) -> Result<Vec<Value>, ApiError> {
    let db = app.db.lock().unwrap();
    let mut st = db
        .prepare("SELECT data FROM questions ORDER BY id")
        .map_err(internal)?;
    let rows = st
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(internal)?;
    rows.map(|r| {
        serde_json::from_str(&r.map_err(internal)?)
            .map(bank::normalize)
            .map_err(internal)
    })
    .collect()
}
async fn guard(headers: HeaderMap, req: axum::extract::Request, next: Next) -> Response {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !(host == "127.0.0.1"
        || host.starts_with("127.0.0.1:")
        || host == "localhost"
        || host.starts_with("localhost:"))
    {
        return err(StatusCode::FORBIDDEN, "仅允许本机访问").into_response();
    }
    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        let expected = format!("http://{host}");
        let dev = std::env::var("STUDY_DEV_ORIGIN").ok();
        if origin != expected && dev.as_deref() != Some(origin) {
            return err(StatusCode::FORBIDDEN, "跨站请求已拒绝").into_response();
        }
    } else if req.method() != axum::http::Method::GET
        && req.method() != axum::http::Method::HEAD
        && headers.get("x-study-client").is_none()
    {
        return err(StatusCode::FORBIDDEN, "Agent写入请求需X-Study-Client标识").into_response();
    }
    let mut response = next.run(req).await;
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    response
        .headers_mut()
        .insert("referrer-policy", "no-referrer".parse().unwrap());
    response
        .headers_mut()
        .insert("x-frame-options", "SAMEORIGIN".parse().unwrap());
    response
}
async fn question(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    Ok(Json(get_question(&app, &id)?))
}
async fn topics(State(app): State<App>) -> Result<Json<Value>, ApiError> {
    let db = app.db.lock().unwrap();
    let mut st=db.prepare("SELECT questions.subject, tags.value, count(DISTINCT questions.id) FROM questions,json_each(questions.data,'$.topics') tags WHERE tags.type='text' GROUP BY questions.subject,tags.value ORDER BY tags.value").map_err(internal)?;
    let rows=st.query_map([],|r|{let title:String=r.get(1)?;Ok(json!({"id":title,"title":title,"subject":r.get::<_,String>(0)?,"count":r.get::<_,i64>(2)?}))}).map_err(internal)?;
    Ok(Json(
        json!({"topics":rows.collect::<Result<Vec<_>,_>>().map_err(internal)?}),
    ))
}
async fn attempts(State(app): State<App>) -> Result<Json<Value>, ApiError> {
    let db = app.db.lock().unwrap();
    let mut st=db.prepare("SELECT id,question_id,status,note,created_at FROM attempts ORDER BY created_at DESC LIMIT 1000").map_err(internal)?;
    let rs=st.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"question_id":r.get::<_,String>(1)?,"status":r.get::<_,String>(2)?,"note":r.get::<_,String>(3)?,"created_at":r.get::<_,String>(4)?}))).map_err(internal)?;
    Ok(Json(
        json!({"attempts":rs.collect::<Result<Vec<_>,_>>().map_err(internal)?}),
    ))
}
#[derive(Deserialize)]
struct Attempt {
    question_id: String,
    status: String,
    #[serde(default)]
    note: String,
}
async fn save_attempt(
    State(app): State<App>,
    Json(a): Json<Attempt>,
) -> Result<Json<Value>, ApiError> {
    if !valid_attempt_status(&a.status) {
        return Err(bad("请选择独立做对、提示后会了或还不会"));
    }
    if a.note.len() > 20000 {
        return Err(bad("笔记过长"));
    }
    get_question(&app, &a.question_id)?;
    let id = uuid::Uuid::new_v4().to_string();
    let time = now();
    app.db
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO attempts VALUES(?1,?2,?3,?4,?5)",
            params![id, a.question_id, a.status, a.note, time],
        )
        .map_err(internal)?;
    Ok(Json(
        json!({"id":id,"question_id":a.question_id,"status":a.status,"note":a.note,"created_at":time}),
    ))
}
async fn imports(State(app): State<App>) -> Result<Json<Value>, ApiError> {
    let db = app.db.lock().unwrap();
    let mut st = db
        .prepare("SELECT data FROM imports ORDER BY rowid DESC")
        .map_err(internal)?;
    let rs = st
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(internal)?;
    let records = rs
        .map(|r| serde_json::from_str::<Value>(&r.map_err(internal)?).map_err(internal))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({"imports":records})))
}
async fn inspect(app: &App, path: &std::path::Path) -> Result<Value, String> {
    pdf_engine::inspect(app, path)
        .await
        .map_err(|(_, Json(v))| v["error"].as_str().unwrap_or("PDF 预检失败").to_owned())
}
async fn upload(State(app): State<App>, mut multipart: Multipart) -> Result<Json<Value>, ApiError> {
    let mut records = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(bad)? {
        if records.len() >= 20 {
            return Err(bad("一次最多上传20份PDF"));
        }
        let filename = field
            .file_name()
            .unwrap_or("unnamed.pdf")
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("unnamed.pdf")
            .to_string();
        let bytes = field.bytes().await.map_err(bad)?;
        if !filename.to_lowercase().ends_with(".pdf") || !bytes.starts_with(b"%PDF-") {
            return Err(bad("仅支持有效PDF文件"));
        }
        if bytes.len() > 25 * 1024 * 1024 {
            return Err(bad("单份PDF不能超过25MB"));
        }
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let old: Option<String> = {
            let db = app.db.lock().unwrap();
            db.query_row("SELECT data FROM imports WHERE hash=?1", [&hash], |r| {
                r.get(0)
            })
            .ok()
        };
        if let Some(old) = old {
            let mut v: Value = serde_json::from_str(&old).map_err(internal)?;
            v["duplicate"] = json!(true);
            records.push(v);
            continue;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let path = app.data.join("files").join(format!("{id}.pdf"));
        tokio::fs::write(&path, &bytes).await.map_err(internal)?;
        let meta = identify_paper(&filename);
        let inspected = inspect(&app, &path).await;
        let (analysis, status, message) = match inspected {
            Ok(v) => (
                v,
                "awaiting_agent",
                "原件已保存并完成规则预检，等待识别Agent自动拆题与配对".to_string(),
            ),
            Err(e) => (
                json!({"pages":0,"candidates":[],"warnings":[e]}),
                "failed",
                "解析失败，原文件已保留，可修复后重试".into(),
            ),
        };
        let subject = meta.as_ref().map(|m| m.subject.clone()).unwrap_or_default();
        let exam = meta
            .as_ref()
            .map(|m| m.exam_code.clone())
            .unwrap_or_default();
        let role = meta
            .as_ref()
            .map(|m| m.role.clone())
            .unwrap_or("unknown".into());
        let record = json!({"id":id,"filename":filename,"hash":hash,"subject":subject,"exam_code":exam,"role":role,"pages":analysis["pages"],"inspection_engine":analysis["engine"],"status":status,"message":message,"candidates":analysis["candidates"],"warnings":analysis["warnings"],"file_url":format!("/api/files/{id}"),"created_at":now()});
        app.db
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO imports VALUES(?1,?2,?3)",
                params![id, hash, record.to_string()],
            )
            .map_err(internal)?;
        records.push(record);
    }
    let ids = records
        .iter()
        .filter_map(|r| r["id"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let job = recognition::create_for_imports(&app, &ids).ok();
    Ok(Json(json!({"imports":records,"recognition_job":job})))
}
#[derive(Deserialize)]
struct ImportMetadata {
    subject: String,
    exam_code: String,
    role: String,
}
fn valid_exam_code(exam: &str) -> bool {
    !exam.is_empty()
        && exam.len() <= 100
        && exam
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
}
async fn update_import(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(meta): Json<ImportMetadata>,
) -> Result<Json<Value>, ApiError> {
    if !["9702", "9708"].contains(&meta.subject.as_str())
        || !["qp", "ms"].contains(&meta.role.as_str())
        || !valid_exam_code(&meta.exam_code)
    {
        return Err(bad("请选择科目、题面或评分标准，并填写有效试卷编号"));
    }
    let mut record = get_import(&app, &id)?;
    if record["status"] == "published" {
        return Err(err(StatusCode::CONFLICT, "已发布试卷元数据不能在此修改"));
    }
    record["subject"] = json!(meta.subject);
    record["exam_code"] = json!(meta.exam_code);
    record["role"] = json!(meta.role);
    record["metadata_reviewed_at"] = json!(now());
    app.db
        .lock()
        .unwrap()
        .execute(
            "UPDATE imports SET data=?1 WHERE id=?2",
            params![record.to_string(), id],
        )
        .map_err(internal)?;
    Ok(Json(record))
}
#[derive(Deserialize)]
struct Publish {
    candidates: Vec<Value>,
    #[serde(default)]
    ms_import_id: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    exam_code: Option<String>,
}
async fn publish(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(p): Json<Publish>,
) -> Result<Json<Value>, ApiError> {
    let mut import = get_import(&app, &id)?;
    if import["status"] == "published" {
        return Err(err(StatusCode::CONFLICT, "这份试卷已入库，不会重复发布"));
    }
    if import["status"] == "failed" {
        return Err(bad("解析失败的PDF不能入库"));
    }
    if import["role"] == "ms" {
        return Err(bad("请在对应题面PDF上关联此评分标准"));
    }
    if p.candidates.is_empty() || p.candidates.len() > 300 {
        return Err(bad("请选择1至300道题"));
    }
    let subject = p
        .subject
        .or_else(|| import["subject"].as_str().map(str::to_owned))
        .unwrap_or_default();
    if !["9702", "9708"].contains(&subject.as_str()) {
        return Err(bad("请选择物理9702或经济9708"));
    }
    let exam = p
        .exam_code
        .or_else(|| import["exam_code"].as_str().map(str::to_owned))
        .unwrap_or_default();
    if exam.is_empty()
        || exam.len() > 100
        || !exam
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
    {
        return Err(bad(
            "试卷编号仅允许100位以内的英文字母、数字、下划线和连字符",
        ));
    }
    let ms = match &p.ms_import_id {
        Some(mid) => Some(get_import(&app, mid)?),
        None => None,
    };
    if let Some(m) = &ms {
        if m["role"] != "ms" {
            return Err(bad("所选答案文件不是评分标准"));
        }
        if m["exam_code"] != exam {
            return Err(bad("题面与评分标准试卷编号不同"));
        }
    }
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for c in p.candidates {
        validate_candidate(
            &c,
            import["pages"].as_u64().unwrap_or(0),
            ms.as_ref().and_then(|m| m["pages"].as_u64()),
        )
        .map_err(bad)?;
        let no = c["question_no"].as_str().unwrap();
        if !seen.insert(no.to_string()) {
            return Err(bad("存在重复题号"));
        }
        let qid = format!("{}:{}", exam, no);
        let qp = c["qp_pages"].as_array().unwrap();
        let msp = c["ms_pages"].as_array().cloned().unwrap_or_default();
        let q = json!({"id":qid,"subject":subject,"title":format!("第 {} 题",no),"text":c["text"],"marks":c.get("marks").cloned().unwrap_or(Value::Null),"source":exam,"question_no":no,"topics":c.get("topics").cloned().unwrap_or(json!([])),"qp_url":format!("/api/files/{}#page={}",id,qp[0]),"ms_url":if !msp.is_empty(){p.ms_import_id.as_ref().map(|mid|format!("/api/files/{}#page={}",mid,msp[0]))}else{None},"qp_file_id":id,"ms_file_id":p.ms_import_id,"qp_pages":qp,"ms_pages":msp,"status":"reviewed","kind":"real","content_reviewed_at":now()});
        rows.push(q)
    }
    let mut db = app.db.lock().unwrap();
    let tx = db.transaction().map_err(internal)?;
    for q in &rows {
        let exists: i64 = tx
            .query_row(
                "SELECT count(*) FROM questions WHERE id=?1",
                [q["id"].as_str().unwrap()],
                |r| r.get(0),
            )
            .map_err(internal)?;
        if exists > 0 {
            return Err(err(
                StatusCode::CONFLICT,
                format!("{} 已存在，未覆盖原题", q["id"]),
            ));
        }
        tx.execute(
            "INSERT INTO questions VALUES(?1,?2,?3)",
            params![q["id"].as_str(), subject, q.to_string()],
        )
        .map_err(internal)?;
    }
    import["status"] = json!("published");
    import["published_count"] = json!(rows.len());
    tx.execute(
        "UPDATE imports SET data=?1 WHERE id=?2",
        params![import.to_string(), id],
    )
    .map_err(internal)?;
    tx.commit().map_err(internal)?;
    Ok(Json(json!({"published":rows.len(),"questions":rows})))
}
async fn file(State(app): State<App>, Path(id): Path<String>) -> Result<Response, ApiError> {
    if uuid::Uuid::parse_str(&id).is_err() {
        return Err(bad("文件编号无效"));
    }
    let bytes = tokio::fs::read(app.data.join("files").join(format!("{id}.pdf")))
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "原PDF暂未导入"))?;
    Ok((
        [
            ("content-type", "application/pdf"),
            ("content-disposition", "inline"),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
struct ExportRequest {
    question_ids: Vec<String>,
    #[serde(default)]
    mode: Option<String>,
}
async fn export_packet(
    State(app): State<App>,
    Json(req): Json<ExportRequest>,
) -> Result<Json<Value>, ApiError> {
    if req.question_ids.is_empty() || req.question_ids.len() > 100 {
        return Err(bad("请选择1至100道题"));
    }
    let mode = req.mode.unwrap_or("practice".into());
    if !["practice", "study"].contains(&mode.as_str()) {
        return Err(bad("导出模式无效"));
    }
    let mut questions = Vec::new();
    let mut warnings = Vec::new();
    for qid in req.question_ids {
        let q = get_question(&app, &qid)?;
        let file = q["qp_file_id"]
            .as_str()
            .ok_or(bad("这道题未导入原PDF，暂不能导出"))?;
        if uuid::Uuid::parse_str(file).is_err() {
            return Err(bad("无效的原卷文件标识"));
        }
        let qp = app.data.join("files").join(format!("{file}.pdf"));
        if !qp.exists() {
            return Err(bad("原卷文件不可用，请先完成文件导入"));
        }
        let ms = if let Some(id) = q["ms_file_id"]
            .as_str()
            .filter(|_| q["ms_pages"].as_array().is_some_and(|a| !a.is_empty()))
        {
            if uuid::Uuid::parse_str(id).is_err() {
                return Err(bad("无效的评分标准标识"));
            }
            Some(app.data.join("files").join(format!("{id}.pdf")))
        } else {
            None
        };
        if q["status"] != "reviewed" && q["status"] != "agent_verified" {
            warnings.push(format!("{} 为旧题库迁移记录，页码/题面尚待复核", qid))
        }
        let note: String = app
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT note FROM attempts WHERE question_id=?1 ORDER BY created_at DESC LIMIT 1",
                [&qid],
                |r| r.get(0),
            )
            .optional()
            .map_err(internal)?
            .unwrap_or_default();
        if mode == "study" && note.chars().count() > 2000 {
            return Err(bad(format!(
                "{} 的笔记超过导出上限2000字，请缩短后重试",
                qid
            )));
        }
        questions.push(json!({"id":qid,"source":q["source"],"question_no":q["question_no"],"qp_path":qp,"qp_pages":q["qp_pages"],"ms_path":ms,"ms_pages":q["ms_pages"],"note":if mode=="study"{note}else{String::new()}}));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let manifest = app.data.join("exports").join(format!("{id}.json"));
    let out = app.data.join("exports").join(format!("{id}.zip"));
    tokio::fs::write(
        &manifest,
        serde_json::to_vec(&json!({"questions":questions,"mode":mode})).unwrap(),
    )
    .await
    .map_err(internal)?;
    let archive = native_packet::export(&app, &json!({"questions":questions,"mode":mode})).await?;
    tokio::fs::write(&out, archive).await.map_err(internal)?;
    app.db
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO exports VALUES(?1,?2,?3)",
            params![id, out.to_string_lossy(), now()],
        )
        .map_err(internal)?;
    warnings.push("本阶段导出完整原卷页，可能包含同页相邻题；尚未按题精确裁切".into());
    Ok(Json(
        json!({"id":id,"url":format!("/api/exports/{id}"),"warnings":warnings}),
    ))
}
async fn download_export(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    if uuid::Uuid::parse_str(&id).is_err() {
        return Err(bad("无效编号"));
    }
    let bytes = tokio::fs::read(app.data.join("exports").join(format!("{id}.zip")))
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "导出文件不存在"))?;
    Ok((
        [
            ("content-type", "application/zip"),
            (
                "content-disposition",
                "attachment; filename=study-packet.zip",
            ),
        ],
        bytes,
    )
        .into_response())
}
const LEGACY_TOOLS: [&str; 8] = [
    "get_subjects",
    "get_syllabus_tree",
    "search_questions",
    "semantic_vector_search",
    "generate_exam_paper",
    "get_syllabus_coverage",
    "get_kp_questions",
    "get_kp_trend",
];
async fn agent_call(
    State(app): State<App>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let tool = body
        .get("tool")
        .or_else(|| body.get("name"))
        .and_then(Value::as_str)
        .ok_or(bad("缺少 tool"))?;
    if !LEGACY_TOOLS.contains(&tool) {
        return Err(err(StatusCode::NOT_FOUND, "未知工具"));
    }
    let args = body
        .get("arguments")
        .or_else(|| body.get("args"))
        .cloned()
        .unwrap_or(json!({}));
    let subject = args
        .get("subject")
        .and_then(Value::as_str)
        .unwrap_or("9708");
    if !["9702", "9708"].contains(&subject) {
        return Err(bad("subject必须为9702或9708"));
    }
    if !args.is_object() {
        return Err(bad("arguments/args 必须是对象"));
    }
    let result = agent::native(&app, tool, &args)
        .await?
        .ok_or_else(|| err(StatusCode::NOT_IMPLEMENTED, "工具尚未迁入"))?;
    Ok(Json(result))
}

async fn health() -> Json<Value> {
    Json(
        json!({"status":"ok","version":"0.1.0","core":"rust","mode":"local","pdf_engine":"PdfCraft 0.20.0","python_required":false}),
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::var("STUDY_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .to_path_buf()
        });
    let data = std::env::var("STUDY_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("data"));
    std::fs::create_dir_all(data.join("files"))?;
    std::fs::create_dir_all(data.join("exports"))?;
    let db = Connection::open(data.join("study.sqlite"))?;
    init_db(&db)?;
    let app = App {
        db: Arc::new(Mutex::new(db)),
        siwc: Arc::new(siwc::Service::new(&data)),
        data,
        root: root.clone(),
    };
    let router = Router::new()
        .merge(recognition::routes())
        .merge(siwc::routes())
        .merge(luna::routes())
        .merge(definitions::routes())
        .route("/api/health", get(health))
        .route("/api/agent/tools", get(agent::catalog).post(agent_call))
        .route("/api/exports", post(export_packet))
        .route("/api/exports/{id}", get(download_export))
        .route("/api/questions", get(bank::questions))
        .route("/api/filter-options", get(bank::options))
        .route("/api/syllabus/{subject}", get(syllabus::source))
        .route("/api/basket", get(basket::get).put(basket::save))
        .route("/api/basket/export", post(basket::export))
        .route("/api/coverage", get(library_features::coverage))
        .route("/api/compositions", post(library_features::compose))
        .route("/api/compositions/{id}", patch(library_features::edit))
        .route(
            "/api/compositions/{id}/export",
            post(library_features::save_export),
        )
        .route(
            "/api/compositions/{id}/download",
            get(library_features::download),
        )
        .route("/api/questions/{id}", get(question))
        .route("/api/questions/{id}/focus", get(pdf_engine::focus))
        .route("/api/topics", get(topics))
        .route("/api/attempts", get(attempts).post(save_attempt))
        .route(
            "/api/study-memory",
            get(library_features::memory).put(library_features::save_memory),
        )
        .route("/api/imports", get(imports).post(upload))
        .route("/api/imports/{id}", patch(update_import))
        .route("/api/imports/{id}/publish", post(publish))
        .route("/api/files/{id}", get(file))
        .route("/api/files/{id}/info", get(pdf_engine::info))
        .route("/api/files/{id}/pages/{page}", get(pdf_engine::render))
        .fallback_service(
            ServeDir::new(root.join("web/dist")).append_index_html_on_directories(true),
        )
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(middleware::from_fn(guard))
        .with_state(app);
    let port = std::env::var("STUDY_PORT").unwrap_or("4317".into());
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    let actual_port = listener.local_addr()?.port();
    let url = format!("http://127.0.0.1:{actual_port}");
    if let Ok(ready) = std::env::var("STUDY_READY_FILE") {
        std::fs::write(ready, &url)?;
    }
    println!("Study core ready: {url}");
    axum::serve(listener, router).await?;
    Ok(())
}
