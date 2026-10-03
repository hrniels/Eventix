#!/usr/bin/env python3

import argparse
import fnmatch
import json
import os
from pathlib import Path
import re
import shutil
import shlex
import subprocess
import sys
import tempfile

# Define application identifiers for normal and debug mode
APP_ID = "io.github.hrniels.Eventix"
APP_ID_DEBUG = APP_ID + "-debug"

PY_VENV = Path("run/venv")
STATE_DIR = Path("flatpak/state")
REPO_DIR = Path("flatpak/repo")


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
    vdirsyncer_bin = PY_VENV / "bin"
    if not os.path.isfile(vdirsyncer_bin / "vdirsyncer"):
        sys.exit("Please install vdirsyncer first via ./b vdirsyncer")
    eventix_bin = os.path.abspath("target/debug")
    env["PATH"] = os.pathsep.join([davmail_bin, str(vdirsyncer_bin.absolute()), eventix_bin, env.get("PATH", "")])
    # use a project-local directory for data and config
    env["XDG_DATA_HOME"] = str(run_dir.absolute())
    env["XDG_CONFIG_HOME"] = str(run_dir.absolute())
    # for debugging
    env["RUST_LOG"] = "trace"
    env["RUST_BACKTRACE"] = "full"
    env["EVENTIX_TESTS"] = "1"
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
    # ensure that the server and getpw are up-to-date as well
    subprocess.run(["cargo", "build", "--bin", "eventix", "--bin", "eventix-getpw"], check=True)

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
    subprocess.run(["python", "-m", "venv", str(PY_VENV)])
    subprocess.run([str(PY_VENV / "bin/pip"), "install", "-e", "contrib/vdirsyncer"])


def cmd_test(args):
    """Runs cargo tests with the prepared development environment."""
    # Some integration tests spawn helper binaries like eventix-getpw via PATH.
    # cargo test only rebuilds the selected test targets, so ensure the helper
    # binaries are up to date before running tests.
    subprocess.run(["cargo", "build", "--bin", "eventix-getpw"], check=True)

    cmd = ["cargo", "test"]
    cmd.extend(args.cargo_args)
    if args.nocapture:
        cmd.extend(["--", "--nocapture"])
    subprocess.run(cmd, env=dev_env(), check=True)


def cmd_coverage(args):
    """Generates code coverage information for the workspace."""
    cmd = [
        "cargo", "llvm-cov",
        "--all-features",
        "--workspace",
        "--exclude", "eventix-import",
        "--exclude", "eventix-app",
    ]
    cmd.extend(args.cargo_args)

    if not args.file:
        subprocess.run(cmd, env=dev_env(), check=True)
        return

    with tempfile.TemporaryDirectory() as tmpdir:
        report_path = Path(tmpdir) / "coverage.json"
        subprocess.run(cmd + ["--json", "--output-path", str(report_path)], env=dev_env(), check=True)
        report = json.loads(report_path.read_text())
    covered_files = find_covered_files(report, args.file)
    for idx, covered_file in enumerate(covered_files):
        if idx > 0:
            print()
        print_file_coverage(covered_file)


def find_covered_files(report, requested_pattern):
    """Returns coverage entries for all files whose paths contain the requested pattern."""
    requested = requested_pattern.replace("\\", "/")
    is_glob = any(ch in requested for ch in "*?[]")
    matches = []
    for data in report.get("data", []):
        for file_data in data.get("files", []):
            filename = file_data["filename"].replace("\\", "/")
            if fnmatch.fnmatch(filename, requested) or (
                not is_glob and requested in filename
            ):
                matches.append(file_data)

    if not matches:
        sys.exit(f"No coverage data found for pattern '{requested_pattern}'.")

    return sorted(matches, key=lambda file_data: file_data["filename"])


def build_line_coverage(file_data, line_count):
    """Builds a line-to-execution-count map from LLVM segment coverage data."""
    coverage = {}
    segments = file_data.get("segments", [])
    for idx, segment in enumerate(segments):
        line, _col, count, has_count, _is_region_entry, is_gap_region = segment
        if not has_count or is_gap_region or line > line_count:
            continue

        next_line = line_count + 1
        if idx + 1 < len(segments):
            next_line = segments[idx + 1][0]

        last_line = line if next_line == line else min(next_line - 1, line_count)
        for current in range(line, last_line + 1):
            prev = coverage.get(current)
            coverage[current] = count if prev is None else max(prev, count)

    return coverage


def print_file_coverage(file_data):
    """Prints line-by-line coverage for a single source file."""
    path = Path(file_data["filename"])
    lines = path.read_text().splitlines()
    coverage = build_line_coverage(file_data, len(lines))

    print(f"\033[1m{path}\033[0m")
    for idx, line in enumerate(lines, start=1):
        count = coverage.get(idx)
        if count is None:
            marker = " " * 7
        elif count == 0:
            marker = "#####  "
        else:
            marker = f"{count:>7}"
        print(f"{marker} {idx:>5} | {line}")


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


