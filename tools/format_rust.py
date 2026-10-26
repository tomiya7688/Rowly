#!/usr/bin/env python3
import argparse
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FORMAT_TIMEOUT_SECONDS = 30
SOURCE_DIRECTORIES = {"src", "tests", "examples", "benches"}
EXCLUDED_DIRECTORIES = {"target", "vendor", "generated", ".git", ".worktrees"}


# {
#   責務: [RustSourceFormatter: 明示したRustファイルだけを固定toolchainで整形する。],
#   フィールド: [root: 検査対象repo。 channel: 固定Rust版。 edition: packageのRust edition。]
# }
class RustSourceFormatter:
    # {
    #   責務: [__init__: repoのtoolchainとeditionを読み取る。],
    #   処理: [pathを絶対化し、既存TOMLの指定をrustfmt起動条件として保持する。],
    #   引数: [root: Rust packageとtoolchain設定のあるrepo。],
    #   戻り値: [None: 設定済みformatterを構築する。],
    #   エラー: [設定の欠落・解析失敗は呼出側へ返す。]
    # }
    def __init__(self, root: Path):
        self.root = root.resolve()
        self.channel = tomllib.loads(
            (self.root / "rust-toolchain.toml").read_text(encoding="utf-8")
        )["toolchain"]["channel"]
        self.edition = str(
            tomllib.loads((self.root / "Cargo.toml").read_text(encoding="utf-8"))[
                "package"
            ]["edition"]
        )

    # {
    #   責務: [validate_source: 書換えを許すrepo内のRust sourceを解決する。],
    #   処理: [symlink解決後の範囲・拡張子・除外directory・file存在を検査する。],
    #   引数: [path: repo基準のpathまたは絶対path。],
    #   戻り値: [Path: 検査済みの絶対path。],
    #   エラー: [範囲外・除外対象・Rust file以外はValueError。]
    # }
    def validate_source(self, path: str) -> Path:
        source_path = (self.root / path).resolve()
        relative_path = source_path.relative_to(self.root)
        if (
            source_path.suffix != ".rs"
            or not source_path.is_file()
            or any(part in EXCLUDED_DIRECTORIES for part in relative_path.parts)
            or not (
                relative_path.parts[0] in SOURCE_DIRECTORIES
                or relative_path == Path("build.rs")
            )
        ):
            raise ValueError(f"整形対象のRust sourceではありません: {path}")
        return source_path

    # {
    #   責務: [run_rustfmt: 固定toolchainのrustfmtへbyte列を渡して結果を取得する。],
    #   処理: [repoの設定を使い、標準入力・出力だけで設定取得または整形を行う。],
    #   引数: [arguments: rustfmtの追加引数。 source_bytes: 標準入力に渡す内容。],
    #   戻り値: [bytes: rustfmtの標準出力。],
    #   エラー: [外部command失敗・timeoutを呼出側へ返す。]
    # }
    def run_rustfmt(self, arguments: list[str], source_bytes: bytes = b"") -> bytes:
        format_result = subprocess.run(
            [
                "rustup",
                "run",
                self.channel,
                "rustfmt",
                "--edition",
                self.edition,
                *arguments,
            ],
            input=source_bytes,
            capture_output=True,
            cwd=self.root,
            timeout=FORMAT_TIMEOUT_SECONDS,
            check=True,
        )
        return format_result.stdout

    # {
    #   責務: [format_files: 指定ファイルの整形結果だけを比較または保存する。],
    #   処理: [全対象を検査し、各内容をrustfmtの標準入力へ渡す。全変換成功後に変更対象だけを書く。],
    #   引数: [paths: 明示するRust source一覧。 check: Trueなら比較だけを行う。],
    #   戻り値: [list[Path]: 整形差分のあるfile。check時は未整形fileの一覧。],
    #   副作用: [check以外では指定fileだけを書き換える。子moduleは起動引数に渡さない。],
    #   エラー: [対象不正・解析失敗・timeout・変換中の外部編集では書換え前に停止する。]
    # }
    def format_files(self, paths: list[str], check: bool = False) -> list[Path]:
        source_paths = list(dict.fromkeys(self.validate_source(path) for path in paths))
        rustfmt_config = tomllib.loads(
            self.run_rustfmt(
                ["--print-config", "current", str(self.root / "Cargo.toml")]
            ).decode("utf-8")
        )
        formatted_sources: list[tuple[Path, bytes, bytes]] = []
        for source_path in source_paths:
            original_bytes = source_path.read_bytes()
            format_arguments = ["--emit", "stdout"]
            # stdinのAutoはLFになるため、最初の改行形式を明示する。repoの明示設定は優先する。
            if rustfmt_config.get("newline_style", "Auto") == "Auto":
                first_line = original_bytes.partition(b"\n")[0]
                newline_style = "Windows" if first_line.endswith(b"\r") else "Unix"
                format_arguments.extend(["--config", f"newline_style={newline_style}"])
            # 標準入出力ならrustfmt自身はfileを書かず、未指定の子moduleへ再帰しない。
            formatted_bytes = self.run_rustfmt(format_arguments, original_bytes)
            formatted_bytes.decode("utf-8")
            formatted_sources.append((source_path, original_bytes, formatted_bytes))
        # 変換を待っている間の他者の編集を上書きしない。
        for source_path, original_bytes, _ in formatted_sources:
            if source_path.read_bytes() != original_bytes:
                raise RuntimeError(f"整形中にfileが変更されました: {source_path}")
        changed_sources: list[Path] = []
        for source_path, original_bytes, formatted_bytes in formatted_sources:
            if original_bytes != formatted_bytes:
                changed_sources.append(source_path)
                if not check:
                    source_path.write_bytes(formatted_bytes)
        return changed_sources


# {
#   責務: [main: 指定sourceの整形または書換えなしの検査を実行する。],
#   処理: [引数を解析して共通formatterへ渡し、変更fileと失敗理由を表示する。],
#   引数: [],
#   戻り値: [int: 成功0、未整形または失敗1。],
#   エラー: [外部command・設定・I/O失敗を標準エラーへ表示して非0終了する。]
# }
def main() -> int:
    parser = argparse.ArgumentParser(description="指定したRust sourceだけを整形します")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("paths", nargs="+")
    args = parser.parse_args()
    try:
        changed_sources = RustSourceFormatter(ROOT).format_files(args.paths, args.check)
        for source_path in changed_sources:
            print(source_path.relative_to(ROOT))
        return int(args.check and bool(changed_sources))
    except (
        OSError,
        ValueError,
        KeyError,
        RuntimeError,
        subprocess.SubprocessError,
    ) as error:
        print(f"Rust sourceの整形に失敗しました: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError):
            print(error.stderr.decode("utf-8", errors="replace"), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
