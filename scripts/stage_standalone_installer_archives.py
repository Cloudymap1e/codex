#!/usr/bin/env python3
"""Stage standalone installer archives for Codex releases."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import tarfile
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
INSTALL_NATIVE_DEPS = REPO_ROOT / "codex-cli" / "scripts" / "install_native_deps.py"
WORKFLOW_NAME = ".github/workflows/rust-release.yml"

CODEX_PLATFORM_PACKAGES: dict[str, dict[str, str]] = {
    "codex-linux-x64": {
        "platform_tag": "linux-x64",
        "target_triple": "x86_64-unknown-linux-musl",
        "os": "linux",
    },
    "codex-linux-arm64": {
        "platform_tag": "linux-arm64",
        "target_triple": "aarch64-unknown-linux-musl",
        "os": "linux",
    },
    "codex-darwin-x64": {
        "platform_tag": "darwin-x64",
        "target_triple": "x86_64-apple-darwin",
        "os": "darwin",
    },
    "codex-darwin-arm64": {
        "platform_tag": "darwin-arm64",
        "target_triple": "aarch64-apple-darwin",
        "os": "darwin",
    },
    "codex-win32-x64": {
        "platform_tag": "win32-x64",
        "target_triple": "x86_64-pc-windows-msvc",
        "os": "win32",
    },
    "codex-win32-arm64": {
        "platform_tag": "win32-arm64",
        "target_triple": "aarch64-pc-windows-msvc",
        "os": "win32",
    },
}
STANDALONE_NATIVE_COMPONENTS = (
    "codex",
    "codex-command-runner",
    "codex-windows-sandbox-setup",
    "rg",
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--release-version",
        required=True,
        help="Version to stage (e.g. 0.1.0 or 0.1.0-alpha.1).",
    )
    parser.add_argument(
        "--vendor-src",
        type=Path,
        help="Directory containing native binaries under vendor/<target>.",
    )
    parser.add_argument(
        "--workflow-url",
        help=(
            "Optional workflow URL to download native artifacts from when --vendor-src "
            "is not provided."
        ),
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=REPO_ROOT / "dist" / "installer",
        help="Directory where standalone archives should be written.",
    )
    parser.add_argument(
        "--package",
        dest="packages",
        action="append",
        choices=sorted(CODEX_PLATFORM_PACKAGES),
        help=(
            "Codex platform package to stage. May be provided multiple times. "
            "Defaults to all platform packages."
        ),
    )
    args = parser.parse_args()
    if args.vendor_src is None and not args.workflow_url:
        parser.error("Provide either --vendor-src or --workflow-url.")
    return args


def archive_name(platform_tag: str, version: str) -> str:
    return f"codex-standalone-{platform_tag}-{version}.tar.gz"


def copy_executable(source: Path, destination: Path) -> None:
    if not source.exists():
        raise RuntimeError(f"Missing standalone installer archive input: {source}")

    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    destination.chmod(0o755)


def stage_target(vendor_src: Path, staging_dir: Path, target: str, is_windows: bool) -> None:
    target_root = vendor_src / target
    codex_root = target_root / "codex"
    path_root = target_root / "path"
    resources_dir = staging_dir / "codex-resources"
    resources_dir.mkdir(parents=True, exist_ok=True)

    if is_windows:
        copy_executable(codex_root / "codex.exe", staging_dir / "codex.exe")
        copy_executable(
            codex_root / "codex-command-runner.exe",
            resources_dir / "codex-command-runner.exe",
        )
        copy_executable(
            codex_root / "codex-windows-sandbox-setup.exe",
            resources_dir / "codex-windows-sandbox-setup.exe",
        )
        copy_executable(path_root / "rg.exe", resources_dir / "rg.exe")
        return

    copy_executable(codex_root / "codex", staging_dir / "codex")
    copy_executable(path_root / "rg", resources_dir / "rg")


def write_archive(staging_dir: Path, output_path: Path) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(output_path, "w:gz") as archive:
        for path in sorted(staging_dir.rglob("*")):
            archive.add(path, arcname=path.relative_to(staging_dir), recursive=False)


def resolve_release_workflow(version: str) -> dict:
    stdout = subprocess.check_output(
        [
            "gh",
            "run",
            "list",
            "--branch",
            f"rust-v{version}",
            "--json",
            "workflowName,url,headSha",
            "--workflow",
            WORKFLOW_NAME,
            "--jq",
            "first(.[])",
        ],
        cwd=REPO_ROOT,
        text=True,
    )
    workflow = json.loads(stdout or "null")
    if not workflow:
        raise RuntimeError(f"Unable to find rust-release workflow for version {version}.")
    return workflow


def resolve_workflow_url(version: str, override: str | None) -> str:
    if override:
        return override
    workflow = resolve_release_workflow(version)
    return workflow["url"]


def install_native_components(workflow_url: str, vendor_root: Path) -> Path:
    cmd = [str(INSTALL_NATIVE_DEPS), "--workflow-url", workflow_url]
    for component in STANDALONE_NATIVE_COMPONENTS:
        cmd.extend(["--component", component])
    cmd.append(str(vendor_root))
    subprocess.run(cmd, cwd=REPO_ROOT, check=True)
    return vendor_root / "vendor"


def main() -> int:
    args = parse_args()
    output_dir = args.output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    packages = args.packages or sorted(CODEX_PLATFORM_PACKAGES)
    runner_temp = Path(os.environ.get("RUNNER_TEMP", tempfile.gettempdir()))

    vendor_temp_root: Path | None = None
    try:
        if args.vendor_src is not None:
            vendor_src = args.vendor_src.resolve()
        else:
            workflow_url = resolve_workflow_url(args.release_version, args.workflow_url)
            vendor_temp_root = Path(
                tempfile.mkdtemp(prefix="standalone-native-", dir=runner_temp)
            )
            vendor_src = install_native_components(workflow_url, vendor_temp_root)

        for package in sorted(set(packages)):
            package_config = CODEX_PLATFORM_PACKAGES[package]
            platform_tag = package_config["platform_tag"]
            target = package_config["target_triple"]
            is_windows = package_config["os"] == "win32"
            output_path = output_dir / archive_name(platform_tag, args.release_version)

            with tempfile.TemporaryDirectory(
                prefix=f"codex-standalone-{platform_tag}-"
            ) as staging_dir_str:
                staging_dir = Path(staging_dir_str)
                stage_target(vendor_src, staging_dir, target, is_windows)
                write_archive(staging_dir, output_path)

            print(f"Staged standalone installer archive at {output_path}")
    finally:
        if vendor_temp_root is not None:
            shutil.rmtree(vendor_temp_root, ignore_errors=True)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
