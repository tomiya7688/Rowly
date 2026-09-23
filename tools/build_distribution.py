#!/usr/bin/env python3
import argparse
import importlib.metadata as metadata
import json
import platform
import shutil
import subprocess
import sys
import sysconfig
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EXPECTED_PYTHON = (3, 12, 14)
BRIDGE_NAME = "rowly-excel-bridge.exe" if sys.platform == "win32" else "rowly-excel-bridge"
ROWLY_NAME = "rowly.exe" if sys.platform == "win32" else "rowly"


def run(*args: str) -> None:
    subprocess.run(args, cwd=ROOT, check=True)


def copy_distribution_licenses(output: Path) -> list[str]:
    licenses = output / "licenses"
    licenses.mkdir(parents=True, exist_ok=True)
    copied: list[str] = []

    for package in ("openpyxl", "et-xmlfile", "pyinstaller"):
        distribution = metadata.distribution(package)
        for entry in distribution.files or ():
            name = Path(str(entry)).name.lower()
            if not (
                name.startswith("license")
                or name.startswith("copying")
                or name.startswith("notice")
            ):
                continue
            source = Path(distribution.locate_file(entry))
            if not source.is_file():
                continue
            destination = licenses / f"{package}-{source.name}"
            shutil.copy2(source, destination)
            copied.append(destination.name)

    for base in {Path(sys.base_prefix), Path(sys.prefix)}:
        for name in ("LICENSE.txt", "LICENSE", "LICENSE.md"):
            source = base / name
            if source.is_file():
                destination = licenses / f"python-{source.name}"
                shutil.copy2(source, destination)
                copied.append(destination.name)
                return copied
    return copied


def write_notices(output: Path, copied_licenses: list[str]) -> None:
    versions = {
        "python": platform.python_version(),
        "openpyxl": metadata.version("openpyxl"),
        "et-xmlfile": metadata.version("et-xmlfile"),
        "pyinstaller": metadata.version("pyinstaller"),
    }
    notice = f"""# Rowly 配布物の第三者コンポーネント

Excel backend はユーザー環境の Python に依存しないよう、PyInstaller で Python runtime と
openpyxl を自己完結 executable にしています。

- Python {versions["python"]} — Python Software Foundation License
- openpyxl {versions["openpyxl"]} — MIT License
- et-xmlfile {versions["et-xmlfile"]} — MIT License
- PyInstaller {versions["pyinstaller"]} — GPL-2.0-or-later with the PyInstaller bootloader exception

ライセンス本文としてビルド環境から取得できたファイルは `licenses/` に同梱しています。
Python: https://www.python.org/psf/license/
openpyxl: https://foss.heptapod.net/openpyxl/openpyxl
et-xmlfile: https://foss.heptapod.net/openpyxl/et_xmlfile
PyInstaller: https://pyinstaller.org/
"""
    (output / "THIRD_PARTY_NOTICES.md").write_text(notice, encoding="utf-8")

    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    manifest = {
        "format": "rowly-distribution",
        "version": 1,
        "rowly_version": cargo["package"]["version"],
        "excel_backend": BRIDGE_NAME,
        "python": versions["python"],
        "openpyxl": versions["openpyxl"],
        "et_xmlfile": versions["et-xmlfile"],
        "pyinstaller": versions["pyinstaller"],
        "license_files": sorted(copied_licenses),
    }
    (output / "rowly-distribution.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=ROOT / "dist" / "rowly")
    parser.add_argument(
        "--allow-python-version-mismatch",
        action="store_true",
        help="開発用。正式配布相当ビルドでは使用しない。",
    )
    args = parser.parse_args()

    current = sys.version_info[:3]
    if current != EXPECTED_PYTHON and not args.allow_python_version_mismatch:
        expected = ".".join(map(str, EXPECTED_PYTHON))
        actual = ".".join(map(str, current))
        raise SystemExit(
            f"distribution build requires Python {expected}; current interpreter is {actual}"
        )

    output = args.output.resolve()
    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True)

    build_root = ROOT / "target" / "excel-backend-package"
    if build_root.exists():
        shutil.rmtree(build_root)
    bridge_dist = build_root / "dist"

    run("cargo", "build", "--release")
    run(
        sys.executable,
        "-m",
        "PyInstaller",
        "--clean",
        "--noconfirm",
        "--onefile",
        "--noupx",
        "--name",
        "rowly-excel-bridge",
        "--distpath",
        str(bridge_dist),
        "--workpath",
        str(build_root / "work"),
        "--specpath",
        str(build_root / "spec"),
        "--collect-all",
        "openpyxl",
        str(ROOT / "python" / "excel_bridge.py"),
    )

    rowly = ROOT / "target" / "release" / ROWLY_NAME
    bridge = bridge_dist / BRIDGE_NAME
    if not rowly.is_file():
        raise SystemExit(f"Rowly release executable was not created: {rowly}")
    if not bridge.is_file():
        raise SystemExit(f"bundled Excel backend was not created: {bridge}")

    shutil.copy2(rowly, output / ROWLY_NAME)
    shutil.copy2(bridge, output / BRIDGE_NAME)
    copied_licenses = copy_distribution_licenses(output)
    write_notices(output, copied_licenses)

    print(output)


if __name__ == "__main__":
    main()