def cmd_flatpak_sources(args):
    """Generates the Flatpak sources for later package builds or FlatHub."""
    sdk_id = "org.gnome.Sdk//50"

    # Ensure cargo-sources.json is up to date
    venv_bin = PY_VENV / "bin"
    subprocess.run(["python", "-m", "venv", str(PY_VENV)])
    subprocess.run([
        venv_bin / "pip",
        "install", "aiohttp", "tomlkit", "requirements-parser", "packaging"
    ], check=True)
    subprocess.run([
        str(venv_bin / "python"), "contrib/flatpak-cargo-generator.py",
        "Cargo.lock", "-o", "flatpak/cargo-sources.json"
    ], check=True)

    # download vdirsyncer dependencies and generate source list
    subprocess.run([
        str(venv_bin / "python"), "contrib/flatpak-pip-generator.py",
        "--output", "flatpak/python-sources",
        "--pyproject-file", "contrib/vdirsyncer/pyproject.toml"
    ], check=True)

    # prevent question of whether to install it into system or user
    user_flag = ["--user"] if (Path.home() / ".local/share/flatpak/runtime/org.gnome.Sdk/x86_64/50").exists() else []
    # build DavMail and generate source list
    with tempfile.TemporaryDirectory(dir="flatpak") as tmp_javadeps:
        # Use relative path for Maven repo local to avoid flatpak-in-flatpak issues
        rel_tmp_javadeps = os.path.relpath(tmp_javadeps, "contrib/davmail")
        log_file = Path(tmp_javadeps) / "maven-log.txt"
        try:
            for (arch, flag) in [("x86_64", "w"), ("aarch64", "a")]:
                with open(log_file, flag) as f:
                    process = subprocess.Popen([
                        "flatpak",
                        "run",
                        f"--arch={arch}",
                        *user_flag,
                        "--command=sh",
                        "--share=network",
                        "--filesystem=" + str(Path.cwd()),
                        sdk_id,
                        "-c",
                        "export PATH=/usr/lib/sdk/openjdk/bin:$PATH && "
                        "export JAVA_HOME=/usr/lib/sdk/openjdk && "
                        "cd contrib/davmail && "
                        "mvn install -Dmaven.repo.local=" + rel_tmp_javadeps,
                    ], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                    stdout, stderr = process.communicate()
                    f.write(stdout.decode())
                    sys.stdout.buffer.write(stdout)
                    sys.stderr.buffer.write(stderr)
                    f.write(stderr.decode())
                    if process.returncode != 0:
                        raise subprocess.CalledProcessError(process.returncode, process.args)
            subprocess.run([
                str(venv_bin / "python"), "contrib/flatpak-gradle-generator.py",
                "--destdir", "flatpak/java-deps",
                "--maven-repo", tmp_javadeps,
                str(log_file), "flatpak/java-sources.json"
            ], check=True)
        finally:
            if log_file.exists():
                log_file.unlink()


def _patch_manifest(manifest_path, archives):
    """Patches the manifest for local build using separate archives."""
    with open(manifest_path, "r") as f:
        manifest = json.load(f)

    for module in manifest.get("modules", []):
        if isinstance(module, str):
            continue

        name = module.get("name")
        if name in archives:
            # Replace the git/file source with our local archive
            # We assume the first source is the main one to be replaced
            new_sources = [{
                "type": "archive",
                "path": archives[name],
                "strip-components": 1
            }]
            # Keep other sources (like dependency lists)
            if "sources" in module:
                for src in module["sources"]:
                    if isinstance(src, str) or (isinstance(src, dict) and src.get("type") != "git"):
                        # Keep everything that isn't a git source (or the main one)
                        # Actually, flathub manifest usually has one git source.
                        if isinstance(src, dict) and src.get("type") == "git":
                            continue
                        new_sources.append(src)
            module["sources"] = new_sources

        # Specific fixes for relative paths in build commands
        if name == "vdirsyncer":
            build_options = module.setdefault("build-options", {})
            env = build_options.setdefault("env", {})
            env["SETUPTOOLS_SCM_PRETEND_VERSION_FOR_VDIRSYNCER"] = "0.20.0+eventix"

        if name == "davmail":
            module["build-commands"] = [
                cmd.replace("/run/build/davmail/flatpak/java-deps", "/run/build/davmail/java-deps")
                for cmd in module.get("build-commands", [])
            ]

    return manifest


def _create_archives(app_id):
    """Creates separate archives for Eventix, vdirsyncer, and davmail."""
    archives = {
        "Eventix": "eventix.tar.gz",
        "vdirsyncer": "vdirsyncer.tar.gz",
        "davmail": "davmail.tar.gz",
    }

    # 1. Eventix (core)
    subprocess.run([
        "tar", "czf", "flatpak/eventix.tar.gz",
        "--exclude=./contrib",
        # put everything into a subdirectory
        "--transform=s#^#eventix/#",
        ".git", "bin", "data", "libs", "Cargo.toml", "Cargo.lock", "package.json", "LICENSE", "b",
        "flatpak/" + app_id + "-Import.desktop",
        "flatpak/" + app_id + ".desktop",
        "flatpak/" + app_id + ".metainfo.xml",
    ], check=True)

    # 2. vdirsyncer
    subprocess.run([
        "tar", "czf", "flatpak/vdirsyncer.tar.gz",
        "-C", "contrib",
        "vdirsyncer",
    ], check=True)

    # 3. DavMail
    subprocess.run([
        "tar", "czf", "flatpak/davmail.tar.gz",
        "-C", "contrib",
        "--exclude=davmail/dist",
        "davmail",
    ], check=True)

    return archives


def _run_flatpak_builder(manifest_path, extra_args):
    """Run the flatpak builder command with given extra arguments."""
    subprocess.run([
        "flatpak", "run", "--command=flathub-build",
        "--filesystem=" + str(Path.cwd()), "org.flatpak.Builder",
        "--state-dir=" + str(STATE_DIR),
        "--repo=" + str(REPO_DIR),
        "--delete-build-dirs",
        *extra_args,
        manifest_path,
    ], check=True)


def _prepare_flatpak_manifest():
    """Generate archives and prepare a temporary patched manifest.

    Returns the path to the temporary manifest file. The caller is
    responsible for deleting it when done.
    """
    archives = _create_archives(APP_ID)
    manifest_path = Path("flatpak") / (APP_ID + ".json")
    manifest = _patch_manifest(manifest_path, archives)

    tmp = tempfile.NamedTemporaryFile(mode="w", suffix=".json", dir="flatpak", delete=False)
    json.dump(manifest, tmp, indent=4)
    tmp.close()
    return tmp.name


def cmd_flatpak_download(args):
    """Download Flatpak sources without building."""
    tmp_manifest_path = _prepare_flatpak_manifest()
    try:
        _run_flatpak_builder(tmp_manifest_path, ["--download-only"])
    finally:
        os.unlink(tmp_manifest_path)


def cmd_flatpak_build(args):
    """Build the Flatpak package using already-downloaded sources."""
    tmp_manifest_path = _prepare_flatpak_manifest()
    try:
        build_args = ["--disable-cache"] if not args.no_rebuild else []
        # Use --disable-download to ensure we don't try to download anything
        _run_flatpak_builder(tmp_manifest_path, build_args + ["--disable-download"])
        subprocess.run([
            "flatpak", "build-bundle", str(REPO_DIR), "flatpak/Eventix.flatpak", APP_ID
        ], check=True)
    finally:
        os.unlink(tmp_manifest_path)

    # remove builddir; apparently we cannot control where that's stored
    shutil.rmtree("builddir", ignore_errors=True)

    print()
    print("Flatpak ready. You can install it via:")
    print("$ flatpak install --user flatpak/Eventix.flatpak")


def cmd_flatpak(args):
    """Builds a Flatpak package for Eventix, including dependencies."""
    cmd_flatpak_sources(None)
    cmd_flatpak_download(None)
    cmd_flatpak_build(args)


def cmd_dist(args):
    """Installs all required files to the given prefix."""
    prefix = Path(args.prefix)
    os.makedirs(prefix, 0o755, exist_ok=True)

    # build in release mode
    subprocess.run(["cargo", "build", "--release"], check=True)

    # install binaries
    bins = Path("target") / "release"
    for bin_name in ["eventix", "eventix-app", "eventix-import", "eventix-getpw"]:
        subprocess.run(["install", "-Dm755", bins / bin_name, "-t", prefix / "bin"], check=True)

    # install desktop files
    apps = prefix / "share" / "applications"
    for suffix in ["", "-Import"]:
        src = Path("flatpak") / f"{APP_ID}{suffix}.desktop"
        subprocess.run(["install", "-Dm644", src, "-t", apps], check=True)

    # install icons
    data = Path("data")
    icons_dst = prefix / "share" / "icons" / "hicolor"
    icons_src = data / "icons"
    subprocess.run(["install", "-Dm644", icons_src / "scalable.svg",
                    icons_dst / "scalable" / "apps" / f"{APP_ID}.svg"], check=True)
    for png in ["256x256", "128x128", "64x64", "48x48"]:
        subprocess.run(["install", "-Dm644", icons_src / f"{png}.png",
                        icons_dst / png / "apps" / f"{APP_ID}.png"], check=True)
    subprocess.run(["install", "-Dm644", icons_src / "index.theme", "-t", icons_dst], check=True)

    # install metainfo and license
    metainfo_src = Path("flatpak") / f"{APP_ID}.metainfo.xml"
    metainfo_dst = prefix / "share" / "metainfo"
    subprocess.run(["install", "-Dm644", metainfo_src, "-t", metainfo_dst], check=True)
    license_dst = prefix / "share" / "licenses" / APP_ID / "eventix"
    subprocess.run(["install", "-Dm644", "LICENSE", "-t", license_dst], check=True)

    # install data folders
    dst = prefix / "share" / APP_ID
    os.makedirs(dst, 0o755, exist_ok=True)
    for dir_name in ["locale", "static", "icons"]:
        if (dst / dir_name).exists():
            shutil.rmtree(dst / dir_name)
        shutil.copytree(data / dir_name, dst / dir_name)


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

    import_parser = subparsers.add_parser("import", help="Import an ICS file")
    import_parser.add_argument("file", help="Path to the ICS file to import")
    import_parser.set_defaults(func=cmd_import)

    davmail_parser = subparsers.add_parser("davmail", help="Build davmail")
    davmail_parser.set_defaults(func=cmd_davmail)

    vdirsyncer_parser = subparsers.add_parser("vdirsyncer", help="Build vdirsyncer")
    vdirsyncer_parser.set_defaults(func=cmd_vdirsyncer)

    test_parser = subparsers.add_parser("test", help="Run cargo tests with bundled dev tools")
    test_parser.add_argument(
        "--nocapture", action="store_true",
        help="Show output from passing tests")
    test_parser.set_defaults(cargo_args=[])
    test_parser.set_defaults(func=cmd_test)

    coverage_parser = subparsers.add_parser("coverage", help="Generate code coverage information")
    coverage_parser.add_argument("--file", help="Show line-by-line coverage for a single file")
    coverage_parser.set_defaults(cargo_args=[])
    coverage_parser.set_defaults(func=cmd_coverage)

    flatpak_src_parser = subparsers.add_parser("flatpak-sources", help="Generate flatpak sources")
    flatpak_src_parser.set_defaults(func=cmd_flatpak_sources)

    flatpak_dl_parser = subparsers.add_parser("flatpak-download", help="Download flatpak sources")
    flatpak_dl_parser.set_defaults(func=cmd_flatpak_download)

    flatpak_build_parser = subparsers.add_parser("flatpak-build", help="Build flatpak package")
    flatpak_build_parser.add_argument(
        "--no-rebuild", help="Skip build step, just repackage", action="store_true")
    flatpak_build_parser.set_defaults(func=cmd_flatpak_build)

    flatpak_parser = subparsers.add_parser(
        "flatpak", help="Generate/download sources and build flatpak package")
    flatpak_parser.add_argument(
        "--no-rebuild", help="Skip build step, just repackage", action="store_true")
    flatpak_parser.set_defaults(func=cmd_flatpak)

    format_parser = subparsers.add_parser(
        "format", help="Format JS, CSS, and HTML templates with Prettier")
    format_parser.set_defaults(func=cmd_format)

    format_check_parser = subparsers.add_parser(
        "format-check", help="Check JS, CSS, and HTML template formatting with Prettier")
    format_check_parser.set_defaults(func=cmd_format_check)

    dist_parser = subparsers.add_parser(
        "dist", help="Installs all required files to the given prefix")
    dist_parser.add_argument("prefix", help="The prefix where to install to")
    dist_parser.set_defaults(func=cmd_dist)

    args, unknown = parser.parse_known_args()
    if args.command == "test" or args.command == "coverage":
        args.cargo_args = unknown
    elif unknown:
        parser.error("unrecognized arguments: {}".format(" ".join(unknown)))
    args.func(args)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as e:
        # Print a concise message for subprocess failures without a Python
        # backtrace. Use shlex.join when the command is a sequence for nicer
        # formatting.
        cmd = shlex.join(e.cmd) if isinstance(e.cmd, (list, tuple)) else e.cmd
        print(f"Command '{cmd}' failed with exit code {e.returncode}", file=sys.stderr)
        # Preserve the subprocess exit code if possible
        try:
            code = int(e.returncode)
        except Exception:
            code = 1
        raise SystemExit(code)
    except KeyboardInterrupt:
        # Respect Ctrl-C with a normal exit code
        raise SystemExit(130)
    except Exception as e:
        # Generic fallback: print the error message only (no traceback)
        print(e, file=sys.stderr)
        raise SystemExit(1)
