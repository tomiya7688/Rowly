import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from format_rust import ROOT, RustSourceFormatter

SCRATCH_ROOT = Path(
    os.environ.get("ROWLY_FORMAT_TEST_ROOT", ROOT / "target" / "formatter-tests")
)


# {
#   責務: [RustSourceFormatterTests: 固定stableで整形範囲と失敗時の保持を検証する。],
#   フィールド: [directory: 明示先に作る一時fixture。 root: fixture repo。 formatter: 検証対象。]
# }
class RustSourceFormatterTests(unittest.TestCase):
    # {
    #   責務: [setUp: システムTempを使わず親・子moduleのfixtureを準備する。],
    #   処理: [指定scratch内へ最小repoを作り、実repoの固定toolchainをコピーする。],
    #   引数: [self: test状態。],
    #   戻り値: [None: testごとに独立したfixtureを用意する。]
    # }
    def setUp(self):
        SCRATCH_ROOT.mkdir(parents=True, exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(dir=SCRATCH_ROOT)
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name) / "repo"
        self.root.mkdir()
        (self.root / "src").mkdir()
        (self.root / "Cargo.toml").write_text('[package]\nedition = "2024"\n')
        (self.root / "rust-toolchain.toml").write_bytes(
            (ROOT / "rust-toolchain.toml").read_bytes()
        )
        self.parent = self.root / "src" / "lib.rs"
        self.child = self.root / "src" / "child.rs"
        self.parent.write_text(
            'mod child;\npub fn parent(){let _text="日本語";}\n', encoding="utf-8"
        )
        self.child.write_text("pub fn child(){let _value=2;}\n", encoding="utf-8")
        self.formatter = RustSourceFormatter(self.root)

    # {
    #   責務: [test_parent_only_preserves_child: 親file指定で子fileが変わらないことを検証する。],
    #   処理: [未整形の子を保持したまま親だけを整形し、UTF8と再実行時の一致を確認する。],
    #   引数: [self: fixture。],
    #   戻り値: [None: 範囲・文字コード・再現性のassertionを完了する。]
    # }
    def test_parent_only_preserves_child(self):
        child_before = self.child.read_bytes()
        changed = self.formatter.format_files(["src/lib.rs"])
        self.assertEqual(changed, [self.parent])
        self.assertEqual(self.child.read_bytes(), child_before)
        self.assertIn('"日本語"', self.parent.read_text(encoding="utf-8"))
        self.assertEqual(self.formatter.format_files(["src/lib.rs"], check=True), [])

    # {
    #   責務: [test_check_never_writes: 未整形の検査がfileを書き換えないことを検証する。],
    #   処理: [親だけをcheckし、親・子のbyte列が変わらないことを比較する。],
    #   引数: [self: fixture。],
    #   戻り値: [None: 未整形検出と非書換えを確認する。]
    # }
    def test_check_never_writes(self):
        before = (self.parent.read_bytes(), self.child.read_bytes())
        self.assertEqual(
            self.formatter.format_files(["src/lib.rs"], check=True), [self.parent]
        )
        self.assertEqual((self.parent.read_bytes(), self.child.read_bytes()), before)

    # {
    #   責務: [test_explicit_children_are_formatted_once: 明示した子fileも整形できることを検証する。],
    #   処理: [親・子と重複pathを渡し、変更一覧の重複がなく両fileが整形されることを確認する。],
    #   引数: [self: fixture。],
    #   戻り値: [None: 明示範囲の整形と重複排除を確認する。]
    # }
    def test_explicit_children_are_formatted_once(self):
        paths = ["src/lib.rs", "src/child.rs", "src/lib.rs"]
        self.assertEqual(self.formatter.format_files(paths), [self.parent, self.child])
        self.assertEqual(self.formatter.format_files(paths, check=True), [])

    # {
    #   責務: [test_formatted_lf_and_crlf_remain_unchanged: 両改行形式で整形済みfileを保持する。],
    #   処理: [同じRust codeをLF・CRLFで保存し、checkと通常整形が差分を作らないことを確認する。],
    #   引数: [self: fixture。],
    #   戻り値: [None: 改行形式だけを理由に整形済みfileを変更しないことを確認する。]
    # }
    def test_formatted_lf_and_crlf_remain_unchanged(self):
        formatted = b"pub fn child() {\n    let _value = 2;\n}\n"
        for newline in [b"\n", b"\r\n"]:
            with self.subTest(newline=newline):
                original = formatted.replace(b"\n", newline)
                self.child.write_bytes(original)
                self.assertEqual(
                    self.formatter.format_files(["src/child.rs"], check=True), []
                )
                self.assertEqual(self.formatter.format_files(["src/child.rs"]), [])
                self.assertEqual(self.child.read_bytes(), original)

    # {
    #   責務: [test_repository_rustfmt_config_is_loaded: repo rootのstable設定を標準入力でも使う。],
    #   処理: [hard_tabs設定をrootへ置き、整形結果のindentと再checkを確認する。],
    #   引数: [self: fixture。],
    #   戻り値: [None: cwdのrepo設定が無視されないことを確認する。]
    # }
    def test_repository_rustfmt_config_is_loaded(self):
        (self.root / "rustfmt.toml").write_text("hard_tabs = true\n")
        self.formatter.format_files(["src/child.rs"])
        self.assertIn(b"\tlet _value = 2;", self.child.read_bytes())
        self.assertEqual(self.formatter.format_files(["src/child.rs"], check=True), [])

    # {
    #   責務: [test_explicit_newline_config_takes_priority: repoで指定した改行形式を優先する。],
    #   処理: [Unix設定を置き、CRLFの入力がLFへ整形されることを確認する。],
    #   引数: [self: fixture。],
    #   戻り値: [None: Auto時の保持が明示設定を上書きしないことを確認する。]
    # }
    def test_explicit_newline_config_takes_priority(self):
        (self.root / "rustfmt.toml").write_text('newline_style = "Unix"\n')
        self.child.write_bytes(b"pub fn child() {\r\n    let _value = 2;\r\n}\r\n")
        self.assertEqual(self.formatter.format_files(["src/child.rs"]), [self.child])
        self.assertNotIn(b"\r\n", self.child.read_bytes())
        self.assertEqual(self.formatter.format_files(["src/child.rs"], check=True), [])

    # {
    #   責務: [test_later_parse_failure_preserves_all_files: 変換失敗前の対象も保存されないことを検証する。],
    #   処理: [二番目のfileを構文不正にし、最初の整形結果が書かれないことを確認する。],
    #   引数: [self: fixture。],
    #   戻り値: [None: 解析失敗時の保持を確認する。]
    # }
    def test_later_parse_failure_preserves_all_files(self):
        self.child.write_text("fn broken(\n", encoding="utf-8")
        before = (self.parent.read_bytes(), self.child.read_bytes())
        with self.assertRaises(subprocess.CalledProcessError):
            self.formatter.format_files(["src/lib.rs", "src/child.rs"])
        self.assertEqual((self.parent.read_bytes(), self.child.read_bytes()), before)

    # {
    #   責務: [test_invalid_targets_preserve_sources: 範囲外や生成物の指定を拒否する。],
    #   処理: [親fileと不正pathを一緒に渡し、対象検査中に親が書かれないことを確認する。],
    #   引数: [self: fixture。],
    #   戻り値: [None: 全不正pathの拒否と元byte列を確認する。]
    # }
    def test_invalid_targets_preserve_sources(self):
        (self.root / "target").mkdir()
        (self.root / "target" / "generated.rs").write_text("fn generated(){}\n")
        (self.root.parent / "outside.rs").write_text("fn outside(){}\n")
        before = self.parent.read_bytes()
        for invalid in [
            "../outside.rs",
            "target/generated.rs",
            "Cargo.toml",
            "src/missing.rs",
        ]:
            with self.subTest(path=invalid):
                with self.assertRaises(ValueError):
                    self.formatter.format_files(["src/lib.rs", invalid])
                self.assertEqual(self.parent.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
