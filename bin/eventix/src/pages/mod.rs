pub mod details;
pub mod monthly;

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Breadcrumb {
    pub url: String,
    pub name: String,
}

impl Breadcrumb {
    #[allow(dead_code)]
    pub fn new<S: ToString>(url: S, name: S) -> Self {
        Self {
            url: url.to_string(),
            name: name.to_string(),
        }
    }
}

pub struct Page {
    start: Instant,
    path: String,
    breadcrumbs: Vec<Breadcrumb>,
    errors: Vec<String>,
    infos: Vec<String>,
}

impl Page {
    pub fn new<S: ToString>(path: S) -> Self {
        Self {
            start: Instant::now(),
            path: path.to_string(),
            breadcrumbs: Vec::new(),
            errors: Vec::new(),
            infos: Vec::new(),
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn breadcrumbs(&self) -> &[Breadcrumb] {
        &self.breadcrumbs
    }

    #[allow(dead_code)]
    pub fn add_breadcrumb(&mut self, breadcrumb: Breadcrumb) {
        self.breadcrumbs.push(breadcrumb);
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    pub fn add_error<S: ToString>(&mut self, message: S) {
        self.errors.push(message.to_string());
    }

    #[allow(dead_code)]
    pub fn add_detailed_error(&mut self, error: anyhow::Error) {
        let mut msg = error.to_string();
        for m in error.chain().skip(1) {
            msg.push_str(": ");
            msg.push_str(&m.to_string());
        }
        self.add_error(msg);
    }

    pub fn infos(&self) -> &[String] {
        &self.infos
    }

    #[allow(dead_code)]
    pub fn add_info<S: ToString>(&mut self, message: S) {
        self.infos.push(message.to_string());
    }

    pub fn time_elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}
