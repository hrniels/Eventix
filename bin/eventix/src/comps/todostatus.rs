use std::sync::Arc;

use askama::Template;
use ical::{
    col::Occurrence,
    objects::{CalCompType, CalTodoStatus},
};
use serde::{Deserialize, Serialize};

use crate::locale::Locale;

use super::{
    combobox::{ComboOption, ComboboxTemplate},
    date::{Date, DateTemplate},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TodoStatus {
    status: CalTodoStatus,
    percent: Option<u8>,
    completed: Option<Date>,
}

impl Default for TodoStatus {
    fn default() -> Self {
        Self {
            status: CalTodoStatus::NeedsAction,
            percent: None,
            completed: None,
        }
    }
}

impl TodoStatus {
    pub fn new_from_occurrence(occ: &Occurrence<'_>) -> Option<Self> {
        if occ.ctype() != CalCompType::Todo {
            return None;
        }

        Some(Self {
            status: occ.todo_status().unwrap_or(CalTodoStatus::NeedsAction),
            percent: occ.todo_percent(),
            completed: occ
                .todo_completed()
                .map(|d| Date::new(Some(d.as_naive_date()))),
        })
    }

    pub fn status(&self) -> CalTodoStatus {
        self.status
    }

    pub fn percent(&self) -> Option<u8> {
        self.percent
    }

    pub fn completed(&self) -> Option<&Date> {
        self.completed.as_ref()
    }
}

#[derive(Template)]
#[template(path = "comps/todostatus.htm")]
pub struct TodoStatusTemplate {
    name: String,
    id: String,
    status: ComboboxTemplate<CalTodoStatus>,
    percent: Option<u8>,
    completed: DateTemplate,
}

impl TodoStatusTemplate {
    pub fn new<N: ToString>(
        locale: Arc<dyn Locale + Send + Sync>,
        name: N,
        value: TodoStatus,
    ) -> Self {
        let name = name.to_string();
        Self {
            id: name.replace("[", "_").replace("]", "_"),
            status: ComboboxTemplate::new_with_options(
                locale.clone(),
                format!("{}[status]", name),
                Some(value.status),
                vec![
                    ComboOption::new(locale.translate("Needs action"), CalTodoStatus::NeedsAction),
                    ComboOption::new(locale.translate("Completed"), CalTodoStatus::Completed),
                    ComboOption::new(locale.translate("In progress"), CalTodoStatus::InProcess),
                    ComboOption::new(locale.translate("Cancelled"), CalTodoStatus::Cancelled),
                ],
            ),
            percent: value.percent,
            completed: DateTemplate::new(format!("{}[completed]", name), value.completed),
            name,
        }
    }
}
