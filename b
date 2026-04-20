#!/usr/bin/env python3

import argparse
import os
from pathlib import Path
import shutil
import shlex
import subprocess
import sys

# Define application identifiers for normal and debug mode
APP_ID = "com.github.hrniels.Eventix"
APP_ID_DEBUG = APP_ID + "-debug"


def dev_env():
    """Sets up the development environment by configuring environment variables
    and creating symbolic links for required directories."""
    env = os.environ.copy()
    run_dir = Path("run")
    os.makedirs(run_dir / APP_ID_DEBUG, 0o700, exist_ok=True)

    # (re-)create symlinks to data/static and data/icons
    # we use symlinks here so that `./b watch` sees changes to these files
    dirs = ["static", "icons", "locale"]
    for dirname in dirs:
        dir_in_run = run_dir / APP_ID_DEBUG / dirname
        if dir_in_run.exists():
            if dir_in_run.is_file() or dir_in_run.is_symlink():
                os.unlink(dir_in_run)
            else:
                shutil.rmtree(dir_in_run, ignore_errors=True)
        os.symlink((Path("data") / dirname).absolute(),
                   dir_in_run,
                   target_is_directory=True)

    # Add DavMail binary to PATH for subprocess usage
    davmail_bin = os.path.abspath("contrib/davmail/dist")
    if not os.path.isfile(davmail_bin + "/davmail"):
        sys.exit("Please install davmail first via ./b davmail")
    vdirsyncer_bin = os.path.abspath("run/venv/bin")
    if not os.path.isfile(vdirsyncer_bin + "/vdirsyncer"):
        sys.exit("Please install vdirsyncer first via ./b vdirsyncer")
    env["PATH"] = os.pathsep.join([davmail_bin, vdirsyncer_bin, env.get("PATH", "")])
    # use a project-local directory for data and config
    env["XDG_DATA_HOME"] = str(run_dir.absolute())
    env["XDG_CONFIG_HOME"] = str(run_dir.absolute())
    # for debugging
    env["RUST_LOG"] = "trace"
    env["RUST_BACKTRACE"] = "full"
    return env


def run_cmd(args):
    """Executes a command with the prepared development environment."""
    try:
        subprocess.run(args, env=dev_env())
    except KeyboardInterrupt:
        pass
    except Exception as e:
        print(e)


def cmd_run(args):
    """Runs the Eventix application in development mode."""
    cmd_args = [
        "cargo", "run", "--bin", "eventix", "--",
        "--address", args.address,
        "--port", str(args.port)
    ]
    run_cmd(cmd_args)


def cmd_watch(args):
    """Watches for changes in the source code and reruns Eventix on changes."""
    cmd = shlex.join([
        "run", "--",
        "--address", args.address,
        "--port", str(args.port)
    ])
    cmd_args = [
        "cargo", "watch", "-C", "bin/eventix",
        "-w", "../../bin/eventix",
        "-w", "../../bin/build.rs",
        "-w", "../../libs",
        "-w", "../../data",
        "-w", "../../Cargo.toml",
        "-x", cmd
    ]
    run_cmd(cmd_args)


def cmd_app(args):
    """Runs the Eventix app."""
    cmd_args = [
        "cargo", "run", "--bin", "eventix-app", "--",
        "--address", args.address,
        "--port", str(args.port)
    ]
    run_cmd(cmd_args)


def cmd_import(args):
    """Imports an ICS file into Eventix."""
    path = Path(args.file).resolve().as_uri()
    cmd_args = ["cargo", "run", "--bin", "eventix-import", "--", path]
    run_cmd(cmd_args)


def cmd_davmail(args):
    """Builds Davmail using Maven and Ant."""
    subprocess.run(["mvn", "install"], cwd='contrib/davmail', check=True)
    subprocess.run(["ant", "dist"], cwd='contrib/davmail', check=True)


def cmd_vdirsyncer(args):
    """Builds vdirsyncer using venv and pip."""
    subprocess.run(["python", "-m", "venv", "run/venv"])
    subprocess.run(["run/venv/bin/pip", "install", "-e", "contrib/vdirsyncer"])


