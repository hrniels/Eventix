use anyhow::Context;
use askama::Template;
use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use std::fmt;
use std::sync::Arc;

use crate::html::filters;
use crate::locale::{self, Locale};
use crate::pages::Page;

#[derive(Template)]
#[template(path = "error.htm")]
struct ErrorTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    msg: &'a str,
    trace: Vec<String>,
}

#[derive(Debug)]
pub struct HTMLError {
    inner: anyhow::Error,
}

impl fmt::Display for HTMLError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl IntoResponse for HTMLError {
    fn into_response(self) -> Response {
        let html = ErrorTemplate {
            page: Page::default(),
            locale: locale::default(),
            msg: &self.inner.to_string(),
            trace: self.inner.chain().skip(1).map(|e| e.to_string()).collect(),
        }
        .render()
        .context("error template")
        .unwrap();

        (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/html")],
            html,
        )
            .into_response()
    }
}

impl From<anyhow::Error> for HTMLError {
    fn from(err: anyhow::Error) -> HTMLError {
        HTMLError { inner: err }
    }
}
