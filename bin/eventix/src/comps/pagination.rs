use std::ops::Range;

use askama::Template;

use crate::html::filters;

#[derive(Template)]
#[template(path = "comps/pagination.htm")]
pub struct PaginationTemplate<F: Fn(&usize) -> String> {
    build_url: F,
    before_range: Range<usize>,
    after_range: Range<usize>,
    page: usize,
    num: usize,
}

impl<F: Fn(&usize) -> String> PaginationTemplate<F> {
    pub fn new(build_url: F, total: usize, per_page: usize, page: usize) -> Self {
        let num = total.div_ceil(per_page);
        let page = if page > num || page < 1 { 1 } else { page };
        Self {
            build_url,
            before_range: (page.saturating_sub(5).max(2)..page),
            after_range: (page + 1)..(page + 1 + 5).min(num).max(page + 1),
            page,
            num,
        }
    }
}