def cmd_coverage(args):
    """Generates code coverage information for the workspace."""
    subprocess.run([
        "cargo", "llvm-cov",
        "--workspace",
        "--exclude", "eventix-import",
        "--exclude", "eventix-app",
        "--exclude", "evlist"
    ])


NPM_PREFIX = Path("target")
PRETTIER = ["npx", "--prefix", str(NPM_PREFIX), "prettier"]


def _ensure_npm_deps():
    """Installs npm dependencies into target/node_modules if not already present.

    Uses ``npm install --prefix target`` so that node_modules stays out of the
    repository root. A symlink from target/package.json to the root package.json
    is created first so that npm can locate the dependency list.
    """
    NPM_PREFIX.mkdir(exist_ok=True)
    pkg_link = NPM_PREFIX / "package.json"
    if not pkg_link.exists():
        pkg_link.symlink_to("../package.json")
    if not (NPM_PREFIX / "node_modules").exists():
        subprocess.run(["npm", "install", "--prefix", str(NPM_PREFIX)], check=True)


def cmd_format(args):
    """Formats Rust, JS, CSS, and HTML template files."""
    _ensure_npm_deps()
    subprocess.run(["cargo", "fmt"])
    subprocess.run(["yamlfmt", "-conf", ".yamlfmt.yaml", ".github"])
    subprocess.run(PRETTIER + ["--write",
                               "data/static/**/*.js",
                               "data/static/style.css",
                               "bin/eventix/templates/**/*.htm"], check=True)


def cmd_format_check(args):
    """Checks Rust, JS, CSS, and HTML template files (exits non-zero on diff)."""
    _ensure_npm_deps()
    subprocess.run(["cargo", "fmt", "--", "--check"])
    subprocess.run(PRETTIER + ["--check",
                               "data/static/**/*.js",
                               "data/static/style.css",
                               "bin/eventix/templates/**/*.htm"], check=True)


def cmd_flatpak(args):
    """Builds a Flatpak package for Eventix, including dependencies."""
    build_dir = "flatpak/build"
    repo_dir = "flatpak/repo"

    # generate archive for flatpak JSON
    subprocess.run([
        "tar", "czf", "flatpak/source.tar.gz",
        "--exclude=contrib/davmail/dist",
        # include .git for GIT_HASH and submodule version metadata
        ".git", "bin", "contrib", "data", "libs", "Cargo.toml", "Cargo.lock"
    ])

    # install flatpak dependencies
    runtimes = [
        "org.gnome.Platform//50",
        "org.gnome.Sdk//50",
        "org.freedesktop.Sdk.Extension.rust-stable//25.08",
        "org.freedesktop.Sdk.Extension.openjdk//25.08"
    ]
    for runtime in runtimes:
        subprocess.run(["flatpak", "install", "-y", "flathub", runtime], check=True)

    # build flatpak
    add_args = ["--disable-cache"] if not args.no_rebuild else []
    subprocess.run(
        ["flatpak-builder", "--disable-rofiles-fuse", "--force-clean"] + add_args +
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

    davmail_parser = subparsers.add_parser(
        "davmail", parents=[parent_parser], help="Build davmail")
    davmail_parser.set_defaults(func=cmd_davmail)

    vdirsyncer_parser = subparsers.add_parser(
        "vdirsyncer", parents=[parent_parser], help="Build vdirsyncer")
    vdirsyncer_parser.set_defaults(func=cmd_vdirsyncer)

    coverage_parser = subparsers.add_parser(
        "coverage", parents=[parent_parser], help="Generate code coverage information")
    coverage_parser.set_defaults(func=cmd_coverage)

    flatpak_parser = subparsers.add_parser(
        "flatpak", parents=[parent_parser], help="Build flatpak package")
    flatpak_parser.add_argument("--no-rebuild", help="Skip build step, just repackage",
                                action="store_true")
    flatpak_parser.set_defaults(func=cmd_flatpak)

    format_parser = subparsers.add_parser(
        "format", parents=[parent_parser],
        help="Format JS, CSS, and HTML templates with Prettier")
    format_parser.set_defaults(func=cmd_format)

    format_check_parser = subparsers.add_parser(
        "format-check", parents=[parent_parser],
        help="Check JS, CSS, and HTML template formatting with Prettier")
    format_check_parser.set_defaults(func=cmd_format_check)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
