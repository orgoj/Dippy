//! Rust port of Dippy's approval decisions.
//!
//! The Python implementation in `src/dippy` is the specification; each
//! module names the Python module it ports.

pub mod allowlists;
pub mod analyzer;
pub mod ast;
pub mod bash;
pub mod cli;
pub mod config;
pub mod dump;
pub mod fnmatch;
pub mod hook;
pub mod parser;
pub mod paths;
pub mod scan;
pub mod sql;
