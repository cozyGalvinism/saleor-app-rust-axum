use std::collections::HashMap;

use async_trait::async_trait;
use axum::{response::{IntoResponse, Response}, http::{StatusCode, Request, header::CONTENT_TYPE}, extract::{FromRequest, Query}, Json, body::{Body, Bytes}};
use serde::{Serialize, Deserialize};

mod enums;
mod apl;

pub use enums::*;
pub use apl::*;

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SaleorManifest {
    pub id: String,
    pub version: String,
    pub required_saleor_version: Option<String>,
    pub name: String,
    pub permissions: Vec<SaleorAppPermission>,
    pub app_url: String,
    pub token_target_url: String,
    pub author: Option<String>,
    pub about: Option<String>,
    pub data_privacy_url: Option<String>,
    pub homepage_url: Option<String>,
    pub support_url: Option<String>,
    pub extensions: Option<Vec<SaleorAppExtension>>,
    pub webhooks: Option<Vec<SaleorWebhookManifest>>,
    pub brand: Option<SaleorBrand>,
}

impl IntoResponse for SaleorManifest {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::OK, axum::Json(self)).into_response()
    }
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SaleorAppExtension {
    pub label: String,
    pub mount: SaleorAppExtensionMount,
    pub target: SaleorAppExtensionTarget,
    pub permissions: Vec<SaleorAppPermission>,
    pub url: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SaleorWebhookManifest {
    pub name: String,
    pub async_events: Option<Vec<SaleorAsyncWebhookEvent>>,
    pub sync_events: Option<Vec<SaleorSyncWebhookEvent>>,
    pub query: String,
    pub target_url: String,
    pub is_active: Option<bool>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SaleorBrand {
    pub logo: SaleorLogo,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SaleorLogo {
    pub default: String,
}

pub struct SaleorRegisterRequest {
    pub auth_token: String,
    pub saleor_domain: String,
    pub saleor_api_url: String,
}

#[derive(Deserialize, Debug)]
struct SaleorAuthToken {
    auth_token: String,
}

pub struct ExtractRegisterRequest(pub SaleorRegisterRequest);

#[async_trait]
impl<S> FromRequest<S, Body> for ExtractRegisterRequest
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request<Body>, state: &S) -> Result<Self, Self::Rejection> {
        let saleor_domain = req
            .headers()
            .get("saleor-domain")
            .map(|h| h.to_str().unwrap().to_string())
            .ok_or((StatusCode::BAD_REQUEST, "missing saleor-domain header").into_response())?;
        let saleor_api_url = req
            .headers()
            .get("saleor-api-url")
            .map(|h| h.to_str().unwrap().to_string())
            .ok_or((StatusCode::BAD_REQUEST, "missing saleor-api-url header").into_response())?;

        let query = Query::<SaleorAuthToken>::try_from_uri(req.uri());
        let auth_token = match query {
            Ok(query) => query.0.auth_token,
            Err(_) => {
                let body = Json::<SaleorAuthToken>::from_request(req, state).await.map_err(IntoResponse::into_response)?;
                body.auth_token.to_owned()
            }
        };

        Ok(ExtractRegisterRequest(SaleorRegisterRequest {
            auth_token,
            saleor_domain,
            saleor_api_url,
        }))
    }
}

#[derive(Serialize, Debug)]
pub struct SaleorRegisterResponse {
    pub success: bool,
    pub error: Option<SaleorRegisterError>,
}

impl SaleorRegisterResponse {
    pub fn success() -> Response {
        (StatusCode::OK, Json(Self {
            success: true,
            error: None,
        })).into_response()
    }

    pub fn jwks_not_available() -> Response {
        (StatusCode::UNAUTHORIZED, Json(Self {
            success: false,
            error: Some(SaleorRegisterError {
                code: "JWKS_NOT_AVAILABLE".to_string(),
                message: "JWKS not available".to_string(),
            }),
        })).into_response()
    }

    pub fn custom(code: &str, message: &str, status_code: StatusCode) -> Response {
        (status_code, Json(Self {
            success: false,
            error: Some(SaleorRegisterError {
                code: code.to_string(),
                message: message.to_string(),
            }),
        })).into_response()
    }
}

#[derive(Serialize, Debug)]
pub struct SaleorRegisterError {
    pub code: String,
    pub message: String,
}
