#![doc = include_str!("../README.md")]

#[cfg(feature = "cache")]
pub mod cache;

#[cfg(feature = "stream")]
pub mod stream;

#[cfg(feature = "singleton")]
pub mod singleton;
