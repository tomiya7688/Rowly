// {
//   責務: [
//     data: CSV読み書き、encoding、table dataの基礎機能を提供する
//   ]
// }
mod data;
// {
//   責務: [
//     excel_python: Python bridgeを介してXLSXをimport / exportする
//   ]
// }
pub mod excel_python;
// {
//   責務: [
//     gui: Rowlyのtable editor、viewer、script editor等のGUIを提供する
//   ]
// }
#[cfg(feature = "gui")]
pub mod gui;
// {
//   責務: [
//     logical_table: project sourceのCSVをschemaごとに論理表示し、source provenanceを保つ
//   ]
// }
pub mod logical_table;
// {
//   責務: [
//     luau: resource制限付きsandboxでLuau user scriptを実行する
//   ]
// }
pub mod luau;
// {
//   責務: [
//     process: CSV documentの参照、編集、履歴、validationを扱う
//   ]
// }
pub mod process;
// {
//   責務: [
//     project: .rwprj manifest、source reference、bounded relinkを管理する
//   ]
// }
pub mod project;
// {
//   責務: [
//     project_init: project init DSLの安全な読込、適用、生成を行う
//   ]
// }
pub mod project_init;
// {
//   責務: [
//     project_session: projectとsource documentのopen、save、session状態を調整する
//   ]
// }
pub mod project_session;
// {
//   責務: [
//     rowly_dsl: Rowly DSLのAST、parser、runtimeを提供する
//   ]
// }
pub mod rowly_dsl;
// {
//   責務: [
//     rowlyx: .rwprj projectをZIP互換の.rowlyx archiveへpack / extractする
//   ]
// }
pub mod rowlyx;
