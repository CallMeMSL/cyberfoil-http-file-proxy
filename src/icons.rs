use std::time::Duration;

use futures_util::{
    StreamExt,
    future::{Either, select},
};
use worker::{
    AbortController, AbortSignal, Cache, CacheMode, Context, Delay, Fetch, Headers, Request,
    RequestInit, RequestRedirect, Response,
};

const MAX_ICON_BYTES: usize = 2 * 1024 * 1024;
const ICON_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, thiserror::Error)]
enum IconError {
    #[error("Icon not found")]
    NotFound,
    #[error("Icon source is unavailable")]
    Upstream,
    #[error("Invalid image from icon source")]
    InvalidImage,
    #[error("Icon exceeds the 2 MiB limit")]
    TooLarge,
    #[error("Icon request timed out")]
    Timeout,
}

impl IconError {
    fn status(&self) -> u16 {
        match self {
            Self::NotFound => 404,
            Self::Timeout => 504,
            Self::Upstream | Self::InvalidImage | Self::TooLarge => 502,
        }
    }
}

pub(crate) async fn serve(
    request: &Request,
    raw_title_id: &str,
    context: &Context,
) -> worker::Result<Response> {
    let Some(title_id) = parse_title_id(raw_title_id) else {
        return crate::error_response("Not found", 404);
    };
    let (primary_id, fallback_id) = image_ids(title_id);
    let cache_key = format!(
        "{}/api/shop/icon/{primary_id:016X}",
        request.url()?.origin().ascii_serialization()
    );
    let cache = Cache::default();
    if let Ok(Some(response)) = cache.get(cache_key.as_str(), false).await {
        return Ok(response);
    }

    let controller = AbortController::default();
    let signal = controller.signal();
    let operation = async {
        if let Some(body) = fetch_image(primary_id, &signal).await? {
            return Ok(body);
        }
        if let Some(fallback_id) = fallback_id
            && let Some(body) = fetch_image(fallback_id, &signal).await?
        {
            return Ok(body);
        }
        Err(IconError::NotFound)
    };
    let result = match select(Box::pin(operation), Box::pin(Delay::from(ICON_TIMEOUT))).await {
        Either::Left((result, _timer)) => result,
        Either::Right(((), _operation)) => Err(IconError::Timeout),
    };
    controller.abort();
    let (body, content_type) = match result {
        Ok(image) => image,
        Err(error) => return crate::error_response(&error.to_string(), error.status()),
    };

    let length = body.len();
    let mut response = Response::from_bytes(body)?;
    response.headers_mut().set("Content-Type", content_type)?;
    response
        .headers_mut()
        .set("Content-Length", &length.to_string())?;
    response
        .headers_mut()
        .set("Cache-Control", "public, max-age=86400")?;
    response
        .headers_mut()
        .set("X-Content-Type-Options", "nosniff")?;
    response
        .headers_mut()
        .set("Referrer-Policy", "no-referrer")?;
    if let Ok(cached_response) = response.cloned() {
        context.wait_until(async move {
            let _ = cache.put(cache_key, cached_response).await;
        });
    }
    Ok(response)
}

async fn fetch_image(
    title_id: u64,
    signal: &AbortSignal,
) -> Result<Option<(Vec<u8>, &'static str)>, IconError> {
    let headers = Headers::new();
    headers
        .set("Accept", "image/jpeg, image/png, image/webp")
        .map_err(|_| IconError::Upstream)?;
    let mut init = RequestInit::new();
    init.with_headers(headers)
        .with_redirect(RequestRedirect::Manual)
        .with_cache(CacheMode::NoStore);
    let request = Request::new_with_init(
        &format!("https://tinfoil.media/thi/{title_id:016X}/0/0/"),
        &init,
    )
    .map_err(|_| IconError::Upstream)?;
    let mut response = Fetch::Request(request)
        .send_with_signal(signal)
        .await
        .map_err(|_| IconError::Upstream)?;
    match response.status_code() {
        200 => {}
        404 | 500 => return Ok(None),
        _ => return Err(IconError::Upstream),
    }
    let content_type = response
        .headers()
        .get("Content-Type")
        .map_err(|_| IconError::InvalidImage)?
        .ok_or(IconError::InvalidImage)?;
    let media_type = content_type.split(';').next().unwrap_or_default().trim();
    if !["image/jpeg", "image/png", "image/webp"]
        .iter()
        .any(|allowed| media_type.eq_ignore_ascii_case(allowed))
    {
        return Err(IconError::InvalidImage);
    }
    let length = response
        .headers()
        .get("Content-Length")
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u64>().ok());
    if length.is_some_and(|size| size > MAX_ICON_BYTES as u64) {
        return Err(IconError::TooLarge);
    }
    let mut body = Vec::with_capacity(length.unwrap_or(0) as usize);
    let mut stream = response.stream().map_err(|_| IconError::InvalidImage)?;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| IconError::InvalidImage)?;
        if chunk.len() > MAX_ICON_BYTES - body.len() {
            return Err(IconError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    let detected_type = image_content_type(&body).ok_or(IconError::InvalidImage)?;
    if !media_type.eq_ignore_ascii_case(detected_type) {
        return Err(IconError::InvalidImage);
    }
    Ok(Some((body, detected_type)))
}

fn parse_title_id(value: &str) -> Option<u64> {
    if value.len() != 16 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(value, 16).ok()
}

fn image_ids(title_id: u64) -> (u64, Option<u64>) {
    match title_id & 0xFFF {
        0 => (title_id, None),
        0x800 => (title_id ^ 0x800, None),
        _ => (title_id, Some((title_id ^ 0x1000) & !0xFFF)),
    }
}

fn image_content_type(body: &[u8]) -> Option<&'static str> {
    if body.starts_with(&[0xFF, 0xD8, 0xFF]) && body.ends_with(&[0xFF, 0xD9]) {
        Some("image/jpeg")
    } else if body.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if body.starts_with(b"RIFF") && body.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_ids_require_exactly_sixteen_hex_characters() {
        assert_eq!(parse_title_id("010055d009f78000"), Some(0x010055D009F78000));
        for value in [
            "",
            "010055D009F7800",
            "010055D009F780000",
            "010055D009F7800G",
            "010055D009F78000/",
            "../010055D009F78000",
            "+10055D009F78000",
            "https://other.example",
        ] {
            assert_eq!(parse_title_id(value), None, "{value}");
        }
    }

    #[test]
    fn updates_use_base_icons_and_dlcs_have_one_base_fallback() {
        assert_eq!(image_ids(0x01006F8002326000), (0x01006F8002326000, None));
        assert_eq!(image_ids(0x01006F8002326800), (0x01006F8002326000, None));
        assert_eq!(
            image_ids(0x01006F80023273E8),
            (0x01006F80023273E8, Some(0x01006F8002326000))
        );
        assert_eq!(
            image_ids(0x010055D009F79004),
            (0x010055D009F79004, Some(0x010055D009F78000))
        );
    }

    #[test]
    fn image_signatures_reject_html_svg_and_truncated_jpeg() {
        for body in [
            b"".as_slice(),
            b"<html>failure</html>",
            b"<svg></svg>",
            &[0xFF, 0xD8, 0xFF],
        ] {
            assert_eq!(image_content_type(body), None);
        }
        assert_eq!(
            image_content_type(&[0xFF, 0xD8, 0xFF, 0xE0, 0xFF, 0xD9]),
            Some("image/jpeg")
        );
        assert_eq!(image_content_type(b"\x89PNG\r\n\x1a\n"), Some("image/png"));
        assert_eq!(image_content_type(b"RIFF....WEBP"), Some("image/webp"));
    }
}
