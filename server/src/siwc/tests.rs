//! Isolated, synthetic OAuth provider. These keys and tokens have no account access.
use super::*;
use axum::extract::Form;
use jsonwebtoken::{encode, EncodingKey, Header};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Mock {
    issuer: String,
    pem: Vec<u8>,
    jwks: Value,
    nonce: Mutex<String>,
    mode: AtomicUsize,
    refreshes: AtomicUsize,
    codes: AtomicUsize,
    last_form: Mutex<BTreeMap<String, String>>,
    last_request: Mutex<Value>,
}
struct Fixture {
    dir: tempfile::TempDir,
    service: Arc<Service>,
    provider: Arc<Mock>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
fn keypair() -> (Vec<u8>, Value) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic-test-only.pem");
    let result = std::process::Command::new("/usr/bin/openssl")
        .args(["genrsa", "-out"])
        .arg(&path)
        .arg("2048")
        .output()
        .unwrap();
    assert!(result.status.success());
    let result = std::process::Command::new("/usr/bin/openssl")
        .args(["rsa", "-in"])
        .arg(&path)
        .args(["-noout", "-modulus"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let hex = String::from_utf8(result.stdout).unwrap();
    let hex = hex.trim().strip_prefix("Modulus=").unwrap();
    let n = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect::<Vec<_>>();
    (
        std::fs::read(path).unwrap(),
        json!({"keys":[{"kty":"RSA","use":"sig","alg":"RS256","kid":"synthetic-test-key","n":URL_SAFE_NO_PAD.encode(n),"e":"AQAB"}]}),
    )
}
fn id_token(mock: &Mock, client: &str, nonce: &str, extra: Value) -> String {
    let mut claims = json!({"iss":mock.issuer,"aud":client,"exp":timestamp()+3600,"sub":"synthetic-user","email":"test@example.invalid","nonce":nonce});
    for (k, v) in extra.as_object().unwrap() {
        claims[k] = v.clone();
    }
    let mut h = Header::new(Algorithm::RS256);
    h.kid = Some("synthetic-test-key".into());
    encode(&h, &claims, &EncodingKey::from_rsa_pem(&mock.pem).unwrap()).unwrap()
}
async fn discover(State(m): State<Arc<Mock>>) -> Json<Value> {
    Json(
        json!({"issuer":m.issuer,"authorization_endpoint":format!("{}/authorize",m.issuer),"token_endpoint":format!("{}/token",m.issuer),"jwks_uri":format!("{}/jwks",m.issuer),"revocation_endpoint":format!("{}/revoke",m.issuer)}),
    )
}
async fn jwks(State(m): State<Arc<Mock>>) -> Json<Value> {
    Json(m.jwks.clone())
}
async fn token(State(m): State<Arc<Mock>>, Form(form): Form<BTreeMap<String, String>>) -> Response {
    *m.last_form.lock().unwrap() = form.clone();
    if form["grant_type"] == "refresh_token" {
        m.refreshes.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(25)).await;
        return Json(json!({"access_token":"synthetic-refreshed","refresh_token":"synthetic-rotated","token_type":"Bearer","expires_in":3600,"scope":SCOPES})).into_response();
    }
    m.codes.fetch_add(1, Ordering::SeqCst);
    if m.mode.load(Ordering::SeqCst) == 1 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_grant"})),
        )
            .into_response();
    }
    let nonce = m.nonce.lock().unwrap().clone();
    let nonce = if m.mode.load(Ordering::SeqCst) == 2 {
        "wrong-nonce"
    } else {
        &nonce
    };
    Json(json!({"access_token":"synthetic-access","refresh_token":"synthetic-refresh","id_token":id_token(&m,&form["client_id"],nonce,json!({})),"token_type":"Bearer","scope":if m.mode.load(Ordering::SeqCst)==3{"openid profile email"}else{SCOPES},"expires_in":3600})).into_response()
}
async fn models(State(m): State<Arc<Mock>>) -> Json<Value> {
    Json(
        json!({"models":if m.mode.load(Ordering::SeqCst)==4{vec![]}else{vec![json!({"slug":MODEL,"display_name":"Synthetic Luna","visibility":"list"}),json!({"slug":"hidden-model","display_name":"Hidden","visibility":"hide"})]}}),
    )
}
async fn responses(State(m): State<Arc<Mock>>, Json(body): Json<Value>) -> Response {
    *m.last_request.lock().unwrap() = body.clone();
    let event = if m.mode.load(Ordering::SeqCst) == 5 {
        json!({"type":"response.failed","response":{"error":{"code":"subscription_sharing_usage_limit_exceeded"}}})
    } else if m.mode.load(Ordering::SeqCst) == 6 {
        let run = body
            .pointer(
                "/text/format/schema/properties/proposal/anyOf/0/properties/agent_run_id/enum/0",
            )
            .unwrap();
        let source: Value =
            serde_json::from_str(body["input"][0]["content"][1]["text"].as_str().unwrap()).unwrap();
        let proposal = json!({"schema_version":"1","agent_run_id":run,"questions":[{"exam_key":"9702/12/w/2023","question_no":"31","subparts":[],"question":[{"source_id":source["source_id"],"page":source["page"],"bbox":[0,0,source["width_pdf_points"],source["height_pdf_points"]],"quote":"In the circuit shown"}],"answer":[],"figures":[],"confidence":0.5,"exception_flags":["missing_answer","independent_verification_required"],"stem_text":"SYNTHETIC PROVIDER OUTPUT; not an actual model transcription","answer_text":"","topics":[],"syllabus_refs":[]}]});
        let result = json!({"proposal":proposal,"source_roles":[{"source_id":source["source_id"],"role":"qp"}],"warnings":["Synthetic local provider only"]});
        json!({"type":"response.completed","response":{"id":"synthetic-preview","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":result.to_string()}]}]}})
    } else {
        json!({"type":"response.completed","response":{"id":"synthetic-response","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"{\"visible_number\":\"31\"}"}]}],"usage":{"input_tokens":1,"output_tokens":1}}})
    };
    (
        [("content-type", "text/event-stream")],
        format!("data: {event}\n\ndata: [DONE]\n\n"),
    )
        .into_response()
}
async fn fixture() -> Fixture {
    let (pem, key) = keypair();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let provider = Arc::new(Mock {
        issuer: issuer.clone(),
        pem,
        jwks: key,
        nonce: Mutex::new(String::new()),
        mode: AtomicUsize::new(0),
        refreshes: AtomicUsize::new(0),
        codes: AtomicUsize::new(0),
        last_form: Mutex::new(BTreeMap::new()),
        last_request: Mutex::new(Value::Null),
    });
    let router = Router::new()
        .route("/discovery", get(discover))
        .route("/jwks", get(jwks))
        .route("/token", post(token))
        .route("/models", get(models))
        .route("/responses", post(responses))
        .route("/revoke", post(|| async { StatusCode::OK }))
        .with_state(provider.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let mut service = Service::new(dir.path());
    service.client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    service.endpoints = Endpoints {
        issuer: issuer.clone(),
        discovery: format!("{issuer}/discovery"),
        authorize: format!("{issuer}/authorize"),
        token: format!("{issuer}/token"),
        api: issuer,
    };
    Fixture {
        dir,
        service: Arc::new(service),
        provider,
        server,
    }
}
async fn begin(f: &Fixture, selected: Option<String>) -> (Value, BTreeMap<String, String>) {
    let r = f
        .service
        .begin("127.0.0.1:45678", selected, false)
        .await
        .unwrap();
    let u = f
        .service
        .authorization(r["attempt"].as_str().unwrap())
        .await
        .unwrap();
    let params = reqwest::Url::parse(&u)
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect::<BTreeMap<_, _>>();
    *f.provider.nonce.lock().unwrap() = params["nonce"].clone();
    (r, params)
}
async fn login(f: &Fixture) -> String {
    let (_, p) = begin(f, None).await;
    f.service
        .callback(&format!(
            "code=synthetic-code&client_id=oaiapp_synthetic&state={}",
            p["state"]
        ))
        .await
        .unwrap();
    f.service.status().unwrap()["active_registration"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn app_owned_registration_pkce_storage_catalog_and_logout() {
    let f = fixture().await;
    assert!(!f.service.dir.exists());
    assert_eq!(f.service.status().unwrap()["authorized"], false);
    assert!(!f.service.dir.exists());
    let (_, p) = begin(&f, None).await;
    assert_eq!(p["client_id"], "dynamic_agent_client");
    assert_eq!(p["agent_name_hint"], APP_NAME);
    assert_eq!(p["redirect_uri"], "http://127.0.0.1:45678/auth/callback");
    assert_eq!(p["resource"], RESOURCE);
    assert_eq!(p["code_challenge_method"], "S256");
    assert!(f
        .service
        .callback("code=bad&client_id=oaiapp_synthetic&state=wrong")
        .await
        .is_err());
    assert_eq!(f.provider.codes.load(Ordering::SeqCst), 0);
    let callback = format!(
        "code=synthetic-code&client_id=oaiapp_synthetic&state={}",
        p["state"]
    );
    f.service.callback(&callback).await.unwrap();
    assert!(f.service.callback(&callback).await.is_err());
    assert_eq!(f.provider.codes.load(Ordering::SeqCst), 1);
    let form = f.provider.last_form.lock().unwrap().clone();
    assert_eq!(form["redirect_uri"], p["redirect_uri"]);
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes())),
        p["code_challenge"]
    );
    assert!(!form.contains_key("client_secret"));
    let status = f.service.status().unwrap();
    assert_eq!(status["authorized"], true);
    assert!(!status.to_string().contains("synthetic-access"));
    assert!(!status.to_string().contains("synthetic-refresh"));
    let id = status["active_registration"].as_str().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(f.service.dir.join("accounts.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&f.service.dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    let catalog = f
        .service
        .models(
            f.service.status().unwrap()["active_registration"]
                .as_str()
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(catalog["source"], "live_account_api");
    assert_eq!(catalog["models"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["luna_available"], true);
    let host = p["ext_agent_host_id"].clone();
    let (_, returning) = begin(&f, Some(id.into())).await;
    assert_eq!(returning["client_id"], "oaiapp_synthetic");
    assert_eq!(returning["ext_agent_host_id"], host);
    assert!(!returning.contains_key("agent_name_hint"));
    assert!(f.service.logout(id).await.unwrap());
    assert_eq!(f.service.status().unwrap()["authorized"], false);
    let record = f.service.read().unwrap();
    assert!(record.accounts[0].access_token.is_empty());
    assert_eq!(record.accounts[0].client_id, "oaiapp_synthetic");
    assert_eq!(record.host_id, host);
}
#[tokio::test]
async fn denied_missing_client_and_nonce_failure_preserve_active_account() {
    let f = fixture().await;
    let id = login(&f).await;
    let before = std::fs::read(f.service.dir.join("accounts.json")).unwrap();
    let (_, p) = begin(&f, None).await;
    assert!(f
        .service
        .callback(&format!("error=access_denied&state={}", p["state"]))
        .await
        .is_err());
    assert_eq!(f.provider.codes.load(Ordering::SeqCst), 1);
    let (_, p) = begin(&f, None).await;
    assert!(f
        .service
        .callback(&format!("code=x&state={}", p["state"]))
        .await
        .is_err());
    let (_, p) = begin(&f, Some(id)).await;
    assert!(f
        .service
        .callback(&format!(
            "code=x&client_id=oaiapp_different&state={}",
            p["state"]
        ))
        .await
        .is_err());
    let (_, p) = begin(&f, None).await;
    f.provider.mode.store(2, Ordering::SeqCst);
    assert!(f
        .service
        .callback(&format!(
            "code=x&client_id=oaiapp_other&state={}",
            p["state"]
        ))
        .await
        .is_err());
    assert_eq!(
        before,
        std::fs::read(f.service.dir.join("accounts.json")).unwrap()
    );
}
#[tokio::test]
async fn missing_plan_scope_cannot_query_or_infer_and_jwt_rejects_forgery() {
    let f = fixture().await;
    f.provider.mode.store(3, Ordering::SeqCst);
    login(&f).await;
    assert_eq!(f.service.status().unwrap()["authorized"], false);
    assert!(f
        .service
        .models(
            f.service.status().unwrap()["active_registration"]
                .as_str()
                .unwrap()
        )
        .await
        .is_err());
    let jwks: JwkSet = serde_json::from_value(f.provider.jwks.clone()).unwrap();
    for extra in [
        json!({"aud":"wrong"}),
        json!({"iss":"https://evil.invalid"}),
        json!({"exp":timestamp()-1000}),
    ] {
        let jwt = id_token(&f.provider, "oaiapp_test", "nonce", extra);
        assert!(
            validate_identity(&jwt, "oaiapp_test", "nonce", &f.provider.issuer, &jwks).is_err()
        );
    }
    let jwt = id_token(&f.provider, "oaiapp_test", "nonce", json!({}));
    assert!(validate_identity(&jwt, "oaiapp_test", "other", &f.provider.issuer, &jwks).is_err());
    let mut parts = jwt.split('.').map(str::to_owned).collect::<Vec<_>>();
    parts[1] = URL_SAFE_NO_PAD.encode(b"{\"sub\":\"forged\"}");
    assert!(validate_identity(
        &parts.join("."),
        "oaiapp_test",
        "nonce",
        &f.provider.issuer,
        &jwks
    )
    .is_err());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refresh_is_serialized_across_app_instances_and_rotates_atomically() {
    let f = fixture().await;
    login(&f).await;
    let mut store = f.service.read().unwrap();
    store.accounts[0].expires_at = timestamp() - 1;
    f.service.write(&store).unwrap();
    let mut second = Service::new(f.dir.path());
    second.client = f.service.client.clone();
    second.endpoints = f.service.endpoints.clone();
    let a = f.service.clone();
    let (one, two) = tokio::join!(a.token(), second.token());
    assert_eq!(one.unwrap().1, "synthetic-refreshed");
    assert_eq!(two.unwrap().1, "synthetic-refreshed");
    assert_eq!(f.provider.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.service.read().unwrap().accounts[0].refresh_token,
        "synthetic-rotated"
    );
}
#[tokio::test]
async fn model_is_account_scoped_and_requests_use_only_public_responses() {
    let f = fixture().await;
    login(&f).await;
    f.provider.mode.store(4, Ordering::SeqCst);
    assert!(f
        .service
        .send(
            &json!({"model":MODEL,"store":false,"stream":true}),
            f.service.status().unwrap()["active_registration"]
                .as_str()
                .unwrap()
        )
        .await
        .is_err());
    assert!(f.provider.last_request.lock().unwrap().is_null());
    f.provider.mode.store(0, Ordering::SeqCst);
    let (_, r) = f
        .service
        .send(
            &json!({"model":MODEL,"store":false,"stream":true}),
            f.service.status().unwrap()["active_registration"]
                .as_str()
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(r.text().await.unwrap().contains("response.completed"));
    assert_eq!(f.provider.last_request.lock().unwrap()["model"], MODEL);
}

#[tokio::test]
async fn synthetic_image_adapter_completes_through_mock_http_and_rejects_late_quota_failure() {
    let f = fixture().await;
    login(&f).await;
    let app = App {
        db: Arc::new(Mutex::new(Connection::open_in_memory().unwrap())),
        data: f.dir.path().into(),
        root: f.dir.path().into(),
        siwc: f.service.clone(),
    };
    let result = crate::luna::smoke(
        State(app.clone()),
        Json(ModelRequest {
            registration_id: f.service.status().unwrap()["active_registration"]
                .as_str()
                .unwrap()
                .into(),
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(result["passed"], true);
    assert_eq!(result["exam_accuracy_measured"], false);
    let request = f.provider.last_request.lock().unwrap().clone();
    assert_eq!(request["store"], false);
    assert_eq!(request["stream"], true);
    assert_eq!(request["model"], MODEL);
    assert_eq!(request["text"]["format"]["strict"], true);
    assert!(request["input"][0]["content"][1]["image_url"]
        .as_str()
        .unwrap()
        .starts_with("data:image/png;base64,"));
    assert!(request.get("max_output_tokens").is_none());
    let success = f.service.status().unwrap()["last_success"].clone();
    assert_eq!(success["kind"], "synthetic_image_test");
    f.provider.mode.store(5, Ordering::SeqCst);
    assert!(crate::luna::smoke(
        State(app),
        Json(ModelRequest {
            registration_id: f.service.status().unwrap()["active_registration"]
                .as_str()
                .unwrap()
                .into()
        })
    )
    .await
    .is_err());
    assert_eq!(f.service.status().unwrap()["last_success"], success);
}

#[tokio::test]
async fn real_pdf_preview_and_proposal_submission_keep_the_independent_review_gate() {
    let f = fixture().await;
    login(&f).await;
    f.provider.mode.store(6, Ordering::SeqCst);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let source = "d81f4af8-192f-5d5a-887a-918d79f09ffd";
    let path = root.join("data/files").join(format!("{source}.pdf"));
    let original = std::fs::read(&path).unwrap();
    let sha = format!("{:x}", Sha256::digest(&original));
    std::fs::create_dir_all(f.dir.path().join("files")).unwrap();
    std::fs::write(
        f.dir.path().join("files").join(format!("{source}.pdf")),
        &original,
    )
    .unwrap();
    let db = Connection::open_in_memory().unwrap();
    init_db(&db).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let job = json!({"id":id,"status":"awaiting_agent","sources":[{"id":source,"sha256":sha,"pages":20,"role_hint":"qp","filename":"9702_w23_12.pdf"}],"published_count":0});
    db.execute(
        "INSERT INTO recognition_jobs VALUES(?1,?2,?3)",
        params![id, "synthetic-fingerprint", job.to_string()],
    )
    .unwrap();
    let app = App {
        db: Arc::new(Mutex::new(db)),
        data: f.dir.path().into(),
        root: root.into(),
        siwc: f.service.clone(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = crate::luna::routes()
        .merge(crate::recognition::routes())
        .with_state(app.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let endpoint = format!("{base}/api/recognition/jobs/{id}/luna/preview");
    let payload = json!({"registration_id":f.service.status().unwrap()["active_registration"],"pages":[{"source_id":source,"pages":[14]}],"question_filter":"31","consent_to_send_pages":false});
    let mut wrong_account = payload.clone();
    wrong_account["registration_id"] = json!("stale-account");
    wrong_account["consent_to_send_pages"] = json!(true);
    assert_eq!(
        client
            .post(&endpoint)
            .json(&wrong_account)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert!(f.provider.last_request.lock().unwrap().is_null());
    assert_eq!(
        client
            .post(&endpoint)
            .json(&payload)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert!(f.provider.last_request.lock().unwrap().is_null());
    let mut payload = payload;
    payload["consent_to_send_pages"] = json!(true);
    let response = client.post(&endpoint).json(&payload).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let preview: Value = response.json().await.unwrap();
    assert_eq!(preview["status"], "unverified_preview");
    assert_eq!(preview["published_count"], 0);
    assert_eq!(
        recognition::get_job(&app, &id).unwrap()["status"],
        "awaiting_agent"
    );
    let run = preview["run_id"].as_str().unwrap();
    let response = client
        .post(format!(
            "{base}/api/recognition/jobs/{id}/luna/runs/{run}/submit"
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        recognition::get_job(&app, &id).unwrap()["status"],
        "awaiting_verification"
    );
    assert_eq!(
        app.db
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM questions", [], |r| r
                .get::<_, usize>(0))
            .unwrap(),
        0
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        std::fs::read(f.dir.path().join("files").join(format!("{source}.pdf"))).unwrap(),
        original
    );
    server.abort();
}

#[tokio::test]
async fn invalid_code_retains_issued_registration_and_expired_attempts_fail_closed() {
    let f = fixture().await;
    let (_, p) = begin(&f, None).await;
    f.provider.mode.store(1, Ordering::SeqCst);
    assert!(f
        .service
        .callback(&format!(
            "code=used-code&client_id=oaiapp_retry&state={}",
            p["state"]
        ))
        .await
        .is_err());
    assert_eq!(f.service.status().unwrap()["authorized"], false);
    let (_, p) = begin(&f, None).await;
    assert_eq!(p["client_id"], "oaiapp_retry");
    assert!(!p.contains_key("agent_name_hint"));
    f.provider.mode.store(0, Ordering::SeqCst);
    f.service
        .callback(&format!("code=fresh-code&state={}", p["state"]))
        .await
        .unwrap();
    assert_eq!(f.service.status().unwrap()["authorized"], true);
    let id = f.service.status().unwrap()["active_registration"]
        .as_str()
        .unwrap()
        .to_owned();
    let (result, p) = begin(&f, Some(id)).await;
    f.service
        .pending
        .lock()
        .await
        .get_mut(result["attempt"].as_str().unwrap())
        .unwrap()
        .deadline = timestamp() - 1;
    let codes = f.provider.codes.load(Ordering::SeqCst);
    assert!(f
        .service
        .callback(&format!("code=x&state={}", p["state"]))
        .await
        .is_err());
    assert_eq!(f.provider.codes.load(Ordering::SeqCst), codes);
    let guard = f.service.inference_guard().await.unwrap();
    assert!(f.service.inference_guard().await.is_err());
    drop(guard);
    assert!(f.service.inference_guard().await.is_ok());
}

#[tokio::test]
async fn stale_account_selection_is_rejected_before_any_model_request() {
    let f = fixture().await;
    login(&f).await;
    assert_eq!(
        f.service
            .send(&json!({"model":MODEL}), "stale-account")
            .await
            .unwrap_err()
            .0,
        StatusCode::CONFLICT
    );
    assert!(f.provider.last_request.lock().unwrap().is_null());
}
