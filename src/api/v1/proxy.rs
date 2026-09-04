use axum::extract::Query;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct ImageProxyParams {
    /// External image URL to download and proxy
    #[param(example = "https://community.cloudflare.steamstatic.com/economy/image/example.png")]
    pub url: String,
}

static HTTP_CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default()
});

#[utoipa::path(
    get,
    path = "/api/v1/proxy/image",
    tag = "Proxy",
    summary = "Proxy and cache external images",
    description = "Proxies external image resources to bypass CORS restrictions (preventing canvas taints). Returns aggressive caching headers to minimize upstream requests.",
    params(
        ImageProxyParams
    ),
    responses(
        (status = 200, description = "The proxied image bytes.", content_type = "image/png"),
        (status = 400, description = "Failed to fetch the source image, invalid URL, or server returned non-success status code."),
        (status = 500, description = "Failed to read or process image bytes on the proxy side.")
    ),
    security()
)]
pub async fn image_proxy(Query(params): Query<ImageProxyParams>) -> impl IntoResponse {
    if !params.url.starts_with("http://") && !params.url.starts_with("https://") {
        return (StatusCode::BAD_REQUEST, "URL must start with http:// or https://").into_response();
    }

    let res = match HTTP_CLIENT.get(&params.url).send().await {
        Ok(r) => r,
        Err(_) => return (StatusCode::BAD_REQUEST, "Failed to download image").into_response(),
    };

    if !res.status().is_success() {
        return (StatusCode::BAD_REQUEST, "Source server returned an error").into_response();
    }

    let content_type = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("image/png")
        .to_string();

    let bytes = match res.bytes().await {
        Ok(b) => b,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read image bytes").into_response(),
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        content_type.parse().unwrap_or(HeaderValue::from_static("image/png")),
    );

    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));

    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );

    (headers, bytes).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_proxy_rejects_non_http_urls() {
        let response = image_proxy(Query(ImageProxyParams {
            url: "ftp://example.com/test.png".to_string(),
        }))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response_file = image_proxy(Query(ImageProxyParams {
            url: "file:///etc/passwd".to_string(),
        }))
        .await
        .into_response();

        assert_eq!(response_file.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_proxy_returns_bad_request_on_unreachable_host() {
        let response = image_proxy(Query(ImageProxyParams {
            url: "http://127.0.0.1:9/non_existent.png".to_string(),
        }))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
