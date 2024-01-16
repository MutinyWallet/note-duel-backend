use crate::config::Config;
use crate::models::bet::Bet;
use crate::models::MIGRATIONS;
use crate::routes::*;
use axum::http::{Method, StatusCode, Uri};
use axum::routing::{get, post};
use axum::{http, Extension, Router};
use clap::Parser;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::PgConnection;
use diesel_migrations::MigrationHarness;
use dlc::secp256k1_zkp::{All, Secp256k1};
use log::{error, info};
use nostr::EventId;
use schnorr_fun::nonce::Deterministic;
use schnorr_fun::Schnorr;
use sha2::Sha256;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch::Sender;
use tokio::sync::{oneshot, watch, Mutex};
use tower_http::cors::{Any, CorsLayer};

mod config;
mod listener;
mod models;
mod routes;
mod utils;

#[derive(Clone)]
pub struct State {
    pub db_pool: Pool<ConnectionManager<PgConnection>>,
    pub event_channel: Arc<Mutex<Sender<HashSet<EventId>>>>,
    pub schnorr: Schnorr<Sha256, Deterministic<Sha256>>,
    pub secp: Secp256k1<All>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::try_init()?;
    let config: Config = Config::parse();

    // DB management
    let manager = ConnectionManager::<PgConnection>::new(&config.pg_url);
    let db_pool = Pool::builder()
        .max_size(16)
        .test_on_check_out(true)
        .build(manager)
        .expect("Could not build connection pool");

    // run migrations
    let mut conn = db_pool.get()?;
    conn.run_pending_migrations(MIGRATIONS)
        .expect("migrations could not run");

    let event_ids = Bet::get_unfinished_bets(&mut conn)?;
    drop(conn);

    let (event_sender, event_receiver) = watch::channel(event_ids);
    let event_channel = Arc::new(Mutex::new(event_sender));

    let nonce_gen = Deterministic::<Sha256>::default();
    let schnorr = Schnorr::<Sha256, _>::new(nonce_gen);

    let state = State {
        db_pool,
        event_channel,
        schnorr,
        secp: Secp256k1::gen_new(),
    };

    let addr: std::net::SocketAddr = format!("{}:{}", config.bind, config.port)
        .parse()
        .expect("Failed to parse bind/port for webserver");

    info!("Webserver running on http://{addr}");

    let server_router = Router::new()
        .route("/health-check", get(health_check))
        .route("/create-bet", post(create_bet))
        .route("/add-sigs", post(add_sigs))
        .route("/list-pending", get(list_pending_events))
        .fallback(fallback)
        .layer(Extension(state.clone()))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_headers(vec![http::header::CONTENT_TYPE])
                .allow_methods([Method::GET, Method::POST]),
        );

    // Set up a oneshot channel to handle shutdown signal
    let (tx, rx) = oneshot::channel();

    // Spawn a task to listen for shutdown signals
    tokio::spawn(async move {
        let mut term_signal = signal(SignalKind::terminate())
            .map_err(|e| error!("failed to install TERM signal handler: {e}"))
            .unwrap();
        let mut int_signal = signal(SignalKind::interrupt())
            .map_err(|e| {
                error!("failed to install INT signal handler: {e}");
            })
            .unwrap();

        tokio::select! {
            _ = term_signal.recv() => {
                info!("Received SIGTERM");
            },
            _ = int_signal.recv() => {
                info!("Received SIGINT");
            },
        }

        let _ = tx.send(());
    });

    let relays = config.relay.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) =
                listener::start_listener(relays.clone(), state.clone(), event_receiver.clone())
                    .await
            {
                error!("listener error: {e}")
            }
        }
    });

    let server = axum::Server::bind(&addr).serve(server_router.into_make_service());

    let graceful = server.with_graceful_shutdown(async {
        let _ = rx.await;
    });

    // Await the server to receive the shutdown signal
    if let Err(e) = graceful.await {
        error!("shutdown error: {e}");
    }

    info!("Graceful shutdown complete");

    Ok(())
}

async fn fallback(uri: Uri) -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, format!("No route for {uri}"))
}
