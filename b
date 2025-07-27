#!/usr/bin/env python3

import argparse
import os
from pathlib import Path
import platformdirs
import shutil
import shlex
import subprocess
import sys


def dev_env():
    env = os.environ.copy()
    env["XDG_DATA_HOME"] = os.path.abspath("data")
    env["XDG_CONFIG_HOME"] = os.path.abspath("config")
    env["RUST_LOG"] = "trace"
    env["RUST_BACKTRACE"] = "full"
    return env


def run_cmd(args):
    try:
        subprocess.run(args, env=dev_env())
    except KeyboardInterrupt:
        pass
    except Exception as e:
        print(e)


def cmd_run(args):
    cmd_args = [
        "cargo", "run", "--bin", "eventix", "--",
        "--address", args.address,
        "--port", str(args.port)
    ]
    run_cmd(cmd_args)


def cmd_watch(args):
    cmd = shlex.join([
        "run", "--bin", "eventix", "--",
        "--address", args.address,
        "--port", str(args.port)
    ])
    cmd_args = [
        "cargo", "watch", "--ignore", "data", "--ignore", "config", "-x", cmd
    ]
    run_cmd(cmd_args)


def cmd_import(args):
    cmd_args = ["cargo", "run", "--bin", "eventix-import", "--", args.file]
    run_cmd(cmd_args)


def cmd_install(args):
    dir = Path(platformdirs.user_data_dir()) / "eventix"
    print("Installing data to {}...".format(dir))
    shutil.rmtree(dir, ignore_errors=True)
    shutil.copytree(Path("data") / "eventix" / "static", dir / "static")


def main():
    parent_parser = argparse.ArgumentParser(add_help=False)
    parent_parser.add_argument(
        "--address", default="127.0.0.1", help="Server address")
    parent_parser.add_argument(
        "--port", type=int, default=8083, help="Server port")

    parser = argparse.ArgumentParser(description="Eventix builder and runner")
    subparsers = parser.add_subparsers(
        dest="command", help="Available commands")
    subparsers.required = True

    run_parser = subparsers.add_parser(
        "run", parents=[parent_parser], help="Run eventix in development mode")
    run_parser.set_defaults(func=cmd_run)

    watch_parser = subparsers.add_parser(
        "watch", parents=[parent_parser],
        help="Watch and rerun eventix on changes")
    watch_parser.set_defaults(func=cmd_watch)

    import_parser = subparsers.add_parser(
        "import", parents=[parent_parser], help="Import an ICS file")
    import_parser.add_argument("file", help="Path to the ICS file to import")
    import_parser.set_defaults(func=cmd_import)

    install_parser = subparsers.add_parser(
        "install", parents=[parent_parser], help="Install eventix")
    install_parser.set_defaults(func=cmd_install)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
