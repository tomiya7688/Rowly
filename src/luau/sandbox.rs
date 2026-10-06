use mlua::{Lua, LuaOptions, Result, StdLib, Value};

// {
//   責務: [ALLOWED_GLOBALS: sandbox VMのglobalへ残す標準名をallowlistで固定する。]
//   補足: [権限境界の変更はtests_sandboxの公開面テストとsecurity reviewを伴う。]
// }
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

// {
//   責務: [new_vm: 許可された標準libraryとglobalだけを持つ実行ごとのLua VMを作成する。]
//   処理: [許可libraryでLuaを初期化し、globalを走査してallowlist外のkeyを削除する。]
//   引数: []
//   戻り値: [Lua: sandbox化前のVM。呼出側がRowly APIを登録してからsandboxを有効化する。]
//   エラー: [mlua::Error: library初期化またはglobal走査・削除に失敗した場合。]
// }
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
