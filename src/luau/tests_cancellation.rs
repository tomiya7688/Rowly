use std::{fs, sync::mpsc, thread};

use tempfile::tempdir;

use super::*;

// {
//   責務: [sample_document: Luau security test用のCSV fixtureを一時作成してCsvDocumentを開く。]
//   処理: [TempDirへ固定のName/Score CSVを書き込み、CsvDocument::openへ渡す。]
//   引数: []
//   戻り値: [(TempDir, CsvDocument): 一時ディレクトリと開いたdocument。]
//   副作用: [一時ディレクトリにsample.csvを作成する。]
// }
fn sample_document() -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("sample.csv");
    fs::write(&path, "Name,Score\nAlice,10\nBob,20\n").unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

// テスト専用の準備通知を使い、対象編集に到達する前のキャンセルで
// rollback テストが偶然成功することを防ぐ。VM は実行側スレッドに留める。
// {
//   責務: [cancel_after_signal: Luau scriptが準備通知を送った後に別threadからcancelし、実行errorを返す。]
//   処理: [test VMへSignalReadyを登録し、通知受信後にtokenをcancelするscoped threadとexecute_in_luaを同期する。]
//   引数: [document: 編集対象CsvDocument。 script: cancellation pointを含むLuau source。]
//   戻り値: [LuauError: script停止またはruntime error。]
//   副作用: [別threadの起動と、script内のRowly APIによるdocument編集。]
//   補足: [VMを実行threadに留め、signal後に停止するため編集到達前の誤成功を避ける。]
// }
fn cancel_after_signal(document: &mut CsvDocument, script: &str) -> LuauError {
    let cancellation = LuauCancellationToken::new();
    let stop = cancellation.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    let lua = Lua::new();
    lua.globals()
        .set(
            "SignalReady",
            lua.create_function(move |_, ()| ready_tx.send(()).map_err(LuaError::external))
                .unwrap(),
        )
        .unwrap();

    thread::scope(|scope| {
        let canceller = scope.spawn(move || {
            ready_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("script must reach the cancellation point");
            stop.cancel();
        });
        let result = execute_in_lua(
            &lua,
            document,
            script,
            LuauLimits {
                max_duration: Duration::from_secs(10),
                max_interrupts: u64::MAX,
                max_memory_bytes: 8 * 1024 * 1024,
            },
            &cancellation,
        );
        canceller.join().unwrap();
        result.unwrap_err()
    })
}

// {
//   責務: [cancellation_tokens_are_thread_safe_sticky_and_independent: LuauCancellationTokenがSend/Syncで、clone間共有・sticky停止要求・token独立性を持つことを確認する。]
//   処理: [Send/Sync boundをcompile時に検証し、cloneを別threadから複数回cancelして元tokenと別tokenの状態を確認する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
#[test]
fn cancellation_tokens_are_thread_safe_sticky_and_independent() {
    // {
    //   責務: [assert_send_sync: 型がSend + Syncを実装することをcompile時に要求する。]
    //   処理: [型parameterのtrait boundを関数signatureで検査する。]
    //   引数: []
    //   戻り値: [(): compile assertion成功時に値を返さない。]
    // }
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LuauCancellationToken>();

    let token = LuauCancellationToken::new();
    let other = LuauCancellationToken::default();
    let stop = token.clone();
    assert!(!token.is_cancelled());
    thread::spawn(move || {
        stop.cancel();
        stop.cancel();
    })
    .join()
    .unwrap();
    assert!(token.is_cancelled());
    assert!(!other.is_cancelled());
}

