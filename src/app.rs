use std::{
    collections::HashMap,
    env,
    fs as stdfs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

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
    companion,
    config::{
        AppConfig, GatewayBindings, ModelConfig, ProactiveConfig, TenantConfig,
        TokenGatewayConfig, WebGatewayConfig,
    },
    database::{AppDatabase, AuthenticatedAccount, ChatMessageRecord, ProfileRecord, UpsertProfileInput},
    error::AppError,
    provider::{GEMINI_OPENAI_BASE_URL, GEMINI_PROVIDER},
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

    pub fn database(&self) -> Arc<AppDatabase> {
        self.database.clone()
    }

    pub fn http_client(&self) -> reqwest::Client {
        self.http_client.clone()
    }

    pub async fn tenant(&self, tenant_id: &str) -> Option<TenantRuntime> {
        self.inner.read().await.tenants.get(tenant_id).cloned()
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
    tenant_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateTenantRequest {
    id: String,
    display_name: String,
    route: Option<String>,
    model: Option<String>,
    api_key_env: Option<String>,
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
        .route("/gemini-guide", get(gemini_guide_page))
        .route("/gemini-guide.html", get(gemini_guide_page))
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
        .route("/api/admin/tenants", post(create_tenant))
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

async fn gemini_guide_page(State(state): State<AppState>) -> Result<Html<String>, ApiError> {
    serve_html(&state.web_root, "gemini-guide.html").await
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

    let runtime = state.inner.read().await;
    let tenant_id = resolve_requested_tenant(&runtime, request.tenant_id.as_deref())?;
    if state
        .database
        .find_account_by_email_in_tenant(&tenant_id, &email)?
        .is_some()
    {
        return Err(ApiError::bad_request(
            "email is already registered for this companion".to_string(),
        ));
    }
    let default_tenant = runtime
        .tenants
        .get(&tenant_id)
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
    let runtime = state.inner.read().await;
    let tenant_id = resolve_requested_tenant(&runtime, request.tenant_id.as_deref())?;
    drop(runtime);

    let account_record = state
        .database
        .find_account_by_email_in_tenant(&tenant_id, &email)?
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
    validate_personal_inference_request(&account.profile, &request)?;

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
    if request.personal_inference_enabled
        && request
            .personal_inference_model
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
    {
        return Err(ApiError::bad_request(
            "personal_inference_model is required when personal inference is enabled".to_string(),
        ));
    }

    Ok(())
}

fn validate_personal_inference_request(
    current_profile: &ProfileRecord,
    request: &UpsertProfileInput,
) -> Result<(), ApiError> {
    if !request.personal_inference_enabled {
        return Ok(());
    }

    let has_new_key = request
        .personal_inference_api_key
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if !has_new_key && !current_profile.personal_inference_api_key_configured {
        return Err(ApiError::bad_request(
            "add a Gemini API key before enabling personal inference".to_string(),
        ));
    }
    if has_new_key && env::var("WELLBEING_MASTER_KEY").is_err() {
        return Err(ApiError::bad_request(
            "WELLBEING_MASTER_KEY must be set on the server before personal Gemini keys can be stored"
                .to_string(),
        ));
    }

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

    let tenant = state
        .tenant(&account.tenant_id)
        .await
        .ok_or_else(|| ApiError::internal("tenant not found for account".to_string()))?;
    let reply = companion::respond_to_user_message(
        state.database.as_ref(),
        &state.http_client,
        &tenant,
        &account,
        input,
    )
    .await?;
    Ok(Json(ChatReplyEnvelope { reply }))
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

async fn create_tenant(
    State(state): State<AppState>,
    Json(request): Json<CreateTenantRequest>,
) -> Result<Json<TenantSummary>, ApiError> {
    let tenant_id = request.id.trim().to_ascii_lowercase();
    if tenant_id.is_empty() {
        return Err(ApiError::bad_request("tenant id is required".to_string()));
    }
    if !tenant_id
        .chars()
        .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == '-' || value == '_')
    {
        return Err(ApiError::bad_request(
            "tenant id may only contain lowercase letters, numbers, hyphens, and underscores"
                .to_string(),
        ));
    }

    let display_name = request.display_name.trim().to_string();
    if display_name.is_empty() {
        return Err(ApiError::bad_request("display_name is required".to_string()));
    }

    let mut state = state.inner.write().await;
    if state.tenants.contains_key(&tenant_id) {
        return Err(ApiError::bad_request(format!(
            "tenant '{tenant_id}' already exists"
        )));
    }

    let fallback_model = state
        .config
        .tenants
        .iter()
        .find(|tenant| tenant.id == state.default_tenant_id)
        .map(|tenant| tenant.model.clone())
        .or_else(|| state.config.tenants.first().map(|tenant| tenant.model.clone()))
        .ok_or_else(|| ApiError::internal("default tenant config missing".to_string()))?;

    let model = ModelConfig {
        provider: GEMINI_PROVIDER.to_string(),
        base_url: GEMINI_OPENAI_BASE_URL.to_string(),
        model: request
            .model
            .and_then(normalize_optional_string)
            .unwrap_or_else(|| fallback_model.model.clone()),
        api_key_env: request
            .api_key_env
            .and_then(normalize_optional_string)
            .or_else(|| fallback_model.api_key_env.clone())
            .or_else(|| Some("GEMINI_API_KEY".to_string())),
    };

    let route = request
        .route
        .and_then(normalize_optional_string)
        .unwrap_or_else(|| format!("/t/{tenant_id}"));
    let tenant_config = TenantConfig {
        id: tenant_id.clone(),
        display_name: display_name.clone(),
        route,
        agent_path: format!("../templates/tenant/{tenant_id}/agent.md"),
        bootstrap_path: format!("../templates/tenant/{tenant_id}/bootstrap.md"),
        memory_path: format!("../data/{tenant_id}.sqlite"),
        model,
        proactive: ProactiveConfig {
            gentle_checkins_enabled: true,
            quiet_hours: vec!["22:00-07:00".to_string()],
        },
        gateways: GatewayBindings {
            web: Some(WebGatewayConfig { enabled: true }),
            telegram: Some(TokenGatewayConfig {
                enabled: true,
                token_env: Some(format!("{}_TELEGRAM_TOKEN", tenant_id.to_ascii_uppercase().replace('-', "_"))),
                binding: None,
            }),
            whatsapp: None,
            discord: None,
        },
    };
    tenant_config
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;

    let config_dir = state
        .config_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let agent_path = tenant_config.resolve_path(&config_dir, &tenant_config.agent_path);
    let bootstrap_path = tenant_config.resolve_path(&config_dir, &tenant_config.bootstrap_path);
    let tenant_template_dir = agent_path
        .parent()
        .ok_or_else(|| ApiError::internal("tenant template path is invalid".to_string()))?;
    stdfs::create_dir_all(tenant_template_dir).map_err(|source| AppError::CreateDirectory {
        path: tenant_template_dir.to_path_buf(),
        source,
    })?;
    stdfs::write(&agent_path, render_default_agent_template(&display_name))
        .map_err(|source| AppError::WriteConfig {
            path: agent_path.clone(),
            source,
        })?;
    stdfs::write(&bootstrap_path, render_default_bootstrap_template(&display_name))
        .map_err(|source| AppError::WriteConfig {
            path: bootstrap_path.clone(),
            source,
        })?;

    state.config.tenants.push(tenant_config.clone());
    state
        .config
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    state.config.save(&state.config_path)?;

    let runtime_tenant = TenantRuntime::from_config(&config_dir, &tenant_config)?;
    let summary = runtime_tenant.summary();
    state.tenants.insert(tenant_id, runtime_tenant);
    Ok(Json(summary))
}

async fn update_tenant_model(
    AxumPath(tenant_id): AxumPath<String>,
    State(state): State<AppState>,
    Json(request): Json<UpdateModelRequest>,
) -> Result<Json<TenantSummary>, ApiError> {
    let provider = request.provider.trim().to_ascii_lowercase();
    if !provider.is_empty()
        && !matches!(provider.as_str(), "gemini" | "gemini-openai" | "openai-compatible")
    {
        return Err(ApiError::bad_request(
            "only Gemini's OpenAI-compatible endpoint is supported".to_string(),
        ));
    }
    let base_url = request.base_url.trim();
    if !base_url.is_empty() && base_url.trim_end_matches('/') != GEMINI_OPENAI_BASE_URL {
        return Err(ApiError::bad_request(
            "base_url must point to Google's Gemini OpenAI-compatible endpoint".to_string(),
        ));
    }

    let model = ModelConfig {
        provider: GEMINI_PROVIDER.to_string(),
        base_url: GEMINI_OPENAI_BASE_URL.to_string(),
        model: request.model.trim().to_string(),
        api_key_env: request.api_key_env.and_then(normalize_optional_string),
    };

    if model.model.is_empty() {
        return Err(ApiError::bad_request(
            "model is required".to_string(),
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

fn resolve_requested_tenant(
    runtime: &RuntimeState,
    requested_tenant_id: Option<&str>,
) -> Result<String, ApiError> {
    let tenant_id = requested_tenant_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(runtime.default_tenant_id.as_str());
    runtime
        .tenants
        .contains_key(tenant_id)
        .then(|| tenant_id.to_string())
        .ok_or_else(|| ApiError::bad_request(format!("tenant '{tenant_id}' was not found")))
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
    <input name="provider" value="{provider}" readonly />
  </label>
  <label>Inference base URL
    <input name="base_url" value="{base_url}" readonly />
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
    .form-grid {{ display: grid; gap: 0.75rem; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); }}
  </style>
</head>
<body>
  <h1>Wellbeing admin model controls</h1>
  <p>Use this portal to manage Gemini endpoint settings, add new tenants, and keep lightweight companion instances portable. Changes are written back to <code>config.json</code>.</p>
  <section class="card" style="margin-bottom: 1rem;">
    <h2>Create a new tenant</h2>
    <div class="form-grid">
      <label>Tenant ID
        <input name="new_id" placeholder="calm-room" />
      </label>
      <label>Display name
        <input name="new_display_name" placeholder="Calm Room" />
      </label>
      <label>Route
        <input name="new_route" placeholder="/t/calm-room" />
      </label>
      <label>Provider
        <input name="new_provider" value="gemini-openai" readonly />
      </label>
      <label>Base URL
        <input name="new_base_url" value="https://generativelanguage.googleapis.com/v1beta/openai" readonly />
      </label>
      <label>Model
        <input name="new_model" placeholder="gemini-2.5-flash" />
      </label>
      <label>API key env var
        <input name="new_api_key_env" placeholder="GEMINI_API_KEY" />
      </label>
    </div>
    <button onclick="createTenant(this.closest('.card'))">Create tenant</button>
    <p class="status" id="status-create-tenant"></p>
  </section>
  <div class="grid">{cards}</div>
  <script>
    async function createTenant(card) {{
      const status = document.getElementById('status-create-tenant');
      status.textContent = 'Creating...';
      const payload = {{
        id: card.querySelector('[name="new_id"]').value,
        display_name: card.querySelector('[name="new_display_name"]').value,
        route: card.querySelector('[name="new_route"]').value,
        provider: card.querySelector('[name="new_provider"]').value,
        base_url: card.querySelector('[name="new_base_url"]').value,
        model: card.querySelector('[name="new_model"]').value,
        api_key_env: card.querySelector('[name="new_api_key_env"]').value
      }};
      const response = await fetch('/api/admin/tenants', {{
        method: 'POST',
        headers: {{ 'Content-Type': 'application/json' }},
        body: JSON.stringify(payload)
      }});
      const body = await response.json();
      if (response.ok) {{
        status.textContent = `Created tenant ${{body.display_name}}. Refresh to manage its settings.`;
        card.querySelectorAll('input').forEach((input) => (input.value = ''));
      }} else {{
        status.textContent = `Error: ${{body.error || 'unknown error'}}`;
      }}
    }}

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

fn render_default_agent_template(display_name: &str) -> String {
    format!(
        "You are {display_name}, a calm and emotionally supportive companion.\n\nYour purpose is to help the user feel heard, grounded, and less alone.\n\nYou are not a work copilot, therapist, doctor, lawyer, teacher, coder, or crisis professional.\n\nYou should:\n- listen carefully\n- reflect feelings with warmth and clarity\n- keep your tone gentle and human\n- avoid sounding robotic or overly formal\n- encourage small, practical next steps when appropriate\n\nYou must not:\n- give medical diagnoses\n- provide self-harm instructions\n- escalate conflict\n- act like an all-knowing assistant\n\nIf the user shows signs of crisis, switch to the crisis-safe response policy immediately.\n"
    )
}

fn render_default_bootstrap_template(display_name: &str) -> String {
    format!(
        "The user is starting a new relationship with {display_name}.\n\nHow to begin:\n- introduce yourself simply\n- explain that you are here for supportive conversation\n- ask one gentle opening question\n- avoid long disclaimers unless safety requires them\n\nMemory expectations:\n- remember preferences and themes over time\n- do not invent facts about the user\n- if unsure, ask instead of assuming\n"
    )
}
