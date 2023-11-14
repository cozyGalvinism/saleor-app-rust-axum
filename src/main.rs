use std::net::SocketAddr;

use anyhow::Context;
use askama::Template;
use axum::{Router, routing::{get, post}, response::{IntoResponse, Html}, http::{StatusCode, HeaderMap}, error_handling::HandleErrorLayer, BoxError, extract::Host, Json};
use cynic::{QueryBuilder, http::ReqwestExt};
use reqwest::Url;
use saleor::{SaleorManifest, SaleorAppPermission, ExtractRegisterRequest, AuthData, AplId, SaleorRegisterResponse, SaleorApl, SaleorClientAuthenticationRequest, SaleorAppExtension, SaleorAppExtensionMount, SaleorAppExtensionTarget, verify_jwt, MyId};
use templating::HtmlTemplate;
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_sessions::{MemoryStore, SessionManagerLayer, Session};
use tracing::{info, instrument};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::saleor::{SaleorAuthLayer, FileAplStore, SaleorPermission, SaleorAplLayer};

mod saleor;
mod templating;

const APP_ID: &str = env!("CARGO_PKG_NAME");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{APP_ID}=debug").into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
    
    info!("initializing router");

    let session_store = MemoryStore::default();
    let session_service = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|_: BoxError| async {
            StatusCode::BAD_REQUEST
        }))
        .layer(SessionManagerLayer::new(session_store).with_secure(true).with_same_site(tower_sessions::cookie::SameSite::None));

    let apl_layer = SaleorAplLayer::new(FileAplStore);
    let auth_layer = SaleorAuthLayer::with_permissions(&[SaleorPermission::ManageProducts]);

    let api_router = Router::new()
        .route("/hello", get(api_hello))
        .layer(auth_layer)
        .route("/manifest", get(manifest))
        .route("/register", post(register))
        .route("/auth", post(auth));

    let app_router = Router::new()
        .route("/", get(index));

    let assets_path = std::env::current_dir().unwrap();
    let router  = Router::new()
        .route("/", get(index))
        .nest("/app", app_router)
        .nest("/api", api_router)
        .layer(apl_layer)
        .layer(session_service)
        .nest_service(
            "/assets", 
            ServeDir::new(format!("{}/assets", assets_path.display()))
        );
    let port = 8008;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    info!("router initialized, now listening on port {port}");

    axum::Server::bind(&addr)
        .serve(router.into_make_service())
        .await
        .context("error while starting server")?;

    Ok(())
}

async fn api_hello() -> impl IntoResponse {
    "Hello from the API"
}

async fn index() -> impl IntoResponse {
    HtmlTemplate(templating::ExamplePage)
}

pub async fn manifest(Host(host): Host, headers: HeaderMap) -> impl IntoResponse {
    let scheme = headers.get("x-forwarded-proto").map(|h| h.to_str().unwrap()).unwrap_or("https");
    let base_url = format!("{}://{}", scheme, host);

    SaleorManifest {
        id: APP_ID.to_string(),
        version: APP_VERSION.to_string(),
        required_saleor_version: None,
        name: APP_ID.to_string(),
        permissions: vec![SaleorAppPermission::ManageProducts],
        app_url: base_url.clone(),
        token_target_url: format!("{}/api/register", base_url),
        author: None,
        about: None,
        data_privacy_url: None,
        homepage_url: None,
        support_url: None,
        extensions: Some(vec![
            SaleorAppExtension {
                label: "Example Extension".to_string(),
                mount: SaleorAppExtensionMount::ProductOverviewMoreActions,
                target: SaleorAppExtensionTarget::AppPage,
                permissions: vec![],
                url: "/app".to_string(),
            }
        ]),
        webhooks: None,
        brand: None,
    }
}

pub async fn register(apl: SaleorApl, ExtractRegisterRequest(request): ExtractRegisterRequest) -> impl IntoResponse {
    let Ok(api_url) = Url::parse(&request.saleor_api_url) else {
        return SaleorRegisterResponse::api_url_parsing_failed();
    };
    let jwks_url = format!("{}/.well-known/jwks.json", api_url.origin().ascii_serialization());
    let Ok(response) = reqwest::get(&jwks_url).await else {
        return SaleorRegisterResponse::jwks_not_available();
    };
    let Ok(jwks) = response.text().await else {
        return SaleorRegisterResponse::jwks_not_available();
    };

    let auth_data = AuthData {
        domain: Some(request.saleor_domain),
        token: request.auth_token,
        saleor_api_url: request.saleor_api_url,
        app_id: APP_ID.to_string(),
        jwks: Some(jwks),
    };
    apl.set(&Into::<AplId>::into(&auth_data), auth_data).await;

    SaleorRegisterResponse::success()
}

pub async fn auth(session: Session, apl: SaleorApl, Json(auth_request): Json<SaleorClientAuthenticationRequest>) -> impl IntoResponse {
    session.insert("token", &auth_request.token).expect("failed to insert token into session");
    session.insert("saleor_api_url", &auth_request.api_url).expect("failed to insert saleor_api_url into session");

    let jwks = match apl.get(&AplId::from_api_url(&auth_request.api_url)).await {
        Some(auth_data) => {
            match auth_data.jwks {
                Some(jwks) => jwks,
                None => {
                    let jwks_url = format!("{}/.well-known/jwks.json", &auth_request.api_url);
                    reqwest::get(&jwks_url).await.unwrap().text().await.unwrap()
                }
            }
        },
        None => {
            let jwks_url = format!("{}/.well-known/jwks.json", &auth_request.api_url);
            reqwest::get(&jwks_url).await.unwrap().text().await.unwrap()
        }
    };
    if let Err(e) = verify_jwt(&jwks, &auth_request.token, &[]) {
        return (StatusCode::UNAUTHORIZED, e).into_response();
    }

    let operation = MyId::build(());
    let client = reqwest::Client::new();
    let response = client.post(&auth_request.api_url).run_graphql(operation).await;
    let response = match response {
        Ok(response) => response,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    if response.data.is_none() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "no data in response".to_string()).into_response();
    }

    StatusCode::OK.into_response()
}