// {
//   責務: [cancellation_before_start_does_not_edit_or_rollback_external_transaction: 開始前の停止要求ではscriptを実行せず、呼出側transactionを維持することを確認する。]
//   処理: [既存transactionに編集後、事前cancel済みtokenでscriptを呼び、error・cell値・transaction状態をassertする。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
#[test]
fn cancellation_before_start_does_not_edit_or_rollback_external_transaction() {
    let (_directory, mut document) = sample_document();
    document.begin_transaction().unwrap();
    document.set_cell_a1("B2", "existing").unwrap();
    let token = LuauCancellationToken::new();
    token.cancel();

    let error = execute_with_cancellation(
        &mut document,
        r#"Rowly.set_cell("B3", "unexpected")"#,
        &token,
    )
    .unwrap_err();

    assert!(matches!(error, LuauError::Cancelled));
    assert!(document.transaction_active());
    assert_eq!(document.cell_a1("B2").unwrap(), Some("existing"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("20"));
    document.rollback_transaction().unwrap();
}

// {
//   責務: [external_cancellation_stops_infinite_loop_and_allows_next_execution: 外部停止要求が無限loopを止め、同じdocumentで次の実行とundoができることを確認する。]
//   処理: [SignalReady到達後に別threadからcancelし、停止errorを確認して新tokenで後続実行・undoを行う。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
#[test]
fn external_cancellation_stops_infinite_loop_and_allows_next_execution() {
    let (_directory, mut document) = sample_document();
    let error = cancel_after_signal(&mut document, "SignalReady()\nwhile true do end");
    assert!(matches!(error, LuauError::Cancelled));
    assert!(!document.is_dirty());

    execute_with_cancellation(
        &mut document,
        r#"Rowly.set_cell("B2", "42")"#,
        &LuauCancellationToken::new(),
    )
    .unwrap();
    assert_eq!(document.cell_a1("B2").unwrap(), Some("42"));
    assert!(document.undo().unwrap());
    assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
}

// {
//   責務: [cancellation_rolls_back_pending_edits_and_preserves_redo_history: 実行中transactionのpending editをrollbackし、既存redo historyを保つことを確認する。]
//   処理: [redo可能な履歴を作ってからLuau transaction内で編集し、停止後のdocumentとredo結果を検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
#[test]
fn cancellation_rolls_back_pending_edits_and_preserves_redo_history() {
    let (_directory, mut document) = sample_document();
    document.set_cell_a1("B2", "before").unwrap();
    document.undo().unwrap();
    assert!(document.can_redo());

    let error = cancel_after_signal(
        &mut document,
        r#"
            Rowly.begin_transaction()
            Rowly.set_cell("B2", "42")
            Rowly.set_cell("B3", "99")
            SignalReady()
            while true do end
        "#,
    );

    assert!(matches!(error, LuauError::Cancelled));
    assert!(!document.transaction_active());
    assert!(!document.is_dirty());
    assert!(!document.can_undo());
    assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("20"));
    assert!(document.redo().unwrap());
    assert_eq!(document.cell_a1("B2").unwrap(), Some("before"));
}

