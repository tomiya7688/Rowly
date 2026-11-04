# ビルド・配布・サプライチェーンの安全性

Rowlyのビルドを再現可能にし、配布物の真正性を検証できるようにするため、MSRV、Cargo.lock、Python同梱物の更新、署名、PR前チェック、リリース前確認、設計原則の公開を一つの設計にまとめます。

- 状態: 提案: Issue本文に基づく設計。利用者未承認
- 確認日: 2026-11-05
- 対象Issue:
  - #51 Windowsを公式対応OSとしUbuntuでも動作可能にする（親）
  - #220 [P1] 宣言済みMSRV Rust 1.85でRowly本体がコンパイルできるようにする
  - #221 [P1] Rust 1.85 MSRVジョブをCIへ追加してrust-versionとの不一致を防ぐ
  - #222 [P1] Cargo.lockを追跡しCIと配布ビルドを--lockedで再現可能にする
  - #224 [P0] Python証明書インシデント対応として pip / certifi / uv / sigstore を更新する
  - #225 [P0] 全配布バイナリと成果物にコード署名・Artifact Attestationを導入する
  - #227 [P1][開発運用] PR前のローカル簡易自己チェックを整備しCIと分担する
  - #72 1.0.0前にKadoka UI / ユーザビリティテストを実施する
  - #81 Rowly Design Principlesを文書化しCC BY 4.0で公開する
- 読む人: ビルド、CI、配布物、開発手順を実装する人

## 背景と目的

現状では次の問題があります。`Cargo.toml` は `rust-version = "1.85"` を宣言していますが、CIはstable一本です。そのため、新しいRustでだけ通る構文がMSRVを壊しても検出されません。`Cargo.lock` が未追跡のため、同じcommitでも依存解決の時期によりビルド内容が変わります。配布物は未署名で、真正性を利用者が検証できません。PR前にCIの失敗に気づく手段が単一の入口として整っていません。

目的は次の五つです。

1. Windowsを公式対応、Ubuntuを準対応として、両OSのビルドと起動の確認を維持する（#51の方針は #51 を正本とし、CI仕様は [CI.md](CI.md) を正本とします）。
2. 宣言したMSRVとロックファイルをCIで強制する。
3. 同梱するPython周辺ツールの信頼経路を更新し、確認結果を残す。
4. 利用者へ渡る最終成果物の作成元をAttestationと署名で検証可能にする。
5. 公開前の人手確認と設計原則文書の公開手順を決める。

利用者に環境構築を求めません。Python、openpyxl、証明書束は配布物へ同梱し、本書の手順は開発者とCIだけが実行します。

## 現行実装との差

確認した対象は `Cargo.toml`、`.github/workflows/ci.yml`、`python/*.txt`、`tools/build_distribution.py`、`tools/verify_packaged_excel.py`、`docs/CI.md`、`docs/DISTRIBUTION.md`、`docs/LICENSING.md` です。

| 項目 | 実装済み | 未実装 |
| --- | --- | --- |
| OS | `rust-matrix` が `ubuntu-latest` と `windows-latest` でfmt、clippy、build、test、配布相当ビルドを実行する。集約ジョブ `rust` が両OSの成功を要求する | GUI実操作は未検証（[CI.md](CI.md) の保証の境界） |
| MSRV | `Cargo.toml` に `rust-version = "1.85"` と `edition = "2024"` | CIはstableのみ。`src/` に `&& let` を含むlet-chain形式が5箇所あり、1.85で `E0658` になる（Issue #220の報告と一致） |
| Cargo.lock | ローカルに生成済み | `git ls-files Cargo.lock` が空で未追跡。CIのCargoコマンドに `--locked` が無い |
| Python同梱 | `python/requirements.txt`（openpyxl、et-xmlfile）と `python/build-requirements.txt`（PyInstaller 6.22.3）が完全固定。Python 3.12.10 | pip、certifi、uv、sigstoreの下限指定と、実CA束の確認記録が無い |
| 署名 | 配布物のCI artifact保存（`actions/upload-artifact`） | SHA-256、Artifact Attestation、Authenticode署名、検証ジョブが無い。`ci.yml` の権限は `contents: read` のみ |
| ローカル検査 | 自己レビュー規則は `docs/DEVELOPMENT_WORKFLOW.md` にある | 単一入口のコマンドが無い（`check_local` に相当するファイルは存在しない） |
| 設計原則 | `docs/DATA_MODEL_PRINCIPLES.md` と `docs/WHY_ROWLY.md` | 公開用Design PrinciplesのCC BY 4.0表示は未記録 |

