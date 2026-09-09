use std::time::Duration;

use futures_util::{
    StreamExt,
    future::{Either, select},
};
use worker::{
    AbortController, CacheMode, Delay, Fetch, Headers, Request, RequestInit, RequestRedirect,
};

use crate::{
    ShopError,
    auth::Credentials,
    catalog::{Catalog, CatalogError, FolderResponse},
};

const MAX_LISTING_BYTES: usize = 4 * 1024 * 1024;
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) async fn list_folder(credentials: &Credentials) -> Result<Catalog, ShopError> {
    let url = credentials.folder_url()?;
    let headers = Headers::new();
    headers
        .set("Authorization", &format!("Bearer {}", credentials.api_key))
        .map_err(|_| ShopError::Unauthorized)?;
    headers
        .set("Accept", "application/json")
        .map_err(|_| ShopError::Upstream)?;
    let mut init = RequestInit::new();
    init.with_headers(headers)
        .with_redirect(RequestRedirect::Manual)
        .with_cache(CacheMode::NoStore);
    let request = Request::new_with_init(url.as_str(), &init).map_err(|_| ShopError::Upstream)?;
    let controller = AbortController::default();
    let signal = controller.signal();
    let operation = async {
        let mut response = Fetch::Request(request)
            .send_with_signal(&signal)
            .await
            .map_err(|_| ShopError::Upstream)?;
        let retry_after = response
            .headers()
            .get("Retry-After")
            .ok()
            .flatten()
            .filter(|value| valid_retry_after(value));
        if response.status_code() != 200 {
            return Err(http_error(response.status_code(), retry_after));
        }
        let length = response
            .headers()
            .get("Content-Length")
            .ok()
            .flatten()
            .and_then(|value| value.parse::<usize>().ok());
        if length.is_some_and(|size| size > MAX_LISTING_BYTES) {
            return Err(ShopError::TooLarge);
        }
        let mut body = Vec::with_capacity(length.unwrap_or(0));
        let mut stream = response.stream().map_err(|_| ShopError::Upstream)?;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ShopError::Upstream)?;
            if chunk.len() > MAX_LISTING_BYTES - body.len() {
                return Err(ShopError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let listing: FolderResponse =
            serde_json::from_slice(&body).map_err(|_| ShopError::Upstream)?;
        listing
            .into_catalog(&credentials.folder)
            .map_err(|error| match error {
                CatalogError::InvalidItem => ShopError::Upstream,
                CatalogError::Upstream(code) => api_error(&code, retry_after),
            })
    };
    let result = match select(Box::pin(operation), Box::pin(Delay::from(UPSTREAM_TIMEOUT))).await {
        Either::Left((result, _timer)) => result,
        Either::Right(((), _operation)) => Err(ShopError::Timeout),
    };
    controller.abort();
    result
}

fn api_error(code: &str, retry_after: Option<String>) -> ShopError {
    match code {
        "authentication_failed" => ShopError::Unauthorized,
        "permission_denied" => ShopError::Forbidden,
        "not_found" => ShopError::NotFound,
        "invalid_request" => ShopError::InvalidFolder,
        "rate_limit_reached" | "account_limit_reached" | "service_limit_reached" => {
            ShopError::RateLimited(retry_after)
        }
        "service_down" | "semi_permanent_error" => ShopError::Unavailable(retry_after),
        _ => ShopError::Upstream,
    }
}

fn http_error(status: u16, retry_after: Option<String>) -> ShopError {
    match status {
        401 => ShopError::Unauthorized,
        403 => ShopError::Forbidden,
        404 => ShopError::NotFound,
        429 => ShopError::RateLimited(retry_after),
        503 => ShopError::Unavailable(retry_after),
        408 | 504 => ShopError::Timeout,
        _ => ShopError::Upstream,
    }
}

fn valid_retry_after(value: &str) -> bool {
    (!value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok())
        || httpdate::parse_http_date(value).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_errors_have_safe_http_statuses() {
        for (code, expected) in [
            ("authentication_failed", 401),
            ("permission_denied", 403),
            ("not_found", 404),
            ("invalid_request", 400),
            ("rate_limit_reached", 429),
            ("account_limit_reached", 429),
            ("service_limit_reached", 429),
            ("service_down", 503),
            ("semi_permanent_error", 503),
            ("transient_error", 502),
            ("link_generation_failed", 502),
            ("unknown", 502),
        ] {
            assert_eq!(api_error(code, None).status(), expected, "{code}");
        }
    }

    #[test]
    fn http_errors_do_not_follow_redirects_or_expose_upstream_bodies() {
        for (status, expected) in [
            (301, 502),
            (302, 502),
            (401, 401),
            (403, 403),
            (404, 404),
            (429, 429),
            (500, 502),
            (503, 503),
            (408, 504),
            (504, 504),
        ] {
            assert_eq!(http_error(status, None).status(), expected);
        }
    }

    #[test]
    fn retry_after_accepts_only_seconds_or_http_dates() {
        for value in ["0", "120", "Wed, 09 Sep 2026 12:00:00 GMT"] {
            assert!(valid_retry_after(value), "{value}");
        }
        for value in [
            "",
            "-1",
            "+1",
            "tomorrow",
            "secret",
            "999999999999999999999999",
        ] {
            assert!(!valid_retry_after(value), "{value}");
        }
    }
}
