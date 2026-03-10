use std::{collections::HashMap, path::{Path, PathBuf}, str::FromStr, sync::Arc};

use axum::{
    extract::{Path as AxumPath, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{Duration, NaiveTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use tokio::{fs, sync::RwLock};

use crate::{
    auth,
    config::{AppConfig, ModelConfig},
    database::{AppDatabase, AuthenticatedAccount, ChatMessageRecord, ProfileRecord, UpsertProfileInput},
    error::AppError,
    guardrails::{self, GuardrailDecision},
    provider::{self, ProviderMessage},
    tenant::{TenantRuntime, TenantSummary},
};

const SESSION_COOKIE: &str = "wb_session";
const SESSION_DAYS: i64 = 30;

#[derive(Clone)]
pub struct AppState {
    inner: Arc<RwLock<RuntimeState>>,
    database: Arc<AppDatabase>,
    web_root: PathBuf,
    http_client: reqwest::Client,
}

impl AppState {
    pub fn new(
        config_path: PathBuf,
        config: AppConfig,
        tenants: Vec<TenantRuntime>,
        database: Arc<AppDatabase>,
        web_root: PathBuf,
    ) -> Self {
        Self {
            inner: Arc::new(RwLock::new(RuntimeState::new(config_path, config, tenants))),
            database,
            web_root,
            http_client: reqwest::Client::new(),
        }
    }

    pub async fn tenant_count(&self) -> usize {
        self.inner.read().await.tenants.len()
    }
}

struct RuntimeState {
    config_path: PathBuf,
    config: AppConfig,
    default_tenant_id: String,
    tenants: HashMap<String, TenantRuntime>,
}

impl RuntimeState {
    fn new(config_path: PathBuf, config: AppConfig, tenants: Vec<TenantRuntime>) -> Self {
        let default_tenant_id = tenants
            .first()
            .map(|tenant| tenant.id.clone())
            .unwrap_or_else(|| "default".to_string());
        let tenants = tenants
            .into_iter()
            .map(|tenant| (tenant.id.clone(), tenant))
            .collect();

        Self {
            config_path,
            config,
            default_tenant_id,
            tenants,
        }
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    tenant_count: usize,
}

#[derive(Debug, Deserialize)]
struct UpdateModelRequest {
    provider: String,
    base_url: String,
    model: String,
    api_key_env: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthRequest {
    email: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct AuthEnvelope {
    account: AuthenticatedAccount,
}

#[derive(Debug, Serialize)]
struct ProfileEnvelope {
    profile: ProfileRecord,
}

#[derive(Debug, Serialize)]
struct ChatMessagesEnvelope {
    messages: Vec<ChatMessageRecord>,
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    message: String,
}

#[derive(Debug, Serialize)]
struct ChatReplyEnvelope {
    reply: ChatMessageRecord,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(landing_page))
        .route("/index.html", get(landing_page))
        .route("/login", get(auth_page))
        .route("/login.html", get(auth_page))
        .route("/signup", get(auth_page))
        .route("/onboarding", get(onboarding_page))
        .route("/onboarding.html", get(onboarding_page))
        .route("/chat", get(chat_page))
        .route("/chat.html", get(chat_page))
        .route("/settings", get(settings_page))
        .route("/settings.html", get(settings_page))
        .route("/css/*path", get(css_asset))
        .route("/js/*path", get(js_asset))
        .route("/admin", get(admin_portal))
        .route("/health", get(health))
        .route("/tenants", get(list_tenants))
        .route("/tenants/:tenant_id", get(get_tenant))
        .route("/api/auth/signup", post(signup))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        .route("/api/me/profile", get(get_profile).put(update_profile))
        .route("/api/me/reset", post(reset_bot))
        .route("/api/me/account", delete(delete_account))
        .route("/api/chat/messages", get(list_messages))
        .route("/api/chat", post(send_chat))
        .route("/api/admin/tenants/:tenant_id/model", post(update_tenant_model))
        .with_state(state)
}

async fn landing_page(State(state): State<AppState>) -> Result<Html<String>, ApiError> {
    serve_html(&state.web_root, "index.html").await
}

async fn auth_page(State(state): State<AppState>) -> Result<Html<String>, ApiError> {
    serve_html(&state.web_root, "login.html").await
}

async fn onboarding_page(State(state): State<AppState>) -> Result<Html<String>, ApiError> {
    serve_html(&state.web_root, "onboarding.html").await
}

async fn chat_page(State(state): State<AppState>) -> Result<Html<String>, ApiError> {
    serve_html(&state.web_root, "chat.html").await
}

async fn settings_page(State(state): State<AppState>) -> Result<Html<String>, ApiError> {
    serve_html(&state.web_root, "settings.html").await
}

async fn css_asset(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Result<Response, ApiError> {
    let path = sanitize_asset_path(&path)?;
    let full_path = state.web_root.join("css").join(path);
    let bytes = fs::read(&full_path).await.map_err(|source| AppError::ReadFile {
        path: full_path.clone(),
        source,
    })?;
    let mut response = Response::new(bytes.into());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&full_path)),
    );
    Ok(response)
}

async fn js_asset(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Result<Response, ApiError> {
    let path = sanitize_asset_path(&path)?;
    let full_path = state.web_root.join("js").join(path);
    let bytes = fs::read(&full_path).await.map_err(|source| AppError::ReadFile {
        path: full_path.clone(),
        source,
    })?;
    let mut response = Response::new(bytes.into());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&full_path)),
    );
    Ok(response)
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        tenant_count: state.tenant_count().await,
    })
}

