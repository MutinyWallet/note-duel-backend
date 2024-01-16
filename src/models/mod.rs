use crate::models::bet::Bet;
use diesel::{Connection, PgConnection};
use diesel_migrations::{embed_migrations, EmbeddedMigrations};
use dlc_messages::oracle_msgs::OracleAnnouncement;
use nostr::{EventId, UnsignedEvent};
use schnorr_fun::adaptor::EncryptedSignature;
use std::collections::HashMap;

pub mod bet;
mod schema;
pub mod sig;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

pub fn create_bet(
    conn: &mut PgConnection,
    oracle_announcement: OracleAnnouncement,
    unsigned_a: UnsignedEvent,
    unsigned_b: UnsignedEvent,
    oracle_event_id: EventId,
    sigs: HashMap<String, EncryptedSignature>,
) -> anyhow::Result<()> {
    conn.transaction(|conn| {
        let bet = Bet::create(
            conn,
            oracle_announcement,
            unsigned_a,
            unsigned_b,
            oracle_event_id,
        )?;
        sig::Sig::create_all(conn, bet.id, true, sigs)?;
        Ok(())
    })
}

pub fn add_sigs(
    conn: &mut PgConnection,
    bet_id: i32,
    sigs: HashMap<String, EncryptedSignature>,
) -> anyhow::Result<Bet> {
    conn.transaction(|conn| {
        sig::Sig::create_all(conn, bet_id, false, sigs)?;
        let bet = Bet::set_needs_reply(conn, bet_id)?;
        Ok(bet)
    })
}
