use mlua::{Lua, LuaOptions, Result, StdLib, Value};

// Luau の標準機能を無条件に公開しない。追加は権限境界のレビューと
// tests_sandbox の公開面テストを伴う変更として扱う。
const ALLOWED_GLOBALS: &[&str] = &[
    "_G",
    "_VERSION",
    "assert",
    "error",
    "gcinfo",
    "getmetatable",
    "ipairs",
    "next",
    "pairs",
    "pcall",
    "rawequal",
    "rawget",
    "rawlen",
    "rawset",
    "select",
    "setmetatable",
    "tonumber",
    "tostring",
    "type",
    "typeof",
    "unpack",
    "xpcall",
    "bit32",
    "buffer",
    "coroutine",
    "math",
    "string",
    "table",
    "utf8",
    "vector",
];

/// 実行ごとに外部能力のない VM を作成する。呼び出し側には公開しない。
/// Rowly API の登録後、スクリプト実行前に Lua::sandbox(true) を適用する。
pub(super) fn new_vm() -> Result<Lua> {
    let libraries = StdLib::TABLE
        | StdLib::STRING
        | StdLib::MATH
        | StdLib::UTF8
        | StdLib::BIT
        | StdLib::BUFFER
        | StdLib::VECTOR
        | StdLib::COROUTINE;
    let lua = Lua::new_with(libraries, LuaOptions::default())?;
    let globals = lua.globals();
    // 基本ライブラリや mlua が追加するグローバルも許可リストに限定する。
    // 走査中に変更せず、キーを集めてから削除する。
    let names = globals
        .clone()
        .pairs::<String, Value>()
        .map(|entry| entry.map(|(name, _)| name))
        .collect::<Result<Vec<_>>>()?;
    for name in names {
        if !ALLOWED_GLOBALS.contains(&name.as_str()) {
            globals.raw_set(name, Value::Nil)?;
        }
    }
    Ok(lua)
}
