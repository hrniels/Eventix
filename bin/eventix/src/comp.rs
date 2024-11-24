use std::sync::Arc;

use ical::objects::CalCompType;
use ical::objects::{CalComponent, CalDate, UpdatableEventLike};

use crate::{
    comps::{datetimerange::DateTimeRange, recur::RecurRequest},
    locale::Locale,
    pages::Page,
};

pub trait CompAction {
    fn summary(&self) -> &String;
    fn location(&self) -> &String;
    fn description(&self) -> &String;
    fn rrule(&self) -> Option<&RecurRequest>;
    fn start_end(&self) -> &DateTimeRange;

    fn check(
        &self,
        page: &mut Page,
        locale: &Arc<dyn Locale + Send + Sync>,
        ctype: CalCompType,
    ) -> bool {
        if self.summary().is_empty() {
            page.add_error(locale.translate("Summary cannot be empty."));
            return false;
        }

        let (start, end) = self.start_end().as_caldates(locale);

        if ctype == CalCompType::Event {
            if start.is_none() {
                page.add_error(locale.translate("Please specify the start date/time."));
                return false;
            }
            if end.is_none() {
                page.add_error(locale.translate("Please specify the end date/time."));
                return false;
            }
        }

        if start.is_some()
            && end.is_some()
            && matches!(start.as_ref().unwrap(), CalDate::Date(_))
                != matches!(end.as_ref().unwrap(), CalDate::Date(_))
        {
            page.add_error(
                locale.translate("Please specify the time for both start and end or for none."),
            );
            return false;
        }

        if self.rrule().is_some() && start.is_none() {
            page.add_error(
                locale.translate("Please specify the start for repeating events/tasks."),
            );
            return false;
        }

        true
    }

    fn nonempty_or_none(val: String) -> Option<String> {
        if val.is_empty() {
            None
        } else {
            Some(val)
        }
    }

    fn update(&self, comp: &mut CalComponent, locale: &Arc<dyn Locale + Send + Sync>) {
        let (start, end) = self.start_end().as_caldates(locale);

        comp.set_summary(Self::nonempty_or_none(self.summary().clone()));
        comp.set_location(Self::nonempty_or_none(self.location().clone()));
        comp.set_description(Self::nonempty_or_none(self.description().clone()));
        comp.set_start(start);
        if let Some(ev) = comp.as_event_mut() {
            ev.set_end(end);
        } else {
            comp.as_todo_mut().unwrap().set_due(end);
        }

        comp.set_last_modified(CalDate::now());
        comp.set_stamp(CalDate::now());
    }
}
