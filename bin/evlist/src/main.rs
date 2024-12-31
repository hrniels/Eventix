use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use chrono::{Duration, Local};
use ical::col::{CalSource, CalStore};
use ical::objects::{CalCompType, EventLike};

fn main() -> Result<(), anyhow::Error> {
    let dir = std::env::args().nth(1).unwrap();

    let mut store = CalStore::default();
    store.add(
        CalSource::new_from_dir(
            Arc::new(String::from("test")),
            dir.clone().into(),
            "".to_string(),
            HashMap::new(),
        )
        .context(format!("Unable to parse calendar source {:?}", dir))?,
    );

    println!("TODOs:");
    for todo in store.todos() {
        println!("  {:?}", todo.summary());
    }
    println!();

    let now = Local::now();
    let start = now.with_timezone(&chrono_tz::Europe::Berlin);
    let end = start + Duration::days(14);

    let mut occurrences = store
        .occurrences_within(start, end)
        .filter(|o| o.ctype() == CalCompType::Event)
        .collect::<Vec<_>>();
    occurrences.sort_by_key(|a| a.occurrence_start());

    println!("Events between {} and {}:", start, end);
    for occ in occurrences {
        println!(
            "  {:?} ({:?} for {})",
            occ.summary(),
            occ.occurrence_start(),
            if let Some(dur) = occ.duration() {
                format!("{} min", dur.num_minutes())
            } else {
                "??".to_string()
            }
        );
    }

    store.save()?;

    let mut store2 = CalStore::default();
    store2.add(
        CalSource::new_from_dir(
            Arc::new(String::from("test")),
            dir.into(),
            "".to_string(),
            HashMap::new(),
        )
        .context("Unable to parse calendar source")?,
    );

    if store != store2 {
        println!("{:#?}", store);
        println!("-----");
        println!("{:#?}", store2);
    }

    Ok(())
}
