#!/usr/bin/env python3
import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path


# {
#   責務: [executable_name: 実行platformで使うexecutable suffixをnameへ付ける。]
#   引数: [name: suffix付与前のexecutable名。]
#   戻り値: [str: platform向けexecutable名。]
# }
def executable_name(name: str) -> str:
    return f"{name}.exe" if sys.platform == "win32" else name


# {
#   責務: [run: packaged smoke test用subprocessを指定環境で実行し、失敗出力を報告する。]
#   引数: [command: 起動するargv。 env: child processへ渡すenvironment。]
#   戻り値: [None: 成功時は値を返さない。]
#   副作用: [外部Rowly processを起動する。]
#   エラー: [終了codeが0以外ならstdout/stderrを含むSystemExitを送出する。]
# }
def run(command: list[str], env: dict[str, str]) -> None:
    result = subprocess.run(command, env=env, text=True, capture_output=True)
    if result.returncode != 0:
        raise SystemExit(
            f"command failed ({result.returncode}): {command}\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )


# {
#   責務: [main: system Pythonや未同梱backendへ依存せず配布RowlyでXLSX import/export round tripを確認する。]
#   処理: [子process環境を制限し、fixtureをCSVへimport、XLSXへexport、CSVへ再importして生成物を検査する。]
#   戻り値: [None: 成功したCSV pathをstdoutへ表示する。]
#   引数: [CLI: 配布directory・Excel fixture・一時work directoryを受け取る。]
#   副作用: [work directoryを削除・再作成し、import結果CSVとround-trip workbookを生成する。]
#   エラー: [配布executable不足またはsubprocess失敗時はSystemExitを送出する。]
# }
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--distribution", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    args = parser.parse_args()

    distribution = args.distribution.resolve()
    fixture = args.fixture.resolve()
    work = args.work_dir.resolve()
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)

    rowly = distribution / executable_name("rowly")
    bridge = distribution / executable_name("rowly-excel-bridge")
    if not rowly.is_file() or not bridge.is_file():
        raise SystemExit("distribution does not contain Rowly and bundled Excel backend")

    # system Python へ偶然フォールバックしても成功できない子環境にする。
    env = os.environ.copy()
    env.pop("ROWLY_PYTHON", None)
    env.pop("PYTHONHOME", None)
    env.pop("PYTHONPATH", None)
    env["PATH"] = str(distribution)
    env["PYTHONUTF8"] = "0"
    env["PYTHONIOENCODING"] = "ascii"

    imported = work / "imported.csv"
    roundtrip_xlsx = work / "roundtrip.xlsx"
    roundtrip_csv = work / "roundtrip.csv"
    sheet = "日本語 シート"

    run(
        [str(rowly), "excel", "import", str(fixture), str(imported), sheet],
        env,
    )
    run(
        [str(rowly), "excel", "export", str(imported), str(roundtrip_xlsx), sheet],
        env,
    )
    run(
        [str(rowly), "excel", "import", str(roundtrip_xlsx), str(roundtrip_csv), sheet],
        env,
    )

    for path in (imported, roundtrip_xlsx, roundtrip_csv):
        if not path.is_file():
            raise SystemExit(f"packaged smoke test did not create {path}")

    print(imported)
    print(roundtrip_csv)


if __name__ == "__main__":
    main()
