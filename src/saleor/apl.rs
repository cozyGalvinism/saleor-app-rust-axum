use std::{sync::Arc, future::Future, pin::Pin, time::{SystemTime, Duration}};

use async_trait::async_trait;
use axum::{http::{Request, HeaderMap, HeaderValue}, response::{Response, IntoResponse}, middleware::Next, body::Body};
use jsonwebtoken::{jwk::JwkSet, DecodingKey};
use reqwest::{StatusCode, header::{HOST, AUTHORIZATION}};
use serde::{Serialize, Deserialize};
use tower::{Layer, Service};
use tower_sessions::Session;

use super::SaleorPermission;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AuthData {
    pub domain: Option<String>,
    pub token: String,
    pub saleor_api_url: String,
    pub app_id: String,
    pub jwks: Option<String>,
}

#[derive(PartialEq, Eq, Hash)]
pub struct AplId(String);

impl AplId {
    pub fn from_auth_data(auth_data: &AuthData) -> Self {
        Self(format!("{}:{}", auth_data.app_id, auth_data.saleor_api_url))
    }

    pub fn from_api_url(api_url: &str) -> AplId {
        Self(format!("{}:{}", env!("CARGO_PKG_NAME").to_string(), api_url))
    }
}

impl From<&AuthData> for AplId {
    fn from(auth_data: &AuthData) -> Self {
        Self::from_auth_data(auth_data)
    }
}

impl AsRef<str> for AplId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

fn get_base_url(headers: &HeaderMap<HeaderValue>) -> Option<String> {
    let Some(host) = headers.get(HOST) else {
        return None;
    };
    let forwarded_proto = headers.get("x-forwarded-proto").map(|h| h.to_str().unwrap()).unwrap_or("http");

    Some(format!("{}://{}", forwarded_proto, host.to_str().unwrap()))
}

#[derive(Deserialize, Debug)]
struct Claims {
    app: String,
    user_permissions: Vec<SaleorPermission>,
}

fn verify_jwt(auth_data: &AuthData, token: &str, required_permissions: &[SaleorPermission]) -> Result<(), String> {
    let jwks = serde_json::from_str::<'_, JwkSet>(auth_data.jwks.as_ref().unwrap())
        .map_err(|e| format!("unable to deserialize jwks: {}", e))?;
    let header = jsonwebtoken::decode_header(token).map_err(|e| format!("unable to decode jwt header: {}", e))?;
    let kid = match header.kid {
        Some(kid) => kid,
        None => return Err("missing kid in jwt header".to_string()),
    };
    let jwk = jwks.find(&kid).ok_or_else(|| format!("unable to find jwk with kid {}", kid))?;
    let validation = jsonwebtoken::Validation::new(header.alg);
    let Ok(token) = jsonwebtoken::decode::<Claims>(token, &DecodingKey::from_jwk(jwk).map_err(|e| format!("unable to create decoding key from jwk: {}", e))?, &validation) else {
        return Err("unable to decode jwt".to_string());
    };
    
    if required_permissions.is_empty() {
        return Ok(());
    }

    if token.claims.user_permissions.is_empty() {
        return Err("missing user permissions".to_string());
    }

    for required_permission in required_permissions {
        if !token.claims.user_permissions.contains(required_permission) {
            return Err(format!("missing required permission {:?}", required_permission));
        }
    }

    Ok(())
}

#[derive(Clone)]
pub struct SaleorAuthLayer {
    required_permissions: Vec<SaleorPermission>,
}

impl SaleorAuthLayer {
    pub fn with_permissions(required_permissions: &[SaleorPermission]) -> Self {
        Self {
            required_permissions: required_permissions.to_vec(),
        }
    }
}

impl<S> Layer<S> for SaleorAuthLayer {
    type Service = SaleorAuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SaleorAuthMiddleware {
            inner,
            required_permissions: self.required_permissions.clone(),
        }
    }
}

#[derive(Clone)]
pub struct SaleorAuthMiddleware<S> {
    inner: S,
    required_permissions: Vec<SaleorPermission>,
}

impl<S> Service<Request<Body>> for SaleorAuthMiddleware<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send +'static>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let required_permissions = self.required_permissions.clone();
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        Box::pin(async move {
            let Some(session) = request.extensions().get::<Session>() else {
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, "tower-sessions not loaded!").into_response());
            };
        
            let Some(_) = get_base_url(request.headers()) else {
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, "missing host header").into_response());
            };
        
            let Some(api_url) = request.headers().get("saleor-api-url") else {
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, "missing saleor-api-url header").into_response());
            };
        
            let Some(token) = request.headers().get(AUTHORIZATION) else {
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, "missing authorization header").into_response());
            };
            let bearer_token = token.to_str().unwrap().replace("Bearer ", "");
            let Ok(auth_data) = session.get::<AuthData>(AplId::from_api_url(api_url.to_str().unwrap()).as_ref()) else {
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, "unable to deserialize authentication data").into_response());
            };
            let Some(auth_data) = auth_data else {
                return Ok((StatusCode::UNAUTHORIZED, "authentication data not found").into_response());
            };
        
            if let Err(e) = verify_jwt(&auth_data, &bearer_token, &required_permissions) {
                return Ok((StatusCode::UNAUTHORIZED, e).into_response());
            }

            let response: Response = inner.call(request).await?;
            Ok(response)
        })
    }
}
