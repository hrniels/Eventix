use anyhow::Context;
use clap::Parser;
use eventix_ical::objects::{Calendar, EventLike};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::Mutex;
use xdg::BaseDirectories;

use crate::model::{ImportCalendar, ImportComponent, ImportModel};
use crate::view::ImportView;

mod model;
mod view;

/// Simple GTK dialog to import ICS files into eventix
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// the ICS file to import
    file: String,
}

fn parse_ics_file(path: PathBuf) -> anyhow::Result<Calendar> {
    let mut input = String::new();
    File::open(&path)
        .context(format!("open {:?}", &path))?
        .read_to_string(&mut input)
        .context(format!("read {:?}", &path))?;

    input
        .parse::<Calendar>()
        .context(format!("parse {:?}", &path))
}

struct ImportState {
    state: eventix_state::State,
    xdg: Arc<BaseDirectories>,
    file: String,
}

fn import(state: ImportState, cal: String) -> anyhow::Result<()> {
    let cmd = eventix_cmd::Command::Import(eventix_cmd::ImportOptions {
        file: state.file,
        calendar: cal,
    });

    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        eventix_cmd::send_or_execute(&state.xdg, Arc::new(Mutex::new(state.state)), cmd).await
    })
}

fn main() {
    let args = Args::parse();

    ImportView::init();

    let xdg = Arc::new(BaseDirectories::with_prefix("eventix"));
    let locale = eventix_locale::default();
    let state = eventix_state::State::new(xdg.clone()).expect("loading state");

    // collect all calendars
    let calendars = state
        .settings()
        .calendars()
        .iter()
        .map(|(id, cal)| ImportCalendar {
            id: id.clone(),
            name: cal.name().clone(),
            color: cal.bgcolor().clone(),
            types: cal.types().to_vec(),
        })
        .collect();

    // parse items from ICS file
    let ics = parse_ics_file(PathBuf::from(&args.file)).unwrap();
    let items = ics
        .components()
        .iter()
        .filter(|c| c.rid().is_none())
        .map(|c| {
            let exists_in = state.store().file_by_id(c.uid()).map(|c_file| {
                let name = state
                    .settings()
                    .calendar(c_file.directory())
                    .unwrap()
                    .name();
                ((**c_file.directory()).clone(), name.clone())
            });
            ImportComponent {
                ty: c.ctype(),
                summary: c.summary().cloned(),
                start: c.start().cloned(),
                end: c.end_or_due().cloned(),
                rrule: c.rrule().cloned(),
                exists_in,
            }
        })
        .collect::<Vec<_>>();

    if items
        .iter()
        .filter_map(|i| i.exists_in.as_ref().map(|(id, _name)| id))
        .collect::<HashSet<_>>()
        .len()
        > 1
    {
        ImportView::show_error(
            "The ICS file contains multiple components that exist \
             in different calendars and thus cannot be imported.",
        );
        std::process::exit(1);
    }

    // build model and pass it to view
    let model = ImportModel::new(calendars, items);

    // build our own state for the import later and pass it through the view
    let import_state = ImportState {
        file: args.file,
        state,
        xdg: xdg.clone(),
    };
    let view = ImportView::new(model, &xdg, &*locale, import_state, import);

    view.show();
}