async fn list_tenants(State(state): State<AppState>) -> Json<Vec<TenantSummary>> {
    let state = state.inner.read().await;
    let mut tenants = state
        .tenants
        .values()
        .map(TenantRuntime::summary)
        .collect::<Vec<_>>();
    tenants.sort_by(|left, right| left.id.cmp(&right.id));
    Json(tenants)
}

async fn get_tenant(
    AxumPath(tenant_id): AxumPath<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let state = state.inner.read().await;
    match state.tenants.get(&tenant_id) {
        Some(tenant) => (StatusCode::OK, Json(tenant.summary())).into_response(),
        None => ApiError::not_found(format!("tenant '{tenant_id}' not found")).into_response(),
    }
}

async fn signup(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(request): Json<AuthRequest>,
) -> Result<(CookieJar, Json<AuthEnvelope>), ApiError> {
    let email = request.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(ApiError::bad_request("a valid email is required".to_string()));
    }
    if request.password.len() < 8 {
        return Err(ApiError::bad_request(
            "password must be at least 8 characters".to_string(),
        ));
    }
    if state.database.find_account_by_email(&email)?.is_some() {
        return Err(ApiError::bad_request("email is already registered".to_string()));
    }

    let runtime = state.inner.read().await;
    let default_tenant = runtime
        .tenants
        .get(&runtime.default_tenant_id)
        .ok_or_else(|| ApiError::internal("default tenant missing".to_string()))?;

    let password_hash = auth::hash_password(&request.password)?;
    let account = state.database.create_account(
        &default_tenant.id,
        &email,
        &password_hash,
        &default_tenant.display_name,
    )?;
    drop(runtime);

    let (jar, account) = issue_session(&state.database, jar, account)?;
    Ok((jar, Json(AuthEnvelope { account })))
}

async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(request): Json<AuthRequest>,
) -> Result<(CookieJar, Json<AuthEnvelope>), ApiError> {
    let email = request.email.trim().to_lowercase();
    let account_record = state
        .database
        .find_account_by_email(&email)?
        .ok_or_else(|| ApiError::bad_request("invalid email or password".to_string()))?;

    if !auth::verify_password(&account_record.password_hash, &request.password)? {
        return Err(ApiError::bad_request("invalid email or password".to_string()));
    }

    let account = state
        .database
        .get_account_with_profile(account_record.id)?
        .ok_or_else(|| ApiError::internal("account profile missing".to_string()))?;

    let (jar, account) = issue_session(&state.database, jar, account)?;
    Ok((jar, Json(AuthEnvelope { account })))
}

