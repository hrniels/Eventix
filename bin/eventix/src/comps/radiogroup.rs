use std::fmt::Display;

use askama::Template;

#[derive(Template)]
#[template(path = "comps/radiogroup.htm")]
pub struct RadioGroupTemplate<V: Eq + Display> {
    name: String,
    value: V,
    elems: Vec<(V, String)>,
}

impl<V: Eq + Display> RadioGroupTemplate<V> {
    pub fn new(name: String, value: V, elems: Vec<(V, String)>) -> Self {
        Self { name, value, elems }
    }
}
