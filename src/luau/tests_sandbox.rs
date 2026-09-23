use std::fs;

use tempfile::tempdir;

use super::*;

fn sample_document() -> (tempfile::TempDir, CsvDocument) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("sample.csv");
    fs::write(&path, "Name,Score\nAlice,10\nBob,20\n").unwrap();
    let document = CsvDocument::open(path).unwrap();
    (directory, document)
}

#[test]
fn all_public_entrypoints_expose_only_the_documented_surface() {
    // 実装側の許可リストを流用せず、公開契約を独立に検査する。
    let script = r#"
        local allowed = {
            _G = true, _VERSION = true, assert = true, error = true,
            gcinfo = true, getmetatable = true, ipairs = true, next = true,
            pairs = true, pcall = true, rawequal = true, rawget = true,
            rawlen = true, rawset = true, select = true, setmetatable = true,
            tonumber = true, tostring = true, type = true, typeof = true,
            unpack = true, xpcall = true, bit32 = true, buffer = true,
            coroutine = true, math = true, string = true, table = true,
            utf8 = true, vector = true, Rowly = true,
        }
        for key in pairs(_G) do
            assert(allowed[key], "unexpected global: " .. tostring(key))
        end
        local forbidden = {
            "io", "os", "package", "require", "dofile", "loadfile",
            "load", "loadstring", "debug", "getfenv", "setfenv", "ffi",
            "jit", "socket", "http", "https", "fs", "process", "shell",
            "collectgarbage", "print", "warn", "newproxy",
            "SignalReady", "RequestStopForTest",
        }
        for _, key in ipairs(forbidden) do
            assert(_G[key] == nil, "forbidden global: " .. key)
        end
        assert(string.dump == nil)
        local api = {
            cell = true, set_cell = true, set_range = true,
            row_count = true, column_count = true,
            begin_transaction = true, commit_transaction = true,
            rollback_transaction = true,
        }
        local count = 0
        for key, value in pairs(Rowly) do
            assert(api[key], "unexpected Rowly API: " .. key)
            assert(type(value) == "function")
            count += 1
        end
        assert(count == 8)
        assert(Rowly.fs == nil and Rowly.http == nil and Rowly.process == nil)
        assert(Rowly.undo == nil and Rowly.redo == nil and Rowly.save == nil)
        assert(Rowly.sandbox == nil and Rowly.set_memory_limit == nil)
        assert(Rowly.remove_interrupt == nil)
        Rowly.set_cell("B2", "42")
    "#;
    for entry in 0..4 {
        let (_directory, mut document) = sample_document();
        let token = LuauCancellationToken::new();
        let result = match entry {
            0 => execute(&mut document, script),
            1 => execute_with_limits(&mut document, script, LuauLimits::default()),
            2 => execute_with_cancellation(&mut document, script, &token),
            _ => execute_with_limits_and_cancellation(
                &mut document,
                script,
                LuauLimits::default(),
                &token,
            ),
        };
        result.unwrap();
        assert_eq!(document.cell_a1("B2").unwrap(), Some("42"));
        assert!(document.undo().unwrap());
        assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
    }
}

#[test]
fn sandbox_keeps_safe_standard_libraries_and_user_tables_usable() {
    let (_directory, mut document) = sample_document();
    execute(
        &mut document,
        r#"
            local values = {3, 1, 2}
            table.sort(values)
            assert(table.concat(values, ",") == "1,2,3")
            assert(string.upper("abc") == "ABC")
            assert(("xyz"):sub(2) == "yz")
            assert(math.floor(2.9) == 2)
            assert(bit32.band(7, 3) == 3)
            assert(utf8.len("日本語") == 3)
            local bytes = buffer.create(4)
            buffer.writeu8(bytes, 0, 42)
            assert(buffer.readu8(bytes, 0) == 42)
            local v = vector.create(1, 2, 3)
            assert(v.X == 1)
            local object = setmetatable({}, {__index = {answer = 42}})
            assert(object.answer == 42)
            rawset(object, "answer", 43)
            assert(rawget(object, "answer") == 43)
            local co = coroutine.create(function()
                coroutine.yield("ready")
                Rowly.set_cell("B2", "42")
            end)
            local ok, value = coroutine.resume(co)
            assert(ok and value == "ready")
            assert(coroutine.resume(co))
            assert(Rowly.cell("B2") == "42")
        "#,
    )
    .unwrap();
    assert_eq!(document.cell_a1("B2").unwrap(), Some("42"));
}

