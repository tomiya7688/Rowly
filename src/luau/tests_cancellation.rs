use std::{fs, sync::mpsc, thread};

use tempfile::tempdir;

use super::*;

fn sample_document() -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("sample.csv");
    fs::write(&path, "Name,Score\nAlice,10\nBob,20\n").unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

// テスト専用の準備通知を使い、対象編集に到達する前のキャンセルで
// rollback テストが偶然成功することを防ぐ。VM は実行側スレッドに留める。
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

#[test]
fn cancellation_tokens_are_thread_safe_sticky_and_independent() {
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