// {
//   責務: [cancellation_preserves_committed_edits_but_rolls_back_later_transaction: 先にcommitした編集を残し、後続transactionのpending editだけをrollbackすることを確認する。]
//   処理: [commit済み編集と次transactionのpending editを同じscriptで作り、停止後に残存値・dirty・undoを検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
#[test]
fn cancellation_preserves_committed_edits_but_rolls_back_later_transaction() {
    let (_directory, mut document) = sample_document();
    let error = cancel_after_signal(
        &mut document,
        r#"
            Rowly.begin_transaction()
            Rowly.set_cell("B2", "committed")
            Rowly.commit_transaction()
            Rowly.begin_transaction()
            Rowly.set_cell("B3", "pending")
            SignalReady()
            while true do end
        "#,
    );

    assert!(matches!(error, LuauError::Cancelled));
    assert!(!document.transaction_active());
    assert_eq!(document.cell_a1("B2").unwrap(), Some("committed"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("20"));
    assert!(document.is_dirty());
    assert!(document.undo().unwrap());
    assert!(!document.is_dirty());
}

// {
//   責務: [cancellation_does_not_claim_whole_script_rollback_without_transaction: transactionなしの停止では既に反映したcell編集を保持することを確認する。]
//   処理: [transaction外のcell編集後に停止し、編集保持とundoでの復元を検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
#[test]
fn cancellation_does_not_claim_whole_script_rollback_without_transaction() {
    let (_directory, mut document) = sample_document();
    let error = cancel_after_signal(
        &mut document,
        r#"
            Rowly.set_cell("B2", "kept")
            SignalReady()
            while true do end
        "#,
    );

    assert!(matches!(error, LuauError::Cancelled));
    assert_eq!(document.cell_a1("B2").unwrap(), Some("kept"));
    assert!(document.undo().unwrap());
    assert!(!document.is_dirty());
}

// {
//   責務: [cancellation_keeps_external_transaction_under_caller_control: 実行開始前から呼出側が所有するtransactionを停止処理がrollbackしないことを確認する。]
//   処理: [呼出側で開始したtransactionへscriptからpending editを追加して停止し、transactionを呼出側でrollbackする。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
#[test]
fn cancellation_keeps_external_transaction_under_caller_control() {
    let (_directory, mut document) = sample_document();
    document.begin_transaction().unwrap();
    document.set_cell_a1("B2", "external").unwrap();
    let error = cancel_after_signal(
        &mut document,
        r#"
            Rowly.set_cell("B3", "pending")
            SignalReady()
            while true do end
        "#,
    );

    assert!(matches!(error, LuauError::Cancelled));
    assert!(document.transaction_active());
    assert_eq!(document.cell_a1("B2").unwrap(), Some("external"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("pending"));
    document.rollback_transaction().unwrap();
    assert!(!document.is_dirty());
    assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("20"));
}

// {
//   責務: [protected_calls_cannot_turn_cancellation_into_success_or_skip_cleanup: pcall/xpcallで停止errorを捕捉してもCancelledを返しpending transactionをcleanupする。]
//   処理: [pcallとxpcallの各scriptでtransactionを開始して停止し、Cancelledとrollback結果を検証する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
#[test]
fn protected_calls_cannot_turn_cancellation_into_success_or_skip_cleanup() {
    for protected_loop in [
        "pcall(function() SignalReady(); while true do end end)",
        "xpcall(function() SignalReady(); while true do end end, function(e) return e end)",
    ] {
        let (_directory, mut document) = sample_document();
        let script = format!(
            "Rowly.begin_transaction()\nRowly.set_cell(\"B2\", \"pending\")\n{protected_loop}"
        );
        let error = cancel_after_signal(&mut document, &script);
        assert!(matches!(error, LuauError::Cancelled));
        assert!(!document.transaction_active());
        assert!(!document.is_dirty());
        assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
    }
}

// {
//   責務: [host_api_guards_and_final_check_work_even_between_vm_interrupts: VM interrupt間に停止しても各Rowly API入口と実行終了時確認が停止を検出することを確認する。]
//   処理: [test VMのinterruptを外してからcancelし、Rowly APIごとの拒否・Lua内assert完了・終了時Cancelledとcleanupを確認する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
#[test]
fn host_api_guards_and_final_check_work_even_between_vm_interrupts() {
    let (_directory, mut document) = sample_document();
    let cancellation = LuauCancellationToken::new();
    let stop = cancellation.clone();
    let lua = Lua::new();
    // テストだけで interrupt を外し、API 境界と終了時確認を独立に検証する。
    // この関数や VM 設定の変更手段は本番スクリプトへ公開しない。
    lua.globals()
        .set(
            "RequestStopForTest",
            lua.create_function(move |lua, ()| {
                lua.remove_interrupt();
                stop.cancel();
                Ok(())
            })
            .unwrap(),
        )
        .unwrap();

    let error = execute_in_lua(
        &lua,
        &mut document,
        r#"
            Rowly.begin_transaction()
            Rowly.set_cell("B2", "pending")
            RequestStopForTest()
            assert(not pcall(Rowly.cell, "B2"))
            assert(not pcall(Rowly.set_cell, "B3", "blocked"))
            assert(not pcall(Rowly.set_range, "B2:B3", "blocked"))
            assert(not pcall(Rowly.row_count))
            assert(not pcall(Rowly.column_count))
            assert(not pcall(Rowly.begin_transaction))
            assert(not pcall(Rowly.commit_transaction))
            assert(not pcall(Rowly.rollback_transaction))
            AllGuardsChecked = true
        "#,
        LuauLimits::default(),
        &cancellation,
    )
    .unwrap_err();

    // 終了時の Cancelled への変換が Lua 側の assertion 失敗を隠していないこと。
    assert!(lua.globals().get::<bool>("AllGuardsChecked").unwrap());
    assert!(matches!(error, LuauError::Cancelled));
    assert!(!document.transaction_active());
    assert!(!document.is_dirty());
    assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
    assert_eq!(document.cell_a1("B3").unwrap(), Some("20"));
}

// {
//   責務: [non_cancelled_runs_keep_runtime_and_limit_error_classification: 停止要求がない場合はruntime errorとlimit errorを別分類しtokenを未停止のまま保つ。]
//   処理: [同じ未停止tokenで通常runtime errorとinterrupt超過を起こし、各LuauError variantを比較する。]
//   引数: []
//   戻り値: [(): assertion成功時に値を返さない。]
// }
#[test]
fn non_cancelled_runs_keep_runtime_and_limit_error_classification() {
    let (_directory, mut document) = sample_document();
    let token = LuauCancellationToken::new();
    let error = execute_with_cancellation(&mut document, "error('ordinary')", &token).unwrap_err();
    assert!(matches!(error, LuauError::Runtime(_)));

    let error = execute_with_limits_and_cancellation(
        &mut document,
        "while true do end",
        LuauLimits {
            max_interrupts: 100,
            ..LuauLimits::default()
        },
        &token,
    )
    .unwrap_err();
    assert!(matches!(error, LuauError::Limit(_)));
    assert!(!token.is_cancelled());
}