async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(CookieJar, StatusCode), ApiError> {
    let jar = if let Some(cookie) = jar.get(SESSION_COOKIE) {
        state.database.delete_session(&auth::hash_session_token(cookie.value()))?;
        jar.remove(expired_session_cookie())
    } else {
        jar
    };

    Ok((jar, StatusCode::NO_CONTENT))
}

async fn me(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<AuthEnvelope>, ApiError> {
    let account = require_auth(&state, &jar).await?;
    Ok(Json(AuthEnvelope { account }))
}

async fn get_profile(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<ProfileEnvelope>, ApiError> {
    let account = require_auth(&state, &jar).await?;
    Ok(Json(ProfileEnvelope {
        profile: account.profile,
    }))
}

async fn update_profile(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(request): Json<UpsertProfileInput>,
) -> Result<Json<ProfileEnvelope>, ApiError> {
    let account = require_auth(&state, &jar).await?;

    validate_profile_request(&request)?;

    let profile = state.database.update_profile(account.id, &request)?;
    Ok(Json(ProfileEnvelope { profile }))
}

fn validate_profile_request(request: &UpsertProfileInput) -> Result<(), ApiError> {
    if request.companion_name.trim().is_empty() {
        return Err(ApiError::bad_request(
            "companion_name must not be empty".to_string(),
        ));
    }
    if request.timezone.trim().is_empty() || request.checkin_local_time.trim().is_empty() {
        return Err(ApiError::bad_request(
            "timezone and checkin_local_time are required".to_string(),
        ));
    }
    Tz::from_str(request.timezone.trim()).map_err(|_| {
        ApiError::bad_request("timezone must be a valid IANA timezone".to_string())
    })?;
    NaiveTime::parse_from_str(request.checkin_local_time.trim(), "%H:%M").map_err(|_| {
        ApiError::bad_request("checkin_local_time must use HH:MM format".to_string())
    })?;

    Ok(())
}

async fn list_messages(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<ChatMessagesEnvelope>, ApiError> {
    let account = require_auth(&state, &jar).await?;
    let messages = state.database.list_chat_messages(account.id, 100)?;
    Ok(Json(ChatMessagesEnvelope { messages }))
}

async fn send_chat(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(request): Json<ChatRequest>,
) -> Result<Json<ChatReplyEnvelope>, ApiError> {
    let account = require_auth(&state, &jar).await?;
    let input = request.message.trim();
    if input.is_empty() {
        return Err(ApiError::bad_request("message must not be empty".to_string()));
    }

    state.database.append_chat_message(account.id, "user", input)?;

    let reply = match guardrails::evaluate_user_message(input) {
        GuardrailDecision::Reply(message) => message,
        GuardrailDecision::Allow => {
            let runtime = state.inner.read().await;
            let tenant = runtime
                .tenants
                .get(&account.tenant_id)
                .ok_or_else(|| ApiError::internal("tenant not found for account".to_string()))?
                .clone();
            drop(runtime);

            let history = state.database.list_chat_messages(account.id, 24)?;
            let mut messages = Vec::with_capacity(history.len() + 1);
            messages.push(ProviderMessage {
                role: "system".to_string(),
                content: guardrails::system_prompt(&tenant, &account.profile),
            });
            for message in history {
                messages.push(ProviderMessage {
                    role: message.role,
                    content: message.content,
                });
            }

            provider::generate_reply(&state.http_client, &tenant.model, messages).await?
        }
    };

    state.database.append_chat_message(account.id, "assistant", &reply)?;
    Ok(Json(ChatReplyEnvelope {
        reply: ChatMessageRecord {
            role: "assistant".to_string(),
            content: reply,
            created_at: Utc::now().to_rfc3339(),
        },
    }))
}

async fn reset_bot(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<StatusCode, ApiError> {
    let account = require_auth(&state, &jar).await?;
    state.database.reset_companion(account.id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_account(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(CookieJar, StatusCode), ApiError> {
    let account = require_auth(&state, &jar).await?;
    state.database.delete_account(account.id)?;
    Ok((jar.remove(expired_session_cookie()), StatusCode::NO_CONTENT))
}

async fn admin_portal(State(state): State<AppState>) -> Html<String> {
    let state = state.inner.read().await;
    let mut tenants = state
        .tenants
        .values()
        .map(TenantRuntime::summary)
        .collect::<Vec<_>>();
    tenants.sort_by(|left, right| left.id.cmp(&right.id));
    Html(render_admin_html(&tenants))
}

async fn update_tenant_model(
    AxumPath(tenant_id): AxumPath<String>,
    State(state): State<AppState>,
    Json(request): Json<UpdateModelRequest>,
) -> Result<Json<TenantSummary>, ApiError> {
    let model = ModelConfig {
        provider: request.provider.trim().to_string(),
        base_url: request.base_url.trim().to_string(),
        model: request.model.trim().to_string(),
        api_key_env: request.api_key_env.and_then(normalize_optional_string),
    };

    if model.provider.is_empty() || model.base_url.is_empty() || model.model.is_empty() {
        return Err(ApiError::bad_request(
            "provider, base_url, and model are required".to_string(),
        ));
    }

    let mut state = state.inner.write().await;
    let tenant_index = state
        .config
        .tenants
        .iter()
        .position(|tenant| tenant.id == tenant_id)
        .ok_or_else(|| ApiError::not_found(format!("tenant '{tenant_id}' not found in config")))?;

    state.config.tenants[tenant_index].model = model.clone();
    state.config.save(&state.config_path)?;

    let tenant = state
        .tenants
        .get_mut(&tenant_id)
        .ok_or_else(|| ApiError::not_found(format!("tenant '{tenant_id}' not found")))?;

    tenant.update_model(model);
    Ok(Json(tenant.summary()))
}

async fn require_auth(state: &AppState, jar: &CookieJar) -> Result<AuthenticatedAccount, ApiError> {
    let cookie = jar
        .get(SESSION_COOKIE)
        .ok_or_else(|| ApiError::unauthorized("missing session".to_string()))?;
    let token_hash = auth::hash_session_token(cookie.value());
    state
        .database
        .get_account_by_session(&token_hash, &Utc::now().to_rfc3339())?
        .ok_or_else(|| ApiError::unauthorized("session is invalid or expired".to_string()))
}

fn issue_session(
    database: &AppDatabase,
    jar: CookieJar,
    account: AuthenticatedAccount,
) -> Result<(CookieJar, AuthenticatedAccount), ApiError> {
    let token = auth::new_session_token();
    let token_hash = auth::hash_session_token(&token);
    let expires_at = (Utc::now() + Duration::days(SESSION_DAYS)).to_rfc3339();
    database.create_session(account.id, &token_hash, &expires_at)?;

    let cookie = Cookie::build((SESSION_COOKIE, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::days(SESSION_DAYS))
        .build();

    Ok((jar.add(cookie), account))
}

fn expired_session_cookie() -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(0))
        .build()
}

async fn serve_html(web_root: &Path, file_name: &str) -> Result<Html<String>, ApiError> {
    let path = web_root.join(file_name);
    let html = fs::read_to_string(&path).await.map_err(|source| AppError::ReadFile {
        path,
        source,
    })?;
    Ok(Html(html))
}

fn sanitize_asset_path(path: &str) -> Result<PathBuf, ApiError> {
    if path.contains("..") {
        return Err(ApiError::bad_request("invalid asset path".to_string()));
    }
    Ok(PathBuf::from(path))
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|value| value.to_str()).unwrap_or_default() {
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        _ => "application/octet-stream",
    }
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }

    fn unauthorized(message: String) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message,
        }
    }

    fn not_found(message: String) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message,
        }
    }

    fn internal(message: String) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message,
        }
    }
}

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({
                "error": self.message
            })),
        )
            .into_response()
    }
}