## 設計方針

### 守る契約

- リリース成果物は、`--locked` でビルドした一つのコミットから、CIだけが生成します。開発者のPCで作った成果物は公開しません。
- 署名またはAttestationが付けられない、あるいは検証に失敗した成果物はreleaseしません。
- 秘密鍵、PFX、keystoreはリポジトリへ置きません。
- CIの合否規則は一か所に定義します。ローカル入口はCIと同じ内部処理を呼び、別の合否規則を作りません。

### MSRV（#220、#221）

1. #220で、1.85で使えない構文を互換表現へ書き換えます。挙動変更は混ぜません。MSRVは引き上げません。引き上げる場合は別の仕様変更として理由、配布影響、対応toolchainを記録します。
2. #221で、ジョブ `msrv` を追加し `cargo +1.85.0 check --all-targets --all-features --locked` を実行します。実行OSはUbuntuを推奨します（採用理由は未決事項1）。
3. 集約ジョブ `rust` の `needs` に `msrv` を加え、失敗・キャンセル・スキップを成功とみなしません。
4. [CI.md](CI.md) にMSRV gateを追記します。`rust-version` を変更するPRは、`msrv` ジョブの版と文書を同時に更新します。

### Cargo.lock（#222）

Rowlyは配布するapplicationなので `Cargo.lock` を追跡します。CIのbuild、check、test、clippy、配布ビルド（`tools/build_distribution.py` が呼ぶcargo）に `--locked` を付けます。依存更新は `Cargo.toml` と `Cargo.lock` の差分としてレビューします。これによりstableジョブとMSRVジョブが同じ依存グラフを検証します。1.85で解決できない新しい依存が入った場合は `msrv` ジョブが失敗して検出します。Pythonのロック方針は本件に混ぜません。

### Python周辺の信頼経路（#224）

- 下限は `pip >= 26.2.2`、`certifi >= 2026.10.19`、uvとsigstoreを使う場合は `uv >= 0.13.4`、`sigstore >= 4.5.1` です。現行のリポジトリでは、`python/` にuvとsigstoreの記述は見つかっていません。使用していないことを確認して記録します。
- CIでは `python -m pip install` の前にpipを更新し、`python -m pip --version` と `python -c "import certifi; print(certifi.__version__, certifi.where())"` をログへ出します。
- 更新だけで完了にしません。PyInstallerが同梱するCA束（配布物 `rowly-excel-bridge` の中身）に古いcertifiが残っていないか、ビルド後に確認します。確認結果は、対象のPython/成果物、参照するCA束、versionを `rowly-distribution.json` ではなくIssue #224のコメントへ残します。
- 問題の対象証明書チェーンが旧CA束で信頼されないことの確認方法は、Issue本文に具体的な手順が無いため未決事項2とします。

### 署名とAttestation（#225）

1. releaseジョブで、ZIPまたはTAR.GZなど利用者へ渡る最終成果物ごとにSHA-256を生成し、`SHA256SUMS` にまとめます。
2. GitHub Actionsのkeyless OIDCで、最終成果物と `SHA256SUMS` にArtifact Attestationを付与します。このジョブにだけ `id-token: write` と `attestations: write` を許可します。通常のCIは `contents: read` を維持します。
3. Windowsの `.exe` は、信頼されたCode Signing CertificateでAuthenticode署名し、RFC 3161 timestampを付けます。署名後に成果物を組み立て、Attestationは署名後の最終成果物に付けます。自己署名証明書を一般公開の最終解にしません。
4. releaseジョブ内で `gh attestation verify` とAuthenticode検証を実行し、失敗時はreleaseを止めます。
5. README とRelease notesに利用者向けの検証手順を書きます。

