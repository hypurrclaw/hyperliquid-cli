//! Release updates: a passive "newer version" notice ([`notifier`]) and the
//! `hyperliquid update` self-updater ([`self_update`]). They share only the
//! release facts in [`release`].

mod notifier;
mod release;
mod self_update;

pub use notifier::{
    UpdateCheck, flush_update_check, flush_update_check_silent, maybe_start_update_check,
};
pub use self_update::update;
