use crate::{bible::Bible, model::{Command, Service, Snapshot}, songselect};
use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::{IpAddr, SocketAddr}, path::PathBuf, sync::{Arc, Mutex}, time::{Duration, Instant}};
use uuid::Uuid;

pub struct Host {
    pub snapshot: Mutex<Snapshot>,
    pub operator_token: String,
    remote_token: String,
    pub pairing_code: String,
    attempts: Mutex<HashMap<IpAddr, (Instant, u8)>>,
    pub path: PathBuf,
    pub remote_url: Option<String>,
    bible: Bible,
}

impl Host {
    pub fn new(service: Service, path: PathBuf, remote_url: Option<String>) -> Self {
        Self {
            snapshot: Mutex::new(Snapshot::new(service)),
            operator_token: Uuid::new_v4().simple().to_string(),
            remote_token: Uuid::new_v4().simple().to_string(),
            pairing_code: format!("{:08}", Uuid::new_v4().as_u128() % 100_000_000),
            attempts: Mutex::new(HashMap::new()), path, remote_url, bible: Bible::kjv(),
        }
    }
}

type Error = (StatusCode, String);
fn bad(message: impl Into<String>) -> Error { (StatusCode::BAD_REQUEST, message.into()) }
fn fault() -> Error { (StatusCode::INTERNAL_SERVER_ERROR, "The presentation host is unavailable.".into()) }

fn authorize(host: &Host, headers: &HeaderMap, operator_only: bool) -> Result<(), Error> {
    let token = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    match token {
        Some(token) if token == host.operator_token => Ok(()),
        Some(token) if token == host.remote_token && !operator_only => Ok(()),
        Some(token) if token == host.remote_token => Err((StatusCode::FORBIDDEN, "Use the operator screen to edit the service.".into())),
        _ => Err((StatusCode::UNAUTHORIZED, "Please connect to this presentation host again.".into())),
    }
}

pub fn router(host: Arc<Host>) -> Router {
    Router::new()
        .route("/", get(|| async { Html(include_str!("../web/index.html")) }))
        .route("/remote", get(|| async { Html(include_str!("../web/remote.html")) }))
        .route("/output", get(|| async { Html(include_str!("../web/output.html")) }))
        .route("/app.css", get(|| async { ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], include_str!("../web/app.css")) }))
        .route("/app.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], include_str!("../web/app.js")) }))
        .route("/api/state", get(snapshot))
        .route("/api/operator", get(operator_info))
        .route("/api/pair", post(pair))
        .route("/api/command", post(command))
        .route("/api/service", put(save_service))
        .route("/api/import/songselect", post(import_song))
        .route("/api/bible", get(bible_lookup))
        .route("/api/import/bible", post(import_bible))
        .layer(DefaultBodyLimit::max(4 * 1024 * 1024))
        .layer(middleware::from_fn(security_headers))
        .with_state(host)
}

async fn security_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    headers.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    headers.insert(header::CONTENT_SECURITY_POLICY, "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'self'; base-uri 'none'; form-action 'self'".parse().unwrap());
    response
}

#[derive(Deserialize)]
struct Since { since: Option<u64> }
async fn snapshot(State(host): State<Arc<Host>>, headers: HeaderMap, Query(query): Query<Since>) -> Result<Response, Error> {
    authorize(&host, &headers, false)?;
    let snapshot = host.snapshot.lock().map_err(|_| fault())?;
    if query.since == Some(snapshot.revision) { return Ok(StatusCode::NO_CONTENT.into_response()); }
    Ok(Json(snapshot.clone()).into_response())
}

