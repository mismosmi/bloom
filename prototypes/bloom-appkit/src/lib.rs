//! AppKit renderer prototype for bloom. macOS only.
#![cfg(target_os = "macos")]

pub mod appkit;
mod executor;
pub mod node;

pub use appkit::run;
pub use node::{AppKitNode, Button, Outlet, StackView, TextField, Window};
