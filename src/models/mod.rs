use crate::models::bet::Bet;
use crate::models::sig::Sig;
use diesel::{Connection, PgConnection};
use diesel_migrations::{embed_migrations, EmbeddedMigrations};
use dlc_messages::oracle_msgs::OracleAnnouncement;
use nostr::key::XOnlyPublicKey;
use nostr::{EventId, UnsignedEvent};
use schnorr_fun::adaptor::EncryptedSignature;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod bet;
mod schema;
pub mod sig;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

#[allow(clippy::too_many_arguments)]
pub fn create_bet(
    conn: &mut PgConnection,
    oracle_announcement: OracleAnnouncement,
    win_a: UnsignedEvent,
    lose_a: UnsignedEvent,
    win_b: UnsignedEvent,
    lose_b: UnsignedEvent,
    oracle_event_id: EventId,
    sigs: HashMap<String, (EncryptedSignature, bool)>,
) -> anyhow::Result<i32> {
    conn.transaction(|conn| {
        let bet = Bet::create(
            conn,
            oracle_announcement,
            win_a,
            lose_a,
            win_b,
            lose_b,
            oracle_event_id,
        )?;
        Sig::create_all(conn, bet.id, true, sigs)?;
        Ok(bet.id)
    })
}

pub fn add_sigs(
    conn: &mut PgConnection,
    bet_id: i32,
    sigs: HashMap<String, (EncryptedSignature, bool)>,
) -> anyhow::Result<Bet> {
    conn.transaction(|conn| {
        Sig::create_all(conn, bet_id, false, sigs)?;
        let bet = Bet::set_needs_reply(conn, bet_id)?;
        Ok(bet)
    })
}

pub fn reject_bet(conn: &mut PgConnection, bet_id: i32, key: XOnlyPublicKey) -> anyhow::Result<()> {
    conn.transaction(|conn| {
        let event = Bet::get_by_id(conn, bet_id)?;

        if let Some(bet) = event {
            if bet.user_a() == key || bet.user_b() == key {
                Sig::delete_by_bet_id(conn, bet_id)?;
                Bet::delete_by_bet_id(conn, bet_id)?;
            }
        }
        Ok(())
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Counts {
    active: i64,
    completed: i64,
}

pub fn get_counts(conn: &mut PgConnection) -> anyhow::Result<Counts> {
    conn.transaction(|conn| {
        let active = Bet::get_active_event_count(conn)?;
        let completed = Bet::get_completed_event_count(conn)?;

        Ok(Counts { active, completed })
    })
}
