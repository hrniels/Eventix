use anyhow::Context;
use chrono::{Duration, Local};
use ical::col::{CalSource, CalStore};
use ical::objects::CalComponent;

fn main() -> Result<(), anyhow::Error> {
    let dir = std::env::args().nth(1).unwrap();

    let mut store = CalStore::default();
    store.add(CalSource::new_from_dir(dir.into()).context("Unable to parse calendar source")?);

    println!("TODOs:");
    for todo in store.todos() {
        println!("  {:?}", todo.summary());
    }
    println!();

    let now = Local::now();
    let start = now.with_timezone(&chrono_tz::Europe::Berlin);
    let end = start + Duration::days(7);
    println!("Events between {} and {}:", start, end);

    let mut events = store
        .items_within(start, end)
        .filter_map(|(i, date)| match i {
            CalComponent::Event(ev) => Some((ev, date)),
            _ => None,
        })
        .collect::<Vec<_>>();
    events.sort_by(|(_, a), (_, b)| a.cmp(b));
    for (ev, date) in events {
        println!("  {:?} ({:?})", ev.summary(), date);
    }

    Ok(())
}
