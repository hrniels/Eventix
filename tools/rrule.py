#!/usr/bin/env python3

import argparse
from dateutil.rrule import rrulestr
from dateutil.parser import parse as parse_dt
from dateutil.tz import gettz
import sys


def main():
    parser = argparse.ArgumentParser(
        description="Generate a sequence of dates from an RRULE string."
    )
    parser.add_argument("rrule", help="RRULE string in ICS format, e.g. 'FREQ=WEEKLY;COUNT=10'")
    parser.add_argument("dtstart", help="Start date (DTSTART) in ISO format, e.g. '2026-03-01T10:00:00'")
    parser.add_argument("count", type=int, help="Number of instances to generate")
    parser.add_argument("--tz", help="Timezone name (e.g., Europe/Berlin). Default: UTC", default="UTC")
    parser.add_argument("--after", help="Only show instances after this date (ISO format)")

    args = parser.parse_args()

    # Parse timezone
    tzinfo = gettz(args.tz)
    if tzinfo is None:
        print(f"Unknown timezone: {args.tz}", file=sys.stderr)
        sys.exit(1)

    # Parse start date
    dtstart = parse_dt(args.dtstart)
    dtstart = dtstart.replace(tzinfo=tzinfo)

    # Build RRULE string in full format
    rrule_full = f"DTSTART:{dtstart.strftime('%Y%m%dT%H%M%S')}\nRRULE:{args.rrule}"

    # Parse RRULE
    rule = rrulestr(rrule_full, forceset=True)

    # Apply --after filter if specified
    after_dt = parse_dt(args.after).replace(tzinfo=tzinfo) if args.after else None

    # Generate instances
    instances = []
    for dt in rule:
        if after_dt and dt <= after_dt:
            continue
        instances.append(dt)
        if len(instances) >= args.count:
            break

    # Print in ISO 8601
    for dt in instances:
        print(dt.isoformat())


if __name__ == "__main__":
    main()
