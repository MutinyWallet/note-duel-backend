use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(version, author, about)]
/// Backend server for note-duel.
pub struct Config {
    #[clap(long)]
    /// Postgres connection string
    pub pg_url: String,
    #[clap(short, long)]
    /// Relay to connect to, can be specified multiple times
    pub relay: Vec<String>,
    /// Path for database with events
    #[clap(short, long)]
    pub events_db: String,
    #[clap(default_value = "0.0.0.0", long)]
    /// Bind address for note-duel's webserver
    pub bind: String,
    #[clap(default_value_t = 3000, long)]
    /// Port for note-duel's webserver
    pub port: u16,
}
