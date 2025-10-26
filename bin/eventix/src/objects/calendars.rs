use eventix_state::{CalendarSettings, State};

pub struct Calendar {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub sync_error: Option<String>,
    pub fgcolor: String,
    pub bgcolor: String,
}

#[derive(Default)]
pub struct Calendars(pub Vec<Calendar>);

impl Calendars {
    pub fn new<F>(state: &State, filter: F) -> Self
    where
        F: Fn(&CalendarSettings) -> bool,
    {
        let mut calendars = state
            .settings()
            .calendars()
            .filter_map(|(id, settings)| {
                if filter(settings) {
                    Some(Calendar {
                        id: id.clone(),
                        name: settings.name().clone(),
                        enabled: !state.misc().calendar_disabled(id),
                        sync_error: state.misc().get_sync_error(id).cloned(),
                        fgcolor: settings.fgcolor().clone(),
                        bgcolor: settings.bgcolor().clone(),
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        calendars.sort_by(|a, b| a.name.cmp(&b.name));
        Self(calendars)
    }
}
