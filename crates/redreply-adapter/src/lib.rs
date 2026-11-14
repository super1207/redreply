//! RedReply adapter facade.
//!
//! Shared traits and helpers live in `redreply-adapter-core`; concrete
//! platform implementations live in `redreply-adapter-platforms`.

pub use redreply_adapter_core::*;
pub use redreply_adapter_platforms::{
    email, framework_bridge, kook, onebot11, onebot115, qq_guild_all, qqguild_private,
    qqguild_public, satoriv1, telegram, yunhuv1,
};
