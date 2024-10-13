use anyhow::Context;
use chrono::{Duration, Utc};
use icalendar::Component;
use objects::{CalSource, CalStore};

mod objects;

fn main() -> Result<(), anyhow::Error> {
    let dir = std::env::args().nth(1).unwrap();

    let mut store = CalStore::default();
    store.add(CalSource::new_from_dir(dir.into()).context("Unable to parse calendar source")?);

    println!("TODOs:");
    for todo in store.todos() {
        println!("  {:?}", todo.get_summary());
    }
    println!();

    println!("Events:");
    // for todo in store.events() {
    //     println!("  {:?}", todo.get_summary());
    // }

    let end = Utc::now();
    let start = end - Duration::days(30);
    for ev in store.items_within(start, end) {
        if let Some(ev) = ev.as_event() {
            println!(
                "  {:?} ({:?} .. {:?})",
                ev.get_summary(),
                ev.get_start(),
                ev.get_end()
            );
        }
    }

    Ok(())
}