#[test]
fn rowly_and_standard_tables_cannot_be_modified_even_with_raw_operations() {
    let (_directory, mut document) = sample_document();
    execute(
        &mut document,
        r#"
            assert(table.isfrozen(Rowly))
            assert(not pcall(function() Rowly.cell = function() return "fake" end end))
            assert(not pcall(rawset, Rowly, "set_cell", function() end))
            assert(not pcall(rawset, Rowly, "fs", {}))
            assert(not pcall(setmetatable, Rowly, {}))
            assert(not pcall(table.clear, Rowly))
            for _, lib in ipairs({string, table, math, utf8, bit32, buffer, vector, coroutine}) do
                assert(table.isfrozen(lib))
                assert(not pcall(rawset, lib, "injected", true))
            end
            assert(not pcall(function() math.abs = function() return 0 end end))
            assert(not pcall(rawset, _G, "injected", true))
            local mt = getmetatable("")
            assert(not pcall(rawset, mt, "__index", {}))
            assert(math.abs(-2) == 2)
            assert(Rowly.cell("A2") == "Alice")
            Rowly.set_cell("B2", "42")
        "#,
    )
    .unwrap();
    assert_eq!(document.cell_a1("B2").unwrap(), Some("42"));
}

#[test]
fn global_shadowing_and_user_state_do_not_escape_the_current_execution() {
    let (_directory, mut document) = sample_document();
    execute(
        &mut document,
        r#"
            PerRunMarker = {value = 42}
            assert(PerRunMarker.value == 42)
            local original = Rowly
            Rowly = {cell = function() return "local" end}
            assert(Rowly.cell() == "local")
            assert(original.cell("A2") == "Alice")
            assert(_G.Rowly.cell("A2") == "Alice")
            math = {abs = function() return "local" end}
            assert(math.abs() == "local")
            assert(_G.math.abs(-2) == 2)
        "#,
    )
    .unwrap();
    execute(
        &mut document,
        r#"
            assert(PerRunMarker == nil)
            assert(Rowly.cell("A2") == "Alice")
            assert(math.abs(-2) == 2)
            assert(table.isfrozen(Rowly))
        "#,
    )
    .unwrap();
    assert!(!document.is_dirty());
}

#[test]
fn forbidden_access_fails_and_rolls_back_the_scripts_pending_transaction() {
    for attempt in [
        "io.open('unavailable', 'w')",
        "os.execute('echo unavailable')",
        "require('socket')",
        "package.loadlib('unavailable', 'entry')",
        "http.get('https://example.invalid')",
        "loadfile('unavailable')",
        "loadstring('return 1')",
        "getfenv(0)",
        "setfenv(1, {})",
    ] {
        let (_directory, mut document) = sample_document();
        let before = fs::read(document.path()).unwrap();
        let script =
            format!("Rowly.begin_transaction()\nRowly.set_cell(\"B2\", \"pending\")\n{attempt}");
        let error = execute(&mut document, &script).unwrap_err();
        assert!(matches!(error, LuauError::Runtime(_)), "{attempt}: {error}");
        assert!(!document.transaction_active());
        assert!(!document.is_dirty());
        assert!(!document.can_undo());
        assert_eq!(document.cell_a1("B2").unwrap(), Some("10"));
        assert_eq!(fs::read(document.path()).unwrap(), before);
    }
}

#[test]
fn sandbox_does_not_disable_vm_memory_or_duration_limits() {
    let (_directory, mut document) = sample_document();
    let error = execute_with_limits(
        &mut document,
        "local data = string.rep('x', 4 * 1024 * 1024)",
        LuauLimits {
            max_memory_bytes: 512 * 1024,
            ..LuauLimits::default()
        },
    )
    .unwrap_err();
    assert!(matches!(error, LuauError::Memory(_)), "{error}");

    let error = execute_with_limits(
        &mut document,
        "while true do end",
        LuauLimits {
            max_duration: Duration::ZERO,
            ..LuauLimits::default()
        },
    )
    .unwrap_err();
    assert!(matches!(error, LuauError::Limit(_)), "{error}");
    assert!(!document.is_dirty());
}

#[test]
fn non_source_input_is_rejected_without_changing_the_document() {
    let (_directory, mut document) = sample_document();
    let error = execute(&mut document, "\u{3}\0not-luau-source").unwrap_err();
    assert!(matches!(error, LuauError::Runtime(_)));
    assert!(!document.is_dirty());
}
