//! Embedded browser resources. Runtime users do not need Node.js or source files.
use crate::app_state::SharedState;
use axum::{
    Router,
    body::Body,
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/remote_web_assets.rs"));
}

pub(crate) fn entry_script() -> Option<&'static str> {
    embedded::ASSETS.iter().find_map(|(name, _)| {
        (name.starts_with("assets/index-") && name.ends_with(".js")).then_some(*name)
    })
}
pub(crate) fn router() -> Router<SharedState> {
    Router::new()
        .route("/remote", get(|| async { Redirect::temporary("/remote/") }))
        .route("/remote/", get(index))
        .route("/remote/{*path}", get(resource))
}

async fn index() -> Response {
    serve("index.html")
}

async fn resource(Path(path): Path<String>) -> Response {
    serve(&path)
}

fn serve(path: &str) -> Response {
    let Some((_, bytes)) = embedded::ASSETS.iter().find(|(name, _)| *name == path) else {
        return (
            StatusCode::NOT_FOUND,
            "远程网页资源不存在，请使用完整的 Hub 构建",
        )
            .into_response();
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut response = Response::new(Body::from(*bytes));
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, mime.as_ref().parse().unwrap());
    headers.insert(
        header::CACHE_CONTROL,
        if path == "index.html" {
            "no-store"
        } else {
            "no-cache"
        }
        .parse()
        .unwrap(),
    );
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    headers.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    headers.insert(header::CONTENT_SECURITY_POLICY,
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' blob: data:; connect-src 'self'; font-src 'self'; frame-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'".parse().unwrap());
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_entry_and_assets_are_available_without_node() {
        assert_eq!(serve("index.html").status(), StatusCode::OK);
        assert!(embedded::ASSETS.iter().any(|(name, bytes)| {
            name.starts_with("assets/") && name.ends_with(".js") && !bytes.is_empty()
        }));
        assert_eq!(
            serve("index.html").headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff"
        );
        assert_eq!(
            serve("index.html").headers()[header::CACHE_CONTROL],
            "no-store"
        );
        let (_, html) = embedded::ASSETS
            .iter()
            .find(|(name, _)| *name == "index.html")
            .unwrap();
        assert!(
            std::str::from_utf8(html)
                .unwrap()
                .contains(entry_script().unwrap())
        );
    }

    #[test]
    fn embedded_paths_do_not_allow_host_file_reads() {
        assert_eq!(serve("../../Cargo.toml").status(), StatusCode::NOT_FOUND);
        assert_eq!(serve("/index.html").status(), StatusCode::NOT_FOUND);
    }
}
