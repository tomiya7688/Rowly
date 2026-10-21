//! GUI adapterが共有するcommand登録・実行契約。実際の画面への接続はGUI側で行う。

mod context;
mod core;
mod registry;

pub use context::{CommandContext, CommandFocus, CommandMode, CommandScope};
pub use core::{CoreCommand, core_command_definitions};
pub use registry::{
    CommandDefinition, CommandDispatchError, CommandRegistry, CommandRegistryError,
};
