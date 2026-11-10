//! Application-owned SiWC. Never reads Codex credentials or accepts arbitrary providers.
use super::*;
use axum::{
    extract::RawQuery,
    response::{Html, Redirect},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use fs2::FileExt;
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::Write,
    time::Duration,
};
use tokio::sync::Mutex as AsyncMutex;

const ISSUER: &str = "https://auth.openai.com";
const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const APP_NAME: &str = "真题学习";
pub const MODEL: &str = "gpt-6-luna";
#[derive(Clone)]
struct Endpoints {
    issuer: String,
    discovery: String,
    authorize: String,
    token: String,
    api: String,
}
impl Default for Endpoints {
    fn default() -> Self {
        Self {
            issuer: ISSUER.into(),
            discovery: format!("{ISSUER}/.well-known/openid-configuration"),
            authorize: format!("{ISSUER}/api/accounts/authorize"),
            token: format!("{ISSUER}/api/accounts/oauth/token"),
            api: RESOURCE.into(),
        }
    }
}
#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    host_id: String,
    #[serde(default)]
    accounts: Vec<Account>,
    active: Option<String>,
    #[serde(default)]
    retry_client: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Account {
    id: String,
    client_id: String,
    issuer: String,
    subject: String,
    email: String,
    access_token: String,
    refresh_token: String,
    id_token: String,
    scopes: Vec<String>,
    expires_at: i64,
    #[serde(default)]
    last_success: Option<Value>,
    #[serde(default)]
    welcome_seen: bool,
}
impl Account {
    fn enabled(&self) -> bool {
        !self.access_token.is_empty()
            && self.scopes.iter().any(|s| s == "chatgpt.tokens.use.direct")
            && self.scopes.iter().any(|s| s == "resource.invoke")
    }
}
struct Pending {
    state: String,
    nonce: String,
    verifier: String,
    redirect: String,
    authorization: String,
    local_start: String,
    selected: Option<Account>,
    expected_client: Option<String>,
    deadline: i64,
}
pub struct Service {
    dir: PathBuf,
    client: reqwest::Client,
    endpoints: Endpoints,
    pending: AsyncMutex<BTreeMap<String, Pending>>,
    pub inference: AsyncMutex<()>,
    flow: Mutex<Value>,
}
fn auth_error(s: &str) -> ApiError {
    err(StatusCode::BAD_GATEWAY, s)
}
fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}
fn scopes(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_owned).collect()
}
fn safe_code(v: &Value) -> &'static str {
    match v
        .pointer("/error/code")
        .or_else(|| v.get("error"))
        .and_then(Value::as_str)
        .unwrap_or("")
    {
        "subscription_sharing_usage_limit_exceeded" => "subscription_sharing_usage_limit_exceeded",
        "subscription_sharing_usage_unavailable" => "subscription_sharing_usage_unavailable",
        "invalid_grant" => "invalid_grant",
        "model_not_found" => "model_not_found",
        "insufficient_permissions" => "insufficient_permissions",
        _ => "upstream_error",
    }
}
pub fn upstream_error(status: u16, v: &Value) -> ApiError {
    let code = safe_code(v);
    let message = match code {
        "subscription_sharing_usage_limit_exceeded" => {
            "ChatGPT 用量已达限额，请在 ChatGPT 设置中查看用量。"
        }
        "subscription_sharing_usage_unavailable" => {
            "当前账户或工作区暂不允许本应用使用 ChatGPT 订阅。"
        }
        "invalid_grant" => "登录已失效，需要重新登录。",
        "model_not_found" => "此账户暂不能使用所选模型。",
        "insufficient_permissions" => "当前授权缺少所需权限。",
        _ => "OpenAI 请求未成功；未自动重试，也未发布识别结果。",
    };
    err(
        if status == 429 {
            StatusCode::TOO_MANY_REQUESTS
        } else {
            StatusCode::BAD_GATEWAY
        },
        format!("{message} ({code}; HTTP {status})"),
    )
}
impl Service {
    pub fn new(data: &std::path::Path) -> Self {
        Self {
            dir: data.join("siwc"),
            client: reqwest::Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(150))
                .build()
                .expect("TLS HTTP client"),
            endpoints: Endpoints::default(),
            pending: AsyncMutex::new(BTreeMap::new()),
            inference: AsyncMutex::new(()),
            flow: Mutex::new(json!({"phase":"idle"})),
        }
    }
    fn read(&self) -> Result<Store, ApiError> {
        let path = self.dir.join("accounts.json");
        if !path.exists() {
            return Ok(Store::default());
        }
        let meta =
            std::fs::symlink_metadata(&path).map_err(|_| auth_error("无法读取本应用登录记录"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(auth_error(
                    "本应用登录记录权限过宽，请修复为仅本人可读写后重试",
                ));
            }
        }
        if meta.file_type().is_symlink() || meta.len() > 4 * 1024 * 1024 {
            return Err(auth_error("本应用登录记录无效"));
        }
        serde_json::from_slice(&std::fs::read(path).map_err(|_| auth_error("无法读取登录记录"))?)
            .map_err(|_| auth_error("本应用登录记录格式损坏，原记录未覆盖"))
    }
    fn write(&self, store: &Store) -> Result<(), ApiError> {
        let mut file = tempfile::NamedTempFile::new_in(&self.dir)
            .map_err(|_| auth_error("无法安全保存登录"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|_| auth_error("无法保护登录记录权限"))?;
        }
        file.write_all(&serde_json::to_vec(store).map_err(|_| auth_error("无法编码登录记录"))?)
            .map_err(|_| auth_error("无法保存登录记录"))?;
        file.as_file()
            .sync_all()
            .map_err(|_| auth_error("无法同步登录记录"))?;
        file.persist(self.dir.join("accounts.json"))
            .map_err(|_| auth_error("无法原子保存登录记录"))?;
        Ok(())
    }
    async fn lock(&self) -> Result<File, ApiError> {
        let dir = self.dir.clone();
        tokio::task::spawn_blocking(move || {
            if dir.exists() {
                let meta =
                    std::fs::symlink_metadata(&dir).map_err(|_| auth_error("无法读取授权目录"))?;
                if meta.file_type().is_symlink() {
                    return Err(auth_error("授权目录不能是符号链接"));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if meta.permissions().mode() & 0o077 != 0 {
                        return Err(auth_error("授权目录须仅本人可访问"));
                    }
                }
            } else {
                let mut builder = std::fs::DirBuilder::new();
                builder.recursive(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder
                    .create(&dir)
                    .map_err(|_| auth_error("无法建立本应用授权目录"))?;
            }
            let mut options = OpenOptions::new();
            options.create(true).read(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let f = options
                .open(dir.join("session.lock"))
                .map_err(|_| auth_error("无法锁定登录记录"))?;
            f.lock_exclusive()
                .map_err(|_| auth_error("无法锁定登录记录"))?;
            Ok(f)
        })
        .await
        .map_err(|_| auth_error("授权存储任务失败"))?
    }
    pub fn status(&self) -> Result<Value, ApiError> {
        {
            let mut flow = self.flow.lock().unwrap();
            if flow["phase"] == "awaiting_user"
                && flow["expires_at"]
                    .as_i64()
                    .is_some_and(|t| t <= timestamp())
            {
                *flow = json!({"phase":"expired","message":"本次登录已过期；请重新开始，原账户保持不变。"});
            }
        }
        let store = self.read()?;
        let active = store
            .active
            .as_ref()
            .and_then(|id| store.accounts.iter().find(|a| &a.id == id));
        Ok(
            json!({"authorized":active.is_some_and(Account::enabled),"active_registration":store.active,"accounts":store.accounts.iter().map(|a|json!({"id":a.id,"label":format!("{} · {}",if a.email.is_empty(){"ChatGPT"}else{&a.email},&a.id[..8]),"signed_in":!a.access_token.is_empty(),"plan_enabled":a.enabled()})).collect::<Vec<_>>(),"last_success":active.and_then(|a|a.last_success.clone()),"welcome_needed":active.is_some_and(|a|a.enabled()&&!a.welcome_seen),"flow":self.flow.lock().unwrap().clone(),"model":MODEL,"credential_storage":"本应用独立目录，仅本人可访问；不读取 Codex 登录","usage_url":"https://chatgpt.com/settings/usage"}),
        )
    }
    async fn discovery(&self) -> Result<Value, ApiError> {
        let v = self.public_json(&self.endpoints.discovery).await?;
        if v["issuer"] != self.endpoints.issuer
            || v["authorization_endpoint"] != self.endpoints.authorize
            || v["token_endpoint"] != self.endpoints.token
        {
            return Err(auth_error("OpenAI 身份元数据不匹配"));
        }
        for k in ["jwks_uri", "revocation_endpoint"] {
            let url = v[k]
                .as_str()
                .ok_or_else(|| auth_error("身份元数据缺少端点"))?;
            self.check_auth_url(url)?;
        }
        Ok(v)
    }
    fn check_auth_url(&self, url: &str) -> Result<(), ApiError> {
        let base = reqwest::Url::parse(&self.endpoints.issuer).unwrap();
        let u = reqwest::Url::parse(url).map_err(|_| auth_error("身份端点无效"))?;
        if u.origin() != base.origin() || !u.username().is_empty() || u.password().is_some() {
            return Err(auth_error("身份端点不属于 OpenAI"));
        }
        Ok(())
    }
    async fn public_json(&self, url: &str) -> Result<Value, ApiError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| auth_error("无法连接 OpenAI 身份服务，请检查网络后重试"))?;
        self.json(response).await
    }
    async fn json(&self, mut response: reqwest::Response) -> Result<Value, ApiError> {
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| auth_error("OpenAI 响应中断"))?
        {
            if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                return Err(auth_error("OpenAI 响应过大"));
            }
            bytes.extend_from_slice(&chunk)
        }
        let value = serde_json::from_slice(&bytes)
            .map_err(|_| auth_error("OpenAI 返回了无法解析的响应"))?;
        if !(200..300).contains(&status) {
            return Err(upstream_error(status, &value));
        }
        Ok(value)
    }
    pub async fn begin(
        &self,
        host: &str,
        selected: Option<String>,
        fresh: bool,
    ) -> Result<Value, ApiError> {
        let origin =
            reqwest::Url::parse(&format!("http://{host}")).map_err(|_| bad("本机地址无效"))?;
        if origin.host_str() != Some("127.0.0.1") || origin.port().is_none() || origin.path() != "/"
        {
            return Err(bad("请使用 127.0.0.1 本机地址开始登录"));
        }
        let _lock = self.lock().await?;
        let mut store = self.read()?;
        if fresh && store.retry_client.take().is_some() {
            self.write(&store)?;
        }
        let account = match selected {
            Some(id) => Some(
                store
                    .accounts
                    .iter()
                    .find(|a| a.id == id)
                    .cloned()
                    .ok_or_else(|| bad("账户注册不存在"))?,
            ),
            None => None,
        };
        if store.host_id.is_empty() {
            store.host_id = format!("urn:uuid:{}", uuid::Uuid::new_v4());
            self.write(&store)?;
        }
        let state = random();
        let nonce = random();
        let verifier = random();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let redirect = format!("{}/auth/callback", origin.origin().ascii_serialization());
        let client_id = account
            .as_ref()
            .map(|a| a.client_id.as_str())
            .or(store.retry_client.as_deref())
            .unwrap_or("dynamic_agent_client");
        let expected_client = (client_id != "dynamic_agent_client").then(|| client_id.to_owned());
        let mut url = reqwest::Url::parse(&self.endpoints.authorize).unwrap();
        {
            let mut qp = url.query_pairs_mut();
            qp.extend_pairs([
                ("client_id", client_id),
                ("ext_agent_host_id", store.host_id.as_str()),
                ("response_type", "code"),
                ("redirect_uri", &redirect),
                ("scope", SCOPES),
                ("resource", RESOURCE),
                ("state", &state),
                ("nonce", &nonce),
                ("code_challenge_method", "S256"),
                ("code_challenge", &challenge),
            ]);
            if let Some(a) = &account {
                if !a.id_token.is_empty() {
                    qp.append_pair("id_token_hint", &a.id_token);
                }
                if !a.email.is_empty() {
                    qp.append_pair("login_hint", &a.email);
                }
            } else if client_id == "dynamic_agent_client" {
                qp.append_pair("agent_name_hint", APP_NAME);
            }
        }
        let ticket = random();
        let local_start = format!(
            "{}/auth/openai?attempt={ticket}",
            origin.origin().ascii_serialization()
        );
        let mut pending = self.pending.lock().await;
        pending.retain(|_, p| p.deadline > timestamp());
        if pending.len() >= 4 {
            return Err(bad("已有登录正在进行，请先完成或取消"));
        }
        pending.insert(
            ticket.clone(),
            Pending {
                state,
                nonce,
                verifier,
                redirect,
                authorization: url.to_string(),
                local_start: local_start.clone(),
                selected: account,
                expected_client,
                deadline: timestamp() + 600,
            },
        );
        *self.flow.lock().unwrap() = json!({"phase":"awaiting_user","expires_at":timestamp()+600});
        Ok(json!({"attempt":ticket,"browser_url":local_start,"expires_in":600}))
    }
    async fn authorization(&self, ticket: &str) -> Result<String, ApiError> {
        let pending = self.pending.lock().await;
        let p = pending
            .get(ticket)
            .filter(|p| p.deadline > timestamp())
            .ok_or_else(|| bad("登录已过期，请重新开始"))?;
        Ok(p.authorization.clone())
    }
    async fn callback(&self, query: &str) -> Result<(), ApiError> {
        if query.len() > 20000 {
            return Err(bad("登录回调过长"));
        }
        let mut q = BTreeMap::new();
        for (k, v) in reqwest::Url::parse(&format!("http://127.0.0.1/?{query}"))
            .map_err(|_| bad("回调无效"))?
            .query_pairs()
        {
            if q.insert(k.to_string(), v.to_string()).is_some() {
                return Err(bad("重复授权参数"));
            }
        }
        let state = q.get("state").ok_or_else(|| bad("缺少登录 state"))?;
        let p = {
            let mut pending = self.pending.lock().await;
            let ticket = pending
                .iter()
                .find(|(_, p)| p.state == *state && p.deadline > timestamp())
                .map(|(k, _)| k.clone())
                .ok_or_else(|| bad("登录状态无效或已使用"))?;
            pending.remove(&ticket).unwrap()
        };
        if q.contains_key("error") {
            *self.flow.lock().unwrap() =
                json!({"phase":"denied","message":"用户未完成授权；当前账户保持不变。"});
            return Err(bad("授权未完成，未交换或保存凭据"));
        }
        let issued = q
            .get("client_id")
            .cloned()
            .or_else(|| p.expected_client.clone())
            .filter(|s| {
                !s.is_empty()
                    && s != "dynamic_agent_client"
                    && s.len() < 256
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            })
            .ok_or_else(|| bad("注册未完成：缺少颁发的 client_id"))?;
        if p.expected_client.as_ref().is_some_and(|id| *id != issued) {
            return Err(bad("回调账户注册与选择不符，原登录未替换"));
        }
        let code = q
            .get("code")
            .filter(|c| !c.is_empty() && c.len() < 10000)
            .ok_or_else(|| bad("缺少授权码"))?;
        let _lock = self.lock().await?;
        let discovery = self.discovery().await?;
        let response = self
            .client
            .post(&self.endpoints.token)
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", &issued),
                ("code", code),
                ("code_verifier", &p.verifier),
                ("redirect_uri", &p.redirect),
                ("resource", RESOURCE),
            ])
            .send()
            .await
            .map_err(|_| auth_error("登录交换未完成，请重新登录；原登录未替换"))?;
        let tokens = match self.json(response).await {
            Ok(v) => v,
            Err(e) => {
                if p.selected.is_none()
                    && e.1 .0["error"]
                        .as_str()
                        .is_some_and(|s| s.contains("(invalid_grant;"))
                {
                    let mut saved = self.read()?;
                    saved.retry_client = Some(issued);
                    self.write(&saved)?;
                }
                return Err(e);
            }
        };
        let raw = tokens["id_token"]
            .as_str()
            .ok_or_else(|| auth_error("登录响应缺少身份令牌"))?;
        let jwks: JwkSet = serde_json::from_value(
            self.public_json(discovery["jwks_uri"].as_str().unwrap())
                .await?,
        )
        .map_err(|_| auth_error("OpenAI 签名公钥格式无效"))?;
        let identity = validate_identity(raw, &issued, &p.nonce, &self.endpoints.issuer, &jwks)?;
        if p.selected
            .as_ref()
            .is_some_and(|a| a.subject != identity.sub || a.issuer != self.endpoints.issuer)
        {
            return Err(bad("登录身份与原账户不符；未替换原凭据"));
        }
        let grant = tokens["scope"]
            .as_str()
            .ok_or_else(|| auth_error("登录响应缺少实际授权范围"))?;
        if tokens["token_type"]
            .as_str()
            .is_none_or(|s| !s.eq_ignore_ascii_case("Bearer"))
        {
            return Err(auth_error("登录响应类型无效"));
        }
        let access = tokens["access_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| auth_error("登录响应缺少访问令牌"))?;
        let ttl = tokens["expires_in"]
            .as_i64()
            .filter(|n| *n > 0 && *n < 31_536_000)
            .ok_or_else(|| auth_error("登录有效期无效"))?;
        let mut store = self.read()?;
        if store.retry_client.as_deref() == Some(&issued) {
            store.retry_client = None;
        }
        let id = format!(
            "{:x}",
            Sha256::digest(format!(
                "{}\0{}\0{}",
                self.endpoints.issuer, issued, identity.sub
            ))
        );
        let previous = store.accounts.iter().find(|a| a.id == id);
        let welcome_seen = previous.is_some_and(|a| a.welcome_seen);
        let account = Account {
            id: id.clone(),
            client_id: issued,
            issuer: self.endpoints.issuer.clone(),
            subject: identity.sub,
            email: identity.email.unwrap_or_default(),
            access_token: access.into(),
            refresh_token: tokens["refresh_token"].as_str().unwrap_or("").into(),
            id_token: raw.into(),
            scopes: scopes(grant),
            expires_at: timestamp() + ttl,
            last_success: None,
            welcome_seen,
        };
        let enabled = account.enabled();
        store.accounts.retain(|a| a.id != id);
        store.accounts.push(account);
        store.active = Some(id);
        self.write(&store)?;
        *self.flow.lock().unwrap() = json!({"phase":"signed_in","plan_enabled":enabled});
        Ok(())
    }
    pub async fn token(&self) -> Result<(String, String), ApiError> {
        let _lock = self.lock().await?;
        let mut store = self.read()?;
        let selected = store
            .active
            .clone()
            .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "请先用本应用登录 ChatGPT"))?;
        let a = store
            .accounts
            .iter_mut()
            .find(|a| a.id == selected)
            .ok_or_else(|| bad("所选账户不存在"))?;
        if !a.enabled() {
            return Err(err(
                StatusCode::UNAUTHORIZED,
                "此登录未启用本应用的 ChatGPT 订阅使用权限",
            ));
        }
        if a.expires_at <= timestamp() + 60 {
            if a.refresh_token.is_empty() {
                return Err(err(StatusCode::UNAUTHORIZED, "登录已过期，请重新登录"));
            }
            let response = self
                .client
                .post(&self.endpoints.token)
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", a.client_id.as_str()),
                    ("refresh_token", a.refresh_token.as_str()),
                    ("resource", RESOURCE),
                ])
                .send()
                .await
                .map_err(|_| auth_error("登录刷新网络中断，未自动重试"))?;
            let t = self.json(response).await?;
            let access = t["access_token"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| auth_error("刷新响应缺少访问令牌"))?;
            let ttl = t["expires_in"]
                .as_i64()
                .filter(|n| *n > 0 && *n < 31_536_000)
                .ok_or_else(|| auth_error("刷新有效期无效"))?;
            if t["token_type"]
                .as_str()
                .is_none_or(|s| !s.eq_ignore_ascii_case("Bearer"))
            {
                return Err(auth_error("刷新令牌类型无效"));
            }
            a.access_token = access.into();
            a.expires_at = timestamp() + ttl;
            if let Some(r) = t["refresh_token"].as_str() {
                a.refresh_token = r.into()
            }
            if let Some(s) = t["scope"].as_str() {
                a.scopes = scopes(s)
            }
            if !a.enabled() {
                a.last_success = None;
                self.write(&store)?;
                return Err(err(
                    StatusCode::UNAUTHORIZED,
                    "刷新后订阅使用权限不可用，请重新授权",
                ));
            }
            self.write(&store)?;
        }
        let a = store.accounts.iter().find(|a| a.id == selected).unwrap();
        Ok((a.id.clone(), a.access_token.clone()))
    }
    pub fn ensure_account(&self, expected: &str) -> Result<(), ApiError> {
        let status = self.status()?;
        if !status["authorized"].as_bool().unwrap_or(false) {
            return Err(err(
                StatusCode::UNAUTHORIZED,
                "请先完成本应用的 ChatGPT 订阅授权",
            ));
        }
        if status["active_registration"].as_str() != Some(expected) {
            return Err(err(
                StatusCode::CONFLICT,
                "当前账户已切换，请刷新并重新确认所用账户",
            ));
        }
        Ok(())
    }
    pub async fn models(&self, expected: &str) -> Result<Value, ApiError> {
        self.ensure_account(expected)?;
        let (account, token) = self.token().await?;
        if account != expected {
            return Err(err(StatusCode::CONFLICT, "账户已切换，请重新确认"));
        }
        let r = self
            .client
            .get(format!("{}/models", self.endpoints.api))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| auth_error("无法查询当前账户的模型列表"))?;
        let raw = self.json(r).await?;
        let list = raw["models"]
            .as_array()
            .ok_or_else(|| auth_error("当前账户模型目录格式不受支持"))?;
        let models = list
            .iter()
            .filter(|m| m["visibility"] == "list")
            .filter_map(|m| {
                Some(json!({"slug":m["slug"].as_str()?,"display_name":m["display_name"].as_str()?}))
            })
            .collect::<Vec<_>>();
        Ok(
            json!({"registration_id":account,"models":models,"source":"live_account_api","luna_available":models.iter().any(|m|m["slug"]==MODEL)}),
        )
    }
    pub async fn inference_guard(&self) -> Result<File, ApiError> {
        if !self.status()?["authorized"].as_bool().unwrap_or(false) {
            return Err(err(
                StatusCode::UNAUTHORIZED,
                "请先用本应用登录并授权 ChatGPT 订阅",
            ));
        }
        let _storage = self.lock().await?;
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let f = options
            .open(self.dir.join("model-request.lock"))
            .map_err(|_| auth_error("无法建立模型请求锁"))?;
        f.try_lock_exclusive().map_err(|_| {
            err(
                StatusCode::CONFLICT,
                "本应用另一窗口已有模型请求；请等待完成",
            )
        })?;
        Ok(f)
    }
    pub async fn send(
        &self,
        body: &Value,
        expected: &str,
    ) -> Result<(String, reqwest::Response), ApiError> {
        let models = self.models(expected).await?;
        if !models["luna_available"].as_bool().unwrap_or(false) {
            return Err(bad("当前账户模型列表没有 gpt-6-luna；不会自动换用其他模型"));
        }
        let (account, token) = self.token().await?;
        if models["registration_id"] != account {
            return Err(err(StatusCode::CONFLICT, "账户已切换，请重新运行"));
        }
        let r = self
            .client
            .post(format!("{}/responses", self.endpoints.api))
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .map_err(|_| auth_error("模型请求网络中断；未自动重试，未发布题目"))?;
        if !r.status().is_success() {
            let status = r.status().as_u16();
            let v = self.json(r).await;
            return match v {
                Err(e) => Err(e),
                Ok(v) => Err(upstream_error(status, &v)),
            };
        }
        Ok((account, r))
    }
    pub async fn success(&self, id: &str, kind: &str) -> Result<(), ApiError> {
        let _lock = self.lock().await?;
        let mut store = self.read()?;
        if store.active.as_deref() != Some(id) {
            return Err(err(StatusCode::CONFLICT, "账户已切换；结果未标记到新账户"));
        }
        let a = store.accounts.iter_mut().find(|a| a.id == id).unwrap();
        a.last_success = Some(json!({"model":MODEL,"completed_at":now(),"kind":kind}));
        self.write(&store)
    }
    async fn select(&self, id: &str) -> Result<(), ApiError> {
        let _lock = self.lock().await?;
        let mut store = self.read()?;
        if !store.accounts.iter().any(|a| a.id == id) {
            return Err(bad("账户不存在"));
        }
        store.active = Some(id.into());
        self.write(&store)
    }
    async fn welcome(&self) -> Result<(), ApiError> {
        let _lock = self.lock().await?;
        let mut store = self.read()?;
        let id = store.active.clone().ok_or_else(|| bad("尚未登录"))?;
        store
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .unwrap()
            .welcome_seen = true;
        self.write(&store)
    }
    async fn logout(&self, id: &str) -> Result<bool, ApiError> {
        let _lock = self.lock().await?;
        let mut store = self.read()?;
        let a = store
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| bad("账户不存在"))?;
        let mut revoked = a.refresh_token.is_empty() && a.access_token.is_empty();
        if !a.refresh_token.is_empty() {
            if let Ok(d) = self.discovery().await {
                if let Ok(r) = self
                    .client
                    .post(d["revocation_endpoint"].as_str().unwrap())
                    .form(&[
                        ("token", a.refresh_token.as_str()),
                        ("token_type_hint", "refresh_token"),
                        ("client_id", a.client_id.as_str()),
                    ])
                    .send()
                    .await
                {
                    revoked = r.status() == reqwest::StatusCode::OK;
                }
            }
        }
        a.access_token.clear();
        a.refresh_token.clear();
        a.id_token.clear();
        a.scopes.clear();
        a.last_success = None;
        if store.active.as_deref() == Some(id) {
            store.active = None
        }
        self.write(&store)?;
        Ok(revoked)
    }
}
#[derive(Clone, Deserialize)]
struct Identity {
    sub: String,
    email: Option<String>,
    nonce: String,
}
fn validate_identity(
    raw: &str,
    client: &str,
    nonce: &str,
    issuer: &str,
    jwks: &JwkSet,
) -> Result<Identity, ApiError> {
    let header = decode_header(raw).map_err(|_| auth_error("身份令牌格式无效"))?;
    if header.alg != Algorithm::RS256 {
        return Err(auth_error("身份签名算法不受支持"));
    }
    let key = jwks
        .find(
            header
                .kid
                .as_deref()
                .ok_or_else(|| auth_error("身份令牌缺少签名标识"))?,
        )
        .ok_or_else(|| auth_error("身份签名密钥未找到"))?;
    let key = DecodingKey::from_jwk(key).map_err(|_| auth_error("身份公钥无效"))?;
    let mut v = Validation::new(Algorithm::RS256);
    v.set_audience(&[client]);
    v.set_issuer(&[issuer]);
    v.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    v.leeway = 15;
    let identity = decode::<Identity>(raw, &key, &v)
        .map_err(|_| auth_error("身份签名、签发方、账户或有效期校验失败"))?
        .claims;
    if identity.nonce != nonce || identity.sub.is_empty() {
        return Err(auth_error("身份 nonce 或主体校验失败"));
    }
    Ok(identity)
}
#[derive(Deserialize)]
struct Begin {
    registration_id: Option<String>,
    consent_to_plan_use: bool,
    #[serde(default)]
    fresh_registration: bool,
}
#[derive(Deserialize)]
struct AccountRequest {
    registration_id: String,
}
#[derive(Deserialize)]
pub(crate) struct ModelRequest {
    pub registration_id: String,
}
#[derive(Deserialize)]
struct Ticket {
    attempt: String,
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/siwc/status", get(status))
        .route("/api/siwc/login/start", post(start))
        .route("/api/siwc/login/open", post(open))
        .route("/api/siwc/login/cancel", post(cancel))
        .route("/api/siwc/accounts/select", post(select))
        .route("/api/siwc/logout", post(logout))
        .route("/api/siwc/welcome", post(welcome))
        .route("/api/siwc/models", post(models))
        .route("/auth/openai", get(authorize))
        .route("/auth/callback", get(callback))
}
async fn status(State(a): State<App>) -> Result<Json<Value>, ApiError> {
    Ok(Json(a.siwc.status()?))
}
async fn start(
    State(a): State<App>,
    headers: HeaderMap,
    Json(r): Json<Begin>,
) -> Result<Json<Value>, ApiError> {
    if !r.consent_to_plan_use {
        return Err(bad("请先确认本应用的登录保存及订阅使用范围"));
    }
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| bad("缺少本机地址"))?;
    Ok(Json(
        a.siwc
            .begin(host, r.registration_id, r.fresh_registration)
            .await?,
    ))
}
async fn authorize(State(a): State<App>, Query(q): Query<Ticket>) -> Result<Redirect, ApiError> {
    Ok(Redirect::to(&a.siwc.authorization(&q.attempt).await?))
}
async fn open(State(a): State<App>, Json(q): Json<Ticket>) -> Result<Json<Value>, ApiError> {
    let url = {
        let pending = a.siwc.pending.lock().await;
        pending
            .get(&q.attempt)
            .filter(|p| p.deadline > timestamp())
            .ok_or_else(|| bad("登录已过期"))?
            .local_start
            .clone()
    };
    #[cfg(target_os = "macos")]
    {
        tokio::process::Command::new("/usr/bin/open")
            .arg(url)
            .status()
            .await
            .map_err(|_| auth_error("无法打开系统浏览器，请使用登录链接"))?
            .success()
            .then_some(())
            .ok_or_else(|| auth_error("系统浏览器未打开"))?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = url;
        return Err(bad("请使用界面提供的官方登录链接"));
    }
    Ok(Json(json!({"opened":true})))
}
async fn cancel(State(a): State<App>, Json(q): Json<Ticket>) -> Json<Value> {
    a.siwc.pending.lock().await.remove(&q.attempt);
    *a.siwc.flow.lock().unwrap() = json!({"phase":"cancelled"});
    Json(json!({"cancelled":true}))
}
async fn callback(State(a): State<App>, RawQuery(q): RawQuery) -> Response {
    let result = a.siwc.callback(q.as_deref().unwrap_or("")).await;
    let (ok, msg) = match result {
        Ok(()) => (
            true,
            "登录已完成。请返回真题学习，查询模型权限并运行连接测试。",
        ),
        Err(_) => {
            let mut flow = a.siwc.flow.lock().unwrap();
            if flow["phase"] != "denied" {
                *flow = json!({"phase":"failed","message":"登录校验未完成，原账户未被替换。请重新开始登录。"});
            }
            (
                false,
                "登录未完成或校验未通过，原账户未被替换。请返回软件重新开始。",
            )
        }
    };
    let mut response=(if ok{StatusCode::OK}else{StatusCode::BAD_REQUEST},Html(format!("<!doctype html><meta charset=utf-8><title>真题学习 · ChatGPT 登录</title><h1>真题学习</h1><p>{msg}</p><p>可以关闭此页面。</p>"))).into_response();
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response.headers_mut().insert(
        "content-security-policy",
        "default-src 'none'; frame-ancestors 'none'"
            .parse()
            .unwrap(),
    );
    response
}
async fn select(
    State(a): State<App>,
    Json(r): Json<AccountRequest>,
) -> Result<Json<Value>, ApiError> {
    a.siwc.select(&r.registration_id).await?;
    Ok(Json(a.siwc.status()?))
}
async fn logout(
    State(a): State<App>,
    Json(r): Json<AccountRequest>,
) -> Result<Json<Value>, ApiError> {
    let revoked = a.siwc.logout(&r.registration_id).await?;
    Ok(Json(
        json!({"signed_out":true,"remote_revocation_confirmed":revoked,"message":if revoked{"已退出该账户并确认注销可续期会话。"}else{"本机凭据已清除；远程撤销未确认，请在 ChatGPT 设置中断开本应用。"}}),
    ))
}
async fn welcome(State(a): State<App>) -> Result<Json<Value>, ApiError> {
    a.siwc.welcome().await?;
    Ok(Json(json!({"ok":true})))
}
async fn models(
    State(a): State<App>,
    Json(r): Json<ModelRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(a.siwc.models(&r.registration_id).await?))
}

#[cfg(test)]
mod tests;