#[derive(Serialize)]
struct OperatorInfo { pairing_code: String, remote_url: Option<String>, songselect: &'static str }
async fn operator_info(State(host): State<Arc<Host>>, headers: HeaderMap) -> Result<Json<OperatorInfo>, Error> {
    authorize(&host, &headers, true)?;
    Ok(Json(OperatorInfo { pairing_code: host.pairing_code.clone(), remote_url: host.remote_url.clone(), songselect: "Direct integration pending CCLI developer access" }))
}

#[derive(Deserialize)]
struct PairRequest { code: String }
#[derive(Serialize)]
struct PairResponse { token: String }
async fn pair(State(host): State<Arc<Host>>, ConnectInfo(address): ConnectInfo<SocketAddr>, Json(body): Json<PairRequest>) -> Result<Json<PairResponse>, Error> {
    let mut attempts = host.attempts.lock().map_err(|_| fault())?;
    attempts.retain(|_, (start, _)| start.elapsed() < Duration::from_secs(60));
    if attempts.len() > 2048 { return Err((StatusCode::TOO_MANY_REQUESTS, "Try pairing again in a minute.".into())); }
    let entry = attempts.entry(address.ip()).or_insert((Instant::now(), 0));
    if entry.1 >= 5 { return Err((StatusCode::TOO_MANY_REQUESTS, "Too many pairing attempts. Try again in a minute.".into())); }
    entry.1 += 1;
    if body.code.trim() != host.pairing_code { return Err((StatusCode::UNAUTHORIZED, "That code did not match the operator screen.".into())); }
    Ok(Json(PairResponse { token: host.remote_token.clone() }))
}

async fn command(State(host): State<Arc<Host>>, headers: HeaderMap, Json(command): Json<Command>) -> Result<Json<Snapshot>, Error> {
    authorize(&host, &headers, false)?;
    let mut snapshot = host.snapshot.lock().map_err(|_| fault())?;
    snapshot.command(command).map_err(bad)?;
    Ok(Json(snapshot.clone()))
}

#[derive(Deserialize)]
struct SaveRequest { revision: u64, service: Service }
async fn save_service(State(host): State<Arc<Host>>, headers: HeaderMap, Json(body): Json<SaveRequest>) -> Result<Json<Snapshot>, Error> {
    authorize(&host, &headers, true)?;
    body.service.validate().map_err(bad)?;
    let mut snapshot = host.snapshot.lock().map_err(|_| fault())?;
    if body.revision != snapshot.service_revision {
        return Err((StatusCode::CONFLICT, "This service changed in another operator tab. Reload before saving your edits.".into()));
    }
    body.service.save(&host.path).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    snapshot.service = body.service;
    snapshot.service_revision += 1;
    snapshot.revision += 1;
    Ok(Json(snapshot.clone()))
}

fn append(host: &Host, item: crate::model::ServiceItem) -> Result<Json<Snapshot>, Error> {
    let mut snapshot = host.snapshot.lock().map_err(|_| fault())?;
    let mut service = snapshot.service.clone();
    service.items.push(item);
    service.save(&host.path).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    snapshot.service = service;
    snapshot.service_revision += 1;
    snapshot.revision += 1;
    Ok(Json(snapshot.clone()))
}

#[derive(Deserialize)]
struct SongRequest { text: String, lines_per_slide: usize }
async fn import_song(State(host): State<Arc<Host>>, headers: HeaderMap, Json(body): Json<SongRequest>) -> Result<Json<Snapshot>, Error> {
    authorize(&host, &headers, true)?;
    let item = songselect::import_txt(&body.text, body.lines_per_slide).map_err(bad)?;
    append(&host, item)
}

#[derive(Deserialize)]
struct BibleRequest { reference: String }
async fn bible_lookup(State(host): State<Arc<Host>>, headers: HeaderMap, Query(body): Query<BibleRequest>) -> Result<Json<Vec<crate::bible::Verse>>, Error> {
    authorize(&host, &headers, true)?;
    Ok(Json(host.bible.lookup(&body.reference).map_err(bad)?))
}
async fn import_bible(State(host): State<Arc<Host>>, headers: HeaderMap, Json(body): Json<BibleRequest>) -> Result<Json<Snapshot>, Error> {
    authorize(&host, &headers, true)?;
    append(&host, host.bible.slides(&body.reference).map_err(bad)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::{Body, to_bytes}, http::Request};
    use tower::ServiceExt;

    fn host() -> Arc<Host> {
        Arc::new(Host::new(Service::default(), std::env::temp_dir().join(format!("rustypresenter-{}.json", Uuid::new_v4())), None))
    }

    #[tokio::test]
    async fn lyrics_require_authentication_and_remote_cannot_edit() {
        let host = host();
        let app = router(host.clone());
        let response = app.clone().oneshot(Request::builder().uri("/api/state").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = app.clone().oneshot(Request::builder().uri("/api/state").header("Authorization", format!("Bearer {}", host.remote_token)).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = app.oneshot(Request::builder().method("PUT").uri("/api/service").header("Authorization", format!("Bearer {}", host.remote_token)).header("Content-Type", "application/json").body(Body::from(serde_json::json!({"revision":0,"service":Service::default()}).to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn pairing_is_rate_limited() {
        let host = host();
        let app = router(host);
        for attempt in 0..6 {
            let mut request = Request::builder().method("POST").uri("/api/pair").header("Content-Type", "application/json").body(Body::from("{\"code\":\"invalid\"}")).unwrap();
            request.extensions_mut().insert(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()));
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), if attempt < 5 { StatusCode::UNAUTHORIZED } else { StatusCode::TOO_MANY_REQUESTS });
        }
    }

    #[tokio::test]
    async fn authenticated_command_changes_live_output() {
        let host = host();
        let response = router(host.clone()).oneshot(Request::builder().method("POST").uri("/api/command").header("Authorization", format!("Bearer {}", host.remote_token)).header("Content-Type", "application/json").body(Body::from("{\"type\":\"next\"}")).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["live"]["item_title"], "Welcome");
        assert_eq!(body["revision"], 1);
    }
}
