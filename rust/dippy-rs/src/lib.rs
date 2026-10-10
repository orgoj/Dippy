//! Rust port of Dippy's approval decisions.
//!
//! The Python implementation in `src/dippy` is the specification; each
//! module names the Python module it ports.

pub mod admin;
pub mod allowlists;
pub mod analyzer;
pub mod askpass;
pub mod askpass_gui;
pub mod ast;
pub mod audit;
pub mod bash;
pub mod cli;
pub mod config;
pub mod dashboard;
pub mod doctor;
pub mod dump;
pub mod fnmatch;
pub mod hook;
pub mod hooks;
pub mod http;
pub mod logging;
pub mod parser;
pub mod paths;
pub mod pi;
pub mod run;
pub mod scan;
pub mod sql;
