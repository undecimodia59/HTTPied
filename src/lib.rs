//! Restricted, bounded M0 transport. See README for protocol limitations.
pub mod activity;
pub mod model;
pub mod proxy;
#[cfg(feature = "ui")]
pub mod ui;

#[cfg(feature = "tls-spike")]
pub mod tls;

#[cfg(feature = "ui")]
pub mod browser;
pub mod control;
pub mod replay;

pub mod jobs;

mod response;

mod tunnel;

pub mod payload;

pub mod project;
pub mod state;

mod decoding;
pub mod exchange;

/// Shared transport interface keeps HTTP framing independent from TCP or TLS.
pub trait Transport: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Transport for T {}

#[cfg(feature = "tls-spike")]
pub mod authority;

pub mod attack;

pub mod interchange;

pub mod policy;

pub mod protocols;

pub mod resolver;
pub mod trust;

#[cfg(feature = "debug-logging")]
pub mod diagnostics;

pub mod coder;

pub mod campaign;
pub mod jwt;
