//! Read-only Git support: remote parsing, the allowed-command policy and
//! repository inspection. Nothing here can change a repository.

pub mod inspect;
pub mod policy;
pub mod remote;
