use std::{sync::Arc, future::Future, pin::Pin, time::{SystemTime, Duration}, ops::Deref};

use async_trait::async_trait;
use axum::{http::{Request, HeaderMap, HeaderValue, request::Parts}, response::{Response, IntoResponse}, middleware::Next, body::Body, extract::FromRequestParts};
use jsonwebtoken::{jwk::JwkSet, DecodingKey};
use reqwest::{StatusCode, header::{HOST, AUTHORIZATION}};
use serde::{Serialize, Deserialize};
use tower::{Layer, Service};
use tower_sessions::Session;

use super::SaleorPermission;

mod file;

pub use file::FileAplStore;

#[async_trait]
pub trait AplStore: Send + Sync + 'static {
    async fn get(&self, apl_id: &AplId) -> Option<AuthData>;
    async fn set(&self, apl_id: &AplId, auth_data: AuthData);
    async fn remove(&self, apl_id: &AplId);
}

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
        Self(format!("{}:{}", crate::APP_ID, api_url))
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

pub fn verify_jwt(jwks: &str, token: &str, required_permissions: &[SaleorPermission]) -> Result<(), String> {
    let jwks = serde_json::from_str::<'_, JwkSet>(jwks)
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
pub struct SaleorApl {
    inner: Arc<dyn AplStore>,
}

impl Deref for SaleorApl {
    type Target = Arc<dyn AplStore>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for SaleorApl
where
    S: Sync + Send,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<SaleorApl>().cloned().ok_or((StatusCode::INTERNAL_SERVER_ERROR, "apl store not found in request extensions").into_response())
    }
}

#[derive(Clone)]
pub struct SaleorAplService<S> {
    inner: S,
    apl_store: Arc<dyn AplStore>,
}

impl<S> Service<Request<Body>> for SaleorAplService<S>
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

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let apl_store = self.apl_store.clone();
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        Box::pin(async move {
            let extensions = req.extensions_mut();
            let already_set = extensions.get::<SaleorApl>().is_some();
            if !already_set {
                extensions.insert(SaleorApl { inner: apl_store.clone() });
            }

            let response: Response = inner.call(req).await?;
            Ok(response)
        })
    }
}

#[derive(Clone)]
pub struct SaleorAplLayer {
    apl_store: Arc<dyn AplStore>,
}

impl SaleorAplLayer {
    pub fn new(apl_store: impl AplStore) -> Self {
        Self { apl_store: Arc::new(apl_store) }
    }
}

impl<S> Layer<S> for SaleorAplLayer {
    type Service = SaleorAplService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SaleorAplService {
            inner,
            apl_store: self.apl_store.clone(),
        }
    }
}

#[derive(Clone)]
pub struct SaleorAuthLayer {
    required_permissions: Vec<SaleorPermission>,
}

impl SaleorAuthLayer {
    pub fn with_permissions(permissions: &[SaleorPermission]) -> Self {
        Self {
            required_permissions: permissions.to_vec(),
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
            let apl_store = request
                .extensions()
                .get::<SaleorApl>()
                .cloned()
                .expect("apl store not found in request extensions");
            let session = request
                .extensions()
                .get::<Session>()
                .cloned()
                .expect("tower-session not found in request extensions");

            let Some(_) = get_base_url(request.headers()) else {
                return Ok((StatusCode::BAD_REQUEST, "missing host header").into_response());
            };
        
            let api_url = match request.headers().get("saleor-api-url") {
                Some(api_url) => api_url.to_str().unwrap().to_string(),
                None => {
                    let Ok(Some(api_url)) = session.get::<String>("saleor_api_url") else {
                        return Ok((StatusCode::BAD_REQUEST, "couldn't determine saleor api url").into_response());
                    };
        
                    api_url
                }
            };

            let jwks = match apl_store.get(&AplId::from_api_url(&api_url)).await {
                Some(auth_data) => {
                    match auth_data.jwks {
                        Some(jwks) => jwks,
                        None => {
                            let jwks_url = format!("{}/.well-known/jwks.json", &api_url);
                            reqwest::get(&jwks_url).await.unwrap().text().await.unwrap()
                        }
                    }
                },
                None => {
                    let jwks_url = format!("{}/.well-known/jwks.json", &api_url);
                    reqwest::get(&jwks_url).await.unwrap().text().await.unwrap()
                }
            };
        
            let token = match request.headers().get(AUTHORIZATION) {
                Some(token) => token.to_str().unwrap().replace("Bearer ", ""),
                None => {
                    let Ok(Some(token)) = session.get::<String>("token") else {
                        return Ok((StatusCode::BAD_REQUEST, "couldn't determine token").into_response());
                    };
        
                    token
                }
            };
        
            if let Err(e) = verify_jwt(&jwks, &token, &required_permissions) {
                return Ok((StatusCode::UNAUTHORIZED, e).into_response());
            }

            let response: Response = inner.call(request).await?;
            Ok(response)
        })
    }
}
