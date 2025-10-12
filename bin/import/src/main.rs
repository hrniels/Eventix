use anyhow::Context;
use clap::Parser;
use eventix_ical::objects::{Calendar, EventLike};
use gtk::gio::prelude::*;
use gtk::gio::{Cancellable, File};
use std::collections::HashSet;
use std::io::Write;
use std::sync::Arc;
use tempfile::NamedTempFile;
use tokio::runtime::Runtime;
use tokio::sync::Mutex;
use xdg::BaseDirectories;

use crate::model::{ImportCalendar, ImportComponent, ImportModel};
use crate::view::ImportView;

mod model;
mod view;

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

/// Simple GTK dialog to import ICS files into eventix
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// the ICS file to import
    file: String,
}

fn read_ics_file(uri: &str) -> anyhow::Result<String> {
    let file = File::for_uri(uri);
    let stream = file
        .read(None::<&Cancellable>)
        .context(format!("open {uri:?}"))?;

    let mut input = Vec::new();
    let mut buffer = [0u8; 8192];

    loop {
        // Read some bytes from stream
        let bytes_read = stream
            .read(&mut buffer, None::<&Cancellable>)
            .context(format!("read {uri:?}"))?;
        if bytes_read == 0 {
            break;
        }

        input.extend_from_slice(&buffer[..bytes_read]);
    }

    String::from_utf8(input).context(format!("parse UTF-8 {uri:?}"))
}

fn parse_ics_file(uri: &str) -> anyhow::Result<Calendar> {
    let in_str = read_ics_file(uri)?;
    in_str.parse::<Calendar>().context(format!("parse {uri:?}"))
}

struct ImportState {
    state: eventix_state::State,
    xdg: Arc<BaseDirectories>,
    file: String,
}

fn import(state: ImportState, cal: String) -> anyhow::Result<()> {
    let rt = Runtime::new().unwrap();

    // copy URI to temp file in run directory
    let mut tmp_file = NamedTempFile::new_in(state.xdg.get_runtime_directory()?)
        .context("create temp file in runtime directory")?;
    let ics_file = read_ics_file(&state.file)?;
    tmp_file.write_all(ics_file.as_bytes())?;

    let cmd = eventix_cmd::Request::Import(eventix_cmd::ImportOptions {
        file: tmp_file.path().to_str().unwrap().to_string(),
        calendar: cal,
    });

    rt.block_on(async {
        eventix_cmd::send_or_execute(&state.xdg, Arc::new(Mutex::new(state.state)), cmd)
            .await
            .map(|_| ())
    })
}

fn main() {
    let args = Args::parse();

    ImportView::init();

    let xdg = Arc::new(BaseDirectories::with_prefix(APP_ID));
    let state = eventix_state::State::new(xdg.clone()).expect("loading state");
    let locale = state.settings().locale();

    // collect all calendars
    let calendars = state
        .settings()
        .calendars()
        .map(|(id, cal)| ImportCalendar {
            id: id.clone(),
            name: cal.name().clone(),
            color: cal.bgcolor().clone(),
            types: cal.types().to_vec(),
        })
        .collect();

    // parse items from ICS file
    let ics = parse_ics_file(&args.file).unwrap();
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
                    .1
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
