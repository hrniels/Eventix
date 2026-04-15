# Eventix

[![codecov](https://codecov.io/github/hrniels/Eventix/graph/badge.svg?token=MVGTKGG6J9)](https://codecov.io/github/hrniels/Eventix)

Eventix is an iCalendar event and task manager for Linux desktops. It runs a local web server that
provides a calendar UI, and ships a GTK desktop wrapper that embeds that UI in a native application
window with system tray integration.

## Features

- Monthly, weekly, and list calendar views
- Event (`VEVENT`) and task (`VTODO`) management with full create / edit / delete support
- Full RFC 5545 `RRULE` recurrence support (`SECONDLY` through `YEARLY`, `COUNT`, `UNTIL`, `BYDAY`,
  `BYMONTH`, `BYMONTHDAY`, `BYYEARDAY`, `BYWEEKNO`, `BYSETPOS`, `BYHOUR`, `BYMINUTE`, `BYSECOND`,
  `WKST`, `EXDATE`)
- Alarm / notification system with per-calendar personal alarm overrides
- Attendee and organizer support for group-scheduled events
- CalDAV synchronization via [vdirsyncer](https://github.com/pimutils/vdirsyncer) (bundled)
- Microsoft 365 synchronization via [DavMail](http://davmail.sourceforge.net/) (bundled)
- Local filesystem calendar support (no sync required)
- iCalendar (`.ics`) file import via a GTK dialog
- System tray icon showing due-today and overdue task counts
- Multi-language UI (English and German)
- XDG-compliant configuration and data storage
- Flatpak packaging (`com.github.hrniels.Eventix`)

## Installation

Eventix is intended to be run as a Flatpak application. The flatpak package can be built and
installed via:

```bash
./b flatpak
flatpak install --user flatpak/Eventix.flatpak
```

## Running

Start the server:

```bash
flatpak run --command=eventix com.github.hrniels.Eventix
```

Afterwards, start the desktop UI. The flatpak package comes with a `.desktop` file, but it can also
be started via CLI:

```bash
flatpak run com.github.hrniels.Eventix
```

It might make sense to run the server via systemd:

```ini
[Unit]
Description=Eventix webserver

[Service]
Environment=RUST_LOG=info
ExecStart=/usr/bin/flatpak run --command=eventix com.github.hrniels.Eventix
ExecStop=/usr/bin/flatpak kill com.github.hrniels.Eventix
Restart=on-failure
KillMode=process

[Install]
WantedBy=default.target
```

## Relevant Files

Eventix can be configured completely via its web UI. However, in case manual inspection is desired,
configuration and other files are stored in XDG-standard locations under the app ID prefix
`com.github.hrniels.Eventix`. With flatpak, the base directory will be under
`$HOME/.var/app/com.github.hrniels.Eventix`. The relevant files and directories are:

| File | Purpose |
|---|---|
| `<base>/config/com.github.hrniels.Eventix/settings.toml` | Collection and calendar settings |
| `<base>/data/com.github.hrniels.Eventix/misc.toml` | Runtime state: last alarm check, disabled calendars, etc. |
| `<base>/data/com.github.hrniels.Eventix/alarms` | Personal alarms |
| `<base>/data/com.github.hrniels.Eventix/vdirsyncer` | Calendar files from remote servers |

## Architecture

The project is organized into binaries and libraries.

```
eventix/
├── bin/
│   ├── eventix/        # Core: Axum web server + calendar UI
│   ├── app/            # GTK desktop wrapper with system tray
│   ├── import/         # GTK dialog for importing .ics files
├── libs/
│   ├── ical/           # RFC 5545 iCalendar parser and object model
│   ├── state/          # Application state: settings, sync backends, alarms
│   ├── locale/         # Locale/i18n trait and English + German implementations
│   └── cmd/            # IPC protocol over a Unix domain socket
├── data/               # Runtime assets: icons, locale files, static web files
├── flatpak/            # Flatpak build manifests and .desktop files
├── contrib/davmail/    # DavMail submodule for Microsoft 365 CalDAV bridging
└── contrib/vdirsyncer/ # vdirsyncer submodule bundled with Eventix
```

## Tipps

Since Eventix is running in a flatpak sandbox, it does not have direct access to, for example, your
password manager. If you want to retrieve passwords from a password manager, one way is to use
`flatpak-spawn` and enter something like the following as the "Password Command" for a collection:

```bash
flatpak-spawn --host secret-tool lookup <attribute> <value>
```

## License

Eventix is licensed under the [GNU General Public License v3.0 or later](LICENSE)
(SPDX: `GPL-3.0-or-later`).

Bundled third-party components retain their own licenses:

- [DavMail](http://davmail.sourceforge.net/) (`contrib/davmail/`) — GPL-2.0
- [vdirsyncer](https://github.com/pimutils/vdirsyncer) (`contrib/vdirsyncer/`) — BSD-3-Clause
