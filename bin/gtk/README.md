# GTK calendar mutations

The timed week grid supports moving a single event occurrence by dragging it to a
new day/time. Starts snap to quarter hours, the grabbed position is retained, and
elapsed duration is preserved. Moving a recurring occurrence creates or updates
an exception with the original recurrence ID. Destination times use the existing
calendar gap/fold handling.

## Responsibilities

- `src/actions/items/` contains GTK interaction adapters for item operations.
  Drag/drop and future menu/editor entry points submit the same typed commands.
- `eventix_state::actions::items` implements validated, persisted mutations without
  depending on GTK. A move stages a calendar, atomically replaces its file, and
  only then replaces the authoritative in-memory calendar.
- `eventix_state::actions::Command` is the command enum, initially containing
  `MoveEvent`. The page and session accept commands through generic execution APIs.
  Commands expose affected identities before execution and return committed
  identities afterward, keeping query invalidation independent of the operation.
- `src/model/session.rs` owns `State` on one worker thread. Queries and commands
  are serialized; commands are never dropped when navigation changes.
- `src/model/query.rs` tracks the displayed calendar query and the independent
  sidebar query. Navigating replaces the calendar subscription. Hidden periods
  are loaded when displayed again.
- `eventix_view` builds owned snapshots. GTK reconciles the affected view using
  the existing widget identities.

## Change propagation

Before a command runs, the session looks up its target identity and records which
subscriptions contain it. After a successful commit, it looks up that identity
again and invalidates the union of old and new memberships. This handles both
leaving and entering a query, including moved recurrence exceptions.

`Changed` notifications contain a revision, item identities, and affected
subscription IDs. They contain no calendar-object copies or borrowed references
across threads. Consumers request fresh snapshots of affected subscriptions;
these are built from the latest session state. Calendar subscription IDs include
the navigation generation, so an obsolete invalidation cannot restore an old
period. Action failures are delivered independently of navigation generations.

Updates currently rebuild an affected week/month or sidebar, including layout.
They do not reload files or rebuild unrelated views. Query membership can be
conservative, allowing a harmless extra refresh. More granular projection updates
can be added behind this boundary later.

For a new operation, add its reusable mutation and a `Command` variant in
`eventix-state`, plus a GTK adapter under `actions/`. Implement the command's
affected identities and execution; the session checks query membership before
and after committing. Operations affecting an entire series must check
the series rather than just the selected occurrence. Calendar settings and
unknown external changes will require broader invalidation; external processes
need an explicit reload or a future file-watching/IPC adapter.