採用しなかった案は、チェックサムのみで終える案です。作成元（誰が、どのworkflowから）を検証できないため、#225の要求を満たしません。macOS、Android、containerは、現状Rowlyの配布対象ではありません（[CI.md](CI.md) でmacOS CIは対象外）。配布対象に加える時点で、#225の該当節を適用します。

### PR前ローカル簡易自己チェック（#227）

単一の公開入口として `python tools/check_local.py` を新設することを推奨します。入口は変更ファイルから検査を自動選択し、結果をPASS/FAIL/SKIP/UNVERIFIEDの一つの要約で出します。必須検査の失敗、開始失敗、対象0件はFAILにします。

| 担当 | 内容 |
| --- | --- |
| ローカル入口（短時間、ネットワーク不要、読み取り専用） | `git diff --check`、`cargo fmt --all -- --check`、変更に関係する `cargo test`、`python -m py_compile` による `python/` と `tools/` の構文検査 |
| CIに残す | 複数OS、`clippy -D warnings` の全ターゲット、MSRV、配布ビルド、packaged XLSX検証、署名・Attestation |

入口は暗黙の依存導入、自動修正、`git add` を行いません。CIは可能な範囲で同じ内部関数を呼び、合否規則を二重定義しません。

### 1.0.0前のユーザビリティテスト（#72）

Windows版のrelease candidateを、代表的なCSVとプロジェクトで実操作します。確認範囲はIssue #72の一覧に従います。Kadokaの確認を、AIが代行して完了にしません。指摘は重要度別に整理し、コード修正が必要なものは1件1実装Issueへ切り出します。blockerとcriticalが未処理のままならリリースしません。テスト結果は日本語で記録します。

### 設計原則文書の公開（#81）

日本語版を正本とし、コードのApache-2.0と別に、文書をCC BY 4.0で公開します。中心原則（Canonical Data、Editor、Viewer、フォークへの相互運用のお願い）の内容は [DATA_MODEL_PRINCIPLES.md](DATA_MODEL_PRINCIPLES.md) と [WHY_ROWLY.md](WHY_ROWLY.md) を正本とし、本書では複製しません。ライセンスの記載先は [LICENSING.md](LICENSING.md) の「文書」節です。

## 利用者から見た操作

利用者が行う操作は、公式サイトまたはReleaseから配布物を取得し、検証コマンドで真正性を確認することだけです。例は `gh attestation verify <成果物> --repo tomiya7688/Rowly` です。Windowsではファイルのプロパティで署名者を確認できます。検証に失敗した場合は、実行しないでください。Pythonやopensslの導入は求めません。

GUIコマンドとCommand IDは追加しません。#72の確認では、全GUI操作がAlt KeyTipsから到達できること（[CI.md](CI.md) のKeyTipsゲート）を、人手で再確認します。

開発者は `python tools/check_local.py` を実行します。FAILのとき、要約が原因と再実行方法を表示します。

## 永続化と互換性

- CSV本文、`.rwprj`、generated init DSLは変更しません。本書の対象はビルドと配布だけです。
- `Cargo.lock` はリポジトリへ追加します。`rowly-distribution.json` には、既存のPython、openpyxl、et-xmlfile、PyInstallerのversionに加えて、pipとcertifiのversionを追記することを推奨します（配布仕様の正本は [DISTRIBUTION.md](DISTRIBUTION.md)）。
- 署名情報はGitHubのAttestationとRelease assetに置き、リポジトリへは保存しません。

## 安全性と失敗時の挙動

