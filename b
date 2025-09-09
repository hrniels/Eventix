#!/usr/bin/env python3

import argparse
import os
from pathlib import Path
import shutil
import shlex
import subprocess

APP_ID = "com.github.NilsTUD.Eventix"


def dev_env():
    env = os.environ.copy()
    run_dir = Path("run")
    os.makedirs(run_dir, 0o700, exist_ok=True)

    # update data
    shutil.rmtree(run_dir / APP_ID / "static", ignore_errors=True)
    shutil.copytree(Path("data") / "static", run_dir / APP_ID / "static")
    shutil.rmtree(run_dir / APP_ID / "icons", ignore_errors=True)
    shutil.copytree(Path("data") / "icons", run_dir / APP_ID / "icons")

    env["XDG_DATA_HOME"] = run_dir.absolute()
    env["XDG_CONFIG_HOME"] = run_dir.absolute()
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
        "run", "--",
        "--address", args.address,
        "--port", str(args.port)
    ])
    cmd_args = [
        "cargo", "watch", "-C", "bin/eventix", "-x", cmd
    ]
    run_cmd(cmd_args)


def cmd_app(args):
    cmd_args = [
        "cargo", "run", "--bin", "eventix-app", "--",
        "--address", args.address,
        "--port", str(args.port)
    ]
    run_cmd(cmd_args)


def cmd_import(args):
    cmd_args = ["cargo", "run", "--bin", "eventix-import", "--", args.file]
    run_cmd(cmd_args)


def cmd_flatpak(args):
    build_dir = "flatpak/build"
    repo_dir = "flatpak/repo"

    # generate archive for flatpak JSON
    subprocess.run([
        "tar", "czf", "flatpak/source.tar.gz",
        # include .git for GIT_HASH
        ".git", "bin", "data", "libs", "Cargo.toml", "Cargo.lock"
    ])

    # install flatpak dependencies
    runtimes = [
        "org.gnome.Platform//48",
        "org.gnome.Sdk//48",
        "org.freedesktop.Sdk.Extension.rust-stable//24.08"
    ]
    for runtime in runtimes:
        subprocess.run(["flatpak", "install", "-y", "flathub", runtime], check=True)

    # build flatpak
    add_args = ["--disable-cache"] if args.rebuild else []
    subprocess.run(
        ["flatpak-builder", "--force-clean"] + add_args +
        [build_dir, "flatpak/{}.json".format(APP_ID)],
        check=True)
    subprocess.run([
        "flatpak", "build-export", repo_dir, build_dir
    ], check=True)
    subprocess.run([
        "flatpak", "build-bundle", repo_dir, "flatpak/Eventix.flatpak", APP_ID
    ], check=True)

    print()
    print("Flatpak ready. You can install it via:")
    print("$ flatpak install --user flatpak/Eventix.flatpak")


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

    app_parser = subparsers.add_parser(
        "app", parents=[parent_parser],
        help="Run the eventix app with tray icon")
    app_parser.set_defaults(func=cmd_app)

    import_parser = subparsers.add_parser(
        "import", parents=[parent_parser], help="Import an ICS file")
    import_parser.add_argument("file", help="Path to the ICS file to import")
    import_parser.set_defaults(func=cmd_import)

    flatpak_parser = subparsers.add_parser(
        "flatpak", parents=[parent_parser], help="Build flatpak package")
    flatpak_parser.add_argument("--rebuild", help="Force a complete rebuild",
                                action="store_true")
    flatpak_parser.set_defaults(func=cmd_flatpak)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
