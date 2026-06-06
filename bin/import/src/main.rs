// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{anyhow, Context};
use clap::Parser;
use eventix_cmd::Response;
use eventix_ical::objects::{Calendar, EventLike};
use eventix_locale::Locale;
use eventix_state::{Misc, Settings};
use formatx::formatx;
use gtk::gio::prelude::*;
use gtk::gio::{Cancellable, File};
use std::collections::HashSet;
use std::io::Write;
use std::sync::Arc;
use tempfile::NamedTempFile;
use tokio::runtime::Runtime;
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

fn format_error(summary: &str, err: &anyhow::Error) -> String {
    let mut lines = vec![summary.to_string()];

    for cause in err.chain() {
        lines.push(format!("Caused by: {cause}"));
    }

    lines.join("\n")
}

fn error_and_exit<M: AsRef<str>>(msg: M) -> ! {
    ImportView::show_error(msg.as_ref());
    std::process::exit(1);
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

    rt.block_on(async { eventix_cmd::send(&state.xdg, cmd).await.map(|_| ()) })
}

async fn build_model(
    xdg: &BaseDirectories,
    locale: &Arc<dyn Locale + Send + Sync>,
    calendars: Vec<ImportCalendar>,
    ics: &Calendar,
) -> anyhow::Result<ImportModel> {
    let mut items = Vec::new();
    for c in ics.components().iter().filter(|c| c.rid().is_none()) {
        let Response::SearchResponse(exists_in) =
            eventix_cmd::send(xdg, eventix_cmd::Request::Search(c.uid().clone())).await?
        else {
            return Err(anyhow!("Unexpected response"));
        };
        items.push(ImportComponent {
            ty: c.ctype(),
            summary: c.summary().cloned(),
            start: c.start().cloned(),
            end: c.end_or_due().cloned(),
            rrule: c.rrule().cloned(),
            exists_in,
        })
    }

    if items
        .iter()
        .filter_map(|i| i.exists_in.as_ref().map(|(id, _name)| id))
        .collect::<HashSet<_>>()
        .len()
        > 1
    {
        error_and_exit(locale.translate("error.import_multiple_calendars"));
    }

    Ok(ImportModel::new(calendars, items))
}

fn main() {
    let args = Args::parse();

    ImportView::init();

    let xdg = Arc::new(BaseDirectories::with_prefix(APP_ID));
    let misc = Misc::load_from_file(&xdg).expect("load misc state");
    let settings = Settings::load_from_file(&xdg).expect("load settings");
    let locale = eventix_locale::new(&xdg, misc.locale_type()).expect("create locale");

    // collect all calendars
    let calendars = settings
        .calendars()
        .map(|(id, cal)| ImportCalendar {
            id: id.clone(),
            name: cal.name().clone(),
            color: cal.bgcolor().clone(),
            types: cal.types().to_vec(),
        })
        .collect();

    // parse items from ICS file
    let ics = match parse_ics_file(&args.file) {
        Err(err) => error_and_exit(format_error(
            &formatx!(locale.translate("error.import_parse_file"), args.file).unwrap(),
            &err,
        )),
        Ok(ics) => ics,
    };

    let rt = Runtime::new().unwrap();
    let model = rt
        .block_on(async { build_model(&xdg, &locale, calendars, &ics).await })
        .expect("create model");

    // build our own state for the import later and pass it through the view
    let import_state = ImportState {
        file: args.file,
        xdg: xdg.clone(),
    };
    let view = match ImportView::new(model, &xdg, &*locale, import_state, import) {
        Ok(view) => view,
        Err(err) => error_and_exit(format_error(locale.translate("error.import_prepare"), &err)),
    };

    view.show();
}