- `--locked` により、ロックファイルと `Cargo.toml` が不一致ならビルドが失敗します。
- 署名、Attestation、検証のどれかが失敗したら、releaseジョブは成果物を公開せずに失敗します。部分的に公開された状態を避けるため、公開は検証の後の最終ステップでだけ行います。
- 署名用の秘密情報は、GitHubの保護されたenvironmentのsecretまたは外部の署名サービスに置きます。ログへ出さず、forkからのPRでは参照できないようにします。
- `check_local.py` の対象0件、開始失敗、依存不足は、成功扱いにしません。
- 既存の古い成果物のうち再現できないものは、「未署名・検証不可」であることをRelease notesへ明記します（#225）。

## 実装順序と依存

| 順 | 単位 | 先行条件 | 並行可否 |
| --- | --- | --- | --- |
| 1 | #224 pip/certifi等の更新と確認（P0） | なし | 並行可 |
| 1 | #225 署名・Attestation基盤（P0） | なし。Windows署名証明書の入手が前提（未決事項3） | #224と並行可 |
| 1 | #222 Cargo.lock追跡と `--locked` | なし | #220と並行可 |
| 1 | #220 1.85互換への書き換え | なし | #222と並行可 |
| 2 | #221 MSRVジョブ追加 | #220と#222 | 単独 |
| 2 | #227 `tools/check_local.py` | #222（`--locked`の規則を共有） | #221と並行可 |
| 3 | #81 公開用Design Principlesの不足確認 | なし | いつでも並行可 |
| 4 | #72 ユーザビリティテスト | GUI実装完了とrelease candidate（Issueの先行Issue群） | 最後 |

#51は親Issueです。子の#56と#61の統合状況を確認して追跡し、本書の項目で閉じません。

## 受け入れ条件と検証

| Issue | 受け入れ条件との対応 | 検証の種類 |
| --- | --- | --- |
| #220 | 1.85で `cargo check --all-targets --all-features --locked` が成功。stableのfmt/clippy/testも成功 | CI（MSRVジョブ、stable） |
| #221 | 非互換構文を入れるとCIが失敗。集約checkに反映 | CI。意図的な非互換構文で赤になることを一度確認する |
| #222 | `Cargo.lock` が追跡され、不一致でCIが失敗 | CI。`git ls-files` の確認 |
| #224 | 各環境のpip、certifi、CA束、必要ならuv/sigstoreの確認を記録 | CIログと成果物内の確認、Issueへの記録 |
| #225 | 最終成果物の検証が通らないとreleaseされない | releaseジョブ、検証コマンドの実行 |
| #227 | 意図的な構文エラーと短いテスト失敗を検出。所要時間と未検証範囲を記録 | 単体（入口の分岐）、Windowsでの実操作 |
| #72 | RC相当GUIでの実施、blocker未処理なし、日本語の記録 | GUI実操作（Kadoka） |
| #81 | CC BY 4.0の明記、日本語正本 | 文書レビュー |
| #51 | WindowsとUbuntuを別々に記録 | CI、起動確認 |

## 未決事項

1. MSRVジョブの実行OS。選択肢はUbuntuのみ、またはWindowsとUbuntuの両方です。推奨はUbuntuのみです。理由は、構文と依存のMSRV検査はOSに依存しにくく、CIコストを抑えられるためです。OS固有のcfgが問題になった場合に両OSへ広げます。
2. 旧CA束で問題の証明書チェーン（eMudhra/emSign系）が信頼されないことを確認する具体的手順。選択肢は、(a) 該当チェーンの公開証明書を入手して `ssl` で検証する、(b) 古いcertifiと新しいcertifiで同じ検証を比較する。推奨は(b)です。理由は、対象証明書の入手経路をIssue本文が指定しておらず、前後比較なら外部入手物に依存せずに再現できるためです。
3. Windows署名の方式。選択肢は、購入した公開用Code Signing Certificate、またはクラウド署名サービスです。推奨は後者ですが、費用と運用はKadokaの判断が必要です。Issue #225は信頼された証明書とRFC 3161 timestampを要求するだけで、調達方法は決めていません。
4. `check_local.py` の言語。推奨はPythonです（既存の `tools/` と同じで、Windows/Ubuntuの差を吸収しやすいため）。Issue #227は名称と実装言語を問いません。
