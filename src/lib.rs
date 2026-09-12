mod auth;
mod catalog;
mod icons;
mod premiumize;

use serde::Serialize;
use worker::{Context, Env, Method, Request, Response, Result, event};

use auth::Credentials;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ShopError {
    #[error("Invalid credentials")]
    Unauthorized,
    #[error("Invalid folder path")]
    InvalidFolder,
    #[error("Permission denied")]
    Forbidden,
    #[error("Folder not found")]
    NotFound,
    #[error("Premiumize request or account limit reached")]
    RateLimited(Option<String>),
    #[error("Premiumize is unavailable")]
    Unavailable(Option<String>),
    #[error("Invalid response from Premiumize")]
    Upstream,
    #[error("Premiumize request timed out")]
    Timeout,
    #[error("Folder listing exceeds the 4 MiB limit; select a smaller folder")]
    TooLarge,
}

impl ShopError {
    fn status(&self) -> u16 {
        match self {
            Self::Unauthorized => 401,
            Self::InvalidFolder => 400,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::RateLimited(_) => 429,
            Self::Unavailable(_) => 503,
            Self::Upstream | Self::TooLarge => 502,
            Self::Timeout => 504,
        }
    }

    fn response(&self) -> Result<Response> {
        let mut response = error_response(&self.to_string(), self.status())?;
        if matches!(self, Self::Unauthorized) {
            response.headers_mut().set(
                "WWW-Authenticate",
                "Basic realm=\"Premiumize shop\", charset=\"UTF-8\"",
            )?;
        }
        if let Self::RateLimited(Some(retry_after)) | Self::Unavailable(Some(retry_after)) = self {
            response.headers_mut().set("Retry-After", retry_after)?;
        }
        Ok(response)
    }
}

#[event(fetch)]
async fn fetch(request: Request, _env: Env, context: Context) -> Result<Response> {
    let path = request.path();
    let icon_id = path.strip_prefix("/api/shop/icon/");
    if !matches!(path.as_str(), "/" | "/api/shop/sections") && icon_id.is_none() {
        return error_response("Not found", 404);
    }
    if request.method() != Method::Get {
        let mut response = error_response("Method not allowed", 405)?;
        response.headers_mut().set("Allow", "GET")?;
        return Ok(response);
    }
    if let Some(icon_id) = icon_id {
        return icons::serve(&request, icon_id, &context).await;
    }
    match serve_catalog(&request).await {
        Ok(catalog) => json_response(&catalog, 200),
        Err(error) => error.response(),
    }
}

async fn serve_catalog(request: &Request) -> std::result::Result<catalog::Catalog, ShopError> {
    let header = request
        .headers()
        .get("Authorization")
        .map_err(|_| ShopError::Unauthorized)?;
    let credentials = Credentials::parse(header.as_deref())?;
    premiumize::list_folder(&credentials).await
}

fn json_response(value: &impl Serialize, status: u16) -> Result<Response> {
    let body = serde_json::to_string(value)
        .map_err(|_| worker::Error::from("JSON serialization failed"))?;
    let mut response = Response::ok(body)?.with_status(status);
    response
        .headers_mut()
        .set("Content-Type", "application/json; charset=utf-8")?;
    response
        .headers_mut()
        .set("Cache-Control", "private, no-store")?;
    response
        .headers_mut()
        .set("X-Content-Type-Options", "nosniff")?;
    response
        .headers_mut()
        .set("Referrer-Policy", "no-referrer")?;
    Ok(response)
}

fn error_response(message: &str, status: u16) -> Result<Response> {
    #[derive(Serialize)]
    struct ErrorBody<'message> {
        error: &'message str,
    }
    json_response(&ErrorBody { error: message }, status)
}
