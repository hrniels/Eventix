use anyhow::Context;
use chrono::{Duration, Local};
use objects::{CalSource, CalStore};

mod objects;
mod parser;

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
    for (ev, date) in store.items_within(start, end) {
        if let Some(ev) = ev.as_event() {
            println!("  {:?} ({:?})", ev.summary(), date);
        }
    }

    Ok(())
}