fn normalize_optional_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn render_admin_html(tenants: &[TenantSummary]) -> String {
    let cards = tenants
        .iter()
        .map(|tenant| {
            format!(
                r#"
<section class="card">
  <h2>{display_name}</h2>
  <p><strong>Tenant ID:</strong> {tenant_id}</p>
  <p><strong>Gateways:</strong> {gateways}</p>
  <label>Provider
    <input name="provider" value="{provider}" list="provider-options-{tenant_id}" />
  </label>
  <datalist id="provider-options-{tenant_id}">
    <option value="github-models"></option>
    <option value="openai-compatible"></option>
    <option value="openai"></option>
    <option value="anthropic"></option>
    <option value="gemini"></option>
    <option value="groq"></option>
    <option value="ollama"></option>
    <option value="llama.cpp"></option>
  </datalist>
  <label>Inference base URL
    <input name="base_url" value="{base_url}" />
  </label>
  <label>Model
    <input name="model" value="{model}" />
  </label>
  <label>API key env var
    <input name="api_key_env" value="{api_key_env}" placeholder="OPTIONAL_ENV_VAR" />
  </label>
  <button onclick="saveTenantModel('{tenant_id}', this.closest('.card'))">Save model settings</button>
  <p class="status" id="status-{tenant_id}"></p>
</section>
"#,
                display_name = html_escape(&tenant.display_name),
                tenant_id = html_escape(&tenant.id),
                gateways = html_escape(&tenant.enabled_gateways.join(", ")),
                provider = html_escape(&tenant.model_provider),
                base_url = html_escape(&tenant.model_base_url),
                model = html_escape(&tenant.model_name),
                api_key_env = html_escape(tenant.model_api_key_env.as_deref().unwrap_or("")),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Wellbeing Admin</title>
  <style>
    :root {{
      color-scheme: dark;
      font-family: ui-sans-serif, system-ui, sans-serif;
      background: #0f172a;
      color: #e2e8f0;
    }}
    body {{ margin: 0; padding: 2rem; background: linear-gradient(180deg, #0f172a 0%, #111827 100%); }}
    .grid {{ display: grid; gap: 1rem; grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); }}
    .card {{ background: rgba(15, 23, 42, 0.8); border: 1px solid #334155; border-radius: 16px; padding: 1rem; }}
    label {{ display: block; margin: 0.75rem 0; }}
    input {{ width: 100%; margin-top: 0.35rem; border: 1px solid #475569; border-radius: 10px; padding: 0.75rem; box-sizing: border-box; background: #020617; color: #e2e8f0; }}
    button {{ border: 0; border-radius: 999px; padding: 0.75rem 1rem; background: #38bdf8; color: #082f49; font-weight: 700; cursor: pointer; }}
    .status {{ min-height: 1.25rem; color: #93c5fd; }}
  </style>
</head>
<body>
  <h1>Wellbeing admin model controls</h1>
  <p>Use this portal to change model providers and inference endpoints for each tenant. Changes are written back to <code>config.json</code>.</p>
  <div class="grid">{cards}</div>
  <script>
    async function saveTenantModel(tenantId, card) {{
      const status = document.getElementById(`status-${{tenantId}}`);
      status.textContent = 'Saving...';
      const payload = {{
        provider: card.querySelector('[name="provider"]').value,
        base_url: card.querySelector('[name="base_url"]').value,
        model: card.querySelector('[name="model"]').value,
        api_key_env: card.querySelector('[name="api_key_env"]').value
      }};
      const response = await fetch(`/api/admin/tenants/${{tenantId}}/model`, {{
        method: 'POST',
        headers: {{ 'Content-Type': 'application/json' }},
        body: JSON.stringify(payload)
      }});
      const body = await response.json();
      status.textContent = response.ok ? 'Saved.' : `Error: ${{body.error || 'unknown error'}}`;
    }}
  </script>
</body>
</html>"#,
        cards = cards
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
