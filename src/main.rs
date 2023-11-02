use std::net::SocketAddr;

use anyhow::Context;
use askama::Template;
use axum::{Router, routing::{get, post}, response::{IntoResponse, Html}, http::StatusCode, error_handling::HandleErrorLayer, BoxError};
use saleor::{SaleorManifest, SaleorAppPermission, ExtractRegisterRequest, AuthData, AplId, SaleorRegisterResponse};
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_sessions::{MemoryStore, SessionManagerLayer, Session};
use tracing::{info, instrument};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::saleor::SaleorAuthLayer;

mod saleor;
mod templating;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "saleor_logistiker=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
    
    info!("initializing router");

    let acl_store = MemoryStore::default();
    let session_service = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|_: BoxError| async {
            StatusCode::BAD_REQUEST
        }))
        .layer(SessionManagerLayer::new(acl_store).with_secure(false).with_same_site(tower_sessions::cookie::SameSite::Lax));

    let api_router = Router::new()
        .route("/manifest", get(manifest))
        .route("/register", post(register))
        .route("/hello", get(api_hello));

    let app_router = Router::new()
        .route("/", get(index));

    let assets_path = std::env::current_dir().unwrap();
    let router  = Router::new()
        .nest("/app", app_router)
        .layer(SaleorAuthLayer::with_permissions(&[]))
        .nest("/api", api_router)
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
    Html("<h1>Hello from the index</h1>".to_string())
}

pub async fn manifest() -> impl IntoResponse {
    SaleorManifest {
        id: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        required_saleor_version: None,
        name: env!("CARGO_PKG_NAME").to_string(),
        permissions: vec![SaleorAppPermission::ManageUsers, SaleorAppPermission::ManageStaff],
        app_url: "https://localhost:8008/app".to_string(),
        token_target_url: "https://localhost:8008/api/register".to_string(),
        author: None,
        about: None,
        data_privacy_url: None,
        homepage_url: None,
        support_url: None,
        extensions: None,
        webhooks: None,
        brand: None,
    }
}

pub async fn register(session: Session, ExtractRegisterRequest(request): ExtractRegisterRequest) -> impl IntoResponse {
    let jwks_url = format!("{}/.well-known/jwks.json", &request.saleor_api_url);
    let jwks = reqwest::get(&jwks_url).await.unwrap().text().await.unwrap();

    let auth_data = AuthData {
        domain: Some(request.saleor_domain),
        token: request.auth_token,
        saleor_api_url: request.saleor_api_url,
        app_id: env!("CARGO_PKG_NAME").to_string(),
        jwks: Some(jwks),
    };
    let insert = session.insert(AplId::from(&auth_data).as_ref(), auth_data);
    if let Err(e) = insert {
        return SaleorRegisterResponse::custom("APL_UNAVAILABLE", &format!("Unable to set session status: {}", e), StatusCode::INTERNAL_SERVER_ERROR);
    }

    SaleorRegisterResponse::success()
}
