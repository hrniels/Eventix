use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use chrono::Utc;
use eventix_locale::Locale;
use eventix_state::EventixState;
use once_cell::sync::Lazy;
use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

use crate::html::filters;

struct CachedBundle {
    mime_type: String,
    data: Vec<u8>,
}

struct CachedBundles {
    last_mod: String,
    bundles: HashMap<String, CachedBundle>,
}

#[derive(Template)]
#[template(path = "locale.none")]
struct LocaleTemplate {
    locale: Arc<dyn Locale + Send + Sync>,
}

static NOCACHE: Lazy<String> = Lazy::new(|| String::from("no-cache"));
static BUNDLES: Lazy<Mutex<Option<CachedBundles>>> = Lazy::new(|| Mutex::new(None));

fn build_bundle(path: &PathBuf, suffix: &str, without: &str) -> CachedBundle {
    let mut files: Vec<PathBuf> = fs::read_dir(path)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .and_then(|e| e.to_str())
                .map(|s| s.ends_with(suffix) && (without.is_empty() || !s.contains(without)))
                .unwrap_or(false)
        })
        .collect();

    // deterministic order!
    files.sort();

    let mime_type = if suffix.ends_with("js") {
        "application/javascript"
    } else {
        "text/css"
    };

    let mut data = Vec::new();
    for path in files {
        data.extend_from_slice(b"\n/* ---- ");
        data.extend_from_slice(path.file_name().unwrap().to_string_lossy().as_bytes());
        data.extend_from_slice(b" ---- */\n\n");

        let file_data = fs::read(&path).unwrap();
        data.extend_from_slice(&file_data);
        data.extend_from_slice(b"\n\n");
    }

    CachedBundle {
        mime_type: mime_type.to_string(),
        data,
    }
}

fn build_js_locale(locale: Arc<dyn Locale + Send + Sync>) -> CachedBundle {
    let html = LocaleTemplate { locale }.render().unwrap();

    CachedBundle {
        mime_type: "application/javascript".to_string(),
        data: html.into_bytes(),
    }
}

fn build_all_bundles(locale: Arc<dyn Locale + Send + Sync>, static_path: PathBuf) -> CachedBundles {
    let mut res = CachedBundles {
        last_mod: Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
        bundles: HashMap::new(),
    };

    let contrib_path = static_path.join("contrib");
    res.bundles
        .insert("bundle.js".into(), build_bundle(&static_path, ".js", ""));
    res.bundles
        .insert("bundle.css".into(), build_bundle(&contrib_path, ".css", ""));
    res.bundles.insert(
        "contrib.js".into(),
        build_bundle(&contrib_path, ".js", ".min"),
    );
    res.bundles.insert(
        "contrib.min.js".into(),
        build_bundle(&contrib_path, ".min.js", ""),
    );
    res.bundles.insert(
        "contrib.css".into(),
        build_bundle(&contrib_path, ".css", ".min"),
    );
    res.bundles.insert(
        "contrib.min.css".into(),
        build_bundle(&contrib_path, ".min.css", ""),
    );
    res.bundles
        .insert("locale.js".into(), build_js_locale(locale));

    res
}

async fn bundle(
    State((state, static_path)): State<(EventixState, PathBuf)>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let locale = state.lock().await.locale();
    let mut cached = BUNDLES.lock().await;
    if cached.is_none() {
        *cached = Some(build_all_bundles(locale, static_path));
    }

    // if the browser has the file already, reply 304
    let last_mod = &cached.as_ref().unwrap().last_mod;
    if let Some(if_modified_since) = headers.get("if-modified-since")
        && if_modified_since == last_mod
    {
        return (StatusCode::NOT_MODIFIED, headers, Vec::<u8>::new());
    }

    let bundle = cached.as_ref().unwrap().bundles.get(&name).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&bundle.mime_type).unwrap(),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_str(&NOCACHE).unwrap(),
    );
    // tell the browser when we generated it so that it sends us if-modified-since next time
    headers.insert(
        header::LAST_MODIFIED,
        HeaderValue::from_str(last_mod).unwrap(),
    );
    (StatusCode::OK, headers, bundle.data.clone())
}

pub async fn invalidate() {
    BUNDLES.lock().await.take();
}

pub fn router(state: EventixState, static_path: PathBuf) -> Router {
    Router::new()
        .route("/{name}", get(bundle))
        .with_state((state, static_path))
}
