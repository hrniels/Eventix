use anyhow::Context;
use chrono::{Duration, Local};
use ical::col::{CalSource, CalStore};

fn main() -> Result<(), anyhow::Error> {
    let dir = std::env::args().nth(1).unwrap();

    let mut store = CalStore::default();
    store.add(
        CalSource::new_from_dir(dir.into(), "".to_string())
            .context("Unable to parse calendar source")?,
    );

    println!("TODOs:");
    for todo in store.todos() {
        println!("  {:?}", todo.summary());
    }
    println!();

    let now = Local::now();
    let start = now.with_timezone(&chrono_tz::Europe::Berlin) - Duration::days(16);
    let end = start + Duration::days(7);

    let mut occurrences = store
        .components_within(start, end)
        .filter(|o| o.component().is_event())
        .collect::<Vec<_>>();
    occurrences.sort_by(|a, b| a.start().cmp(&b.start()));

    println!("Events between {} and {}:", start, end);
    for occ in occurrences {
        let ev = occ.component().as_event().unwrap();
        println!(
            "  {:?} ({:?} for {})",
            ev.summary(),
            occ.start(),
            if let Some(dur) = occ.duration() {
                format!("{} min", dur.num_minutes())
            } else {
                format!("??")
            }
        );
    }

    Ok(())
}
