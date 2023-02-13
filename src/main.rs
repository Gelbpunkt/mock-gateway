#![feature(once_cell, option_result_contains)]

use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr,
};

use config::CONFIG;
use libc::{c_int, sighandler_t, signal, SIGINT, SIGTERM};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tracing::{error, info};
use tracing_subscriber::{filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{handler::Connection, session::Sessions};

mod config;
mod handler;
mod script;
mod session;

async fn run() -> Result<(), io::Error> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), CONFIG.port);
    let listener = TcpListener::bind(addr).await?;

    let sessions = Sessions::new();

    info!("Listening on {addr}");

    while let Ok((stream, remote_addr)) = listener.accept().await {
        info!("Connection from {remote_addr}");

        let sessions_clone = sessions.clone();

        tokio::spawn(async move {
            if let Ok(ws_stream) = accept_async(stream).await {
                let mut connection = Connection::new(ws_stream, sessions_clone);
                if let Err(e) = connection.handle().await {
                    error!("Websocket handler errored: {e:?}");
                };
            } else {
                error!("Websocket handshake with {remote_addr} failed");
            }
        });
    }

    Ok(())
}

pub extern "C" fn handler(_: c_int) {
    std::process::exit(0);
}

unsafe fn set_os_handlers() {
    signal(SIGINT, handler as extern "C" fn(_) as sighandler_t);
    signal(SIGTERM, handler as extern "C" fn(_) as sighandler_t);
}

fn main() {
    unsafe { set_os_handlers() };

    let level_filter = LevelFilter::from_str(&CONFIG.log_level).unwrap_or(LevelFilter::INFO);
    let fmt_layer = tracing_subscriber::fmt::layer();
    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(level_filter)
        .init();

    if let Err(e) = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run())
    {
        eprintln!("Fatal error: {e}");
    }
}
