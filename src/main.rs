use anyhow::Context;
use icalendar::Component;
use objects::{CalSource, CalStore};

mod objects;

fn main() -> Result<(), anyhow::Error> {
    let dir = std::env::args().nth(1).unwrap();

    let mut store = CalStore::default();
    store.add(CalSource::new_from_dir(dir.into()).context("Unable to parse calendar source")?);

    println!("TODOs:");
    for todo in store.items().map(|i| i.todos()).flatten() {
        println!("  {:?}", todo.get_summary());
    }
    println!();

    println!("Events:");
    for todo in store.items().map(|i| i.events()).flatten() {
        println!("  {:?}", todo.get_summary());
    }

    Ok(())
}
