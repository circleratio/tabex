//! tabex: a fast, lightweight file manager for Windows with saved workspaces.

// Win32/COM wrappers pass raw pointers through by design (COM trait signatures included).
#![allow(clippy::not_unsafe_ptr_arg_deref)]

pub mod app;
pub mod cli;
pub mod fs;
pub mod instance;
pub mod model;
pub mod report;
pub mod shell;
pub mod store;
pub mod ui;
