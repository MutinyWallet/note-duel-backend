use super::schema::bets;
use diesel::prelude::*;
use dlc_messages::oracle_msgs::OracleAnnouncement;
use lightning::util::ser::{Readable, Writeable};
use nostr::key::XOnlyPublicKey;
use nostr::{EventId, JsonUtil, UnsignedEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::io::Cursor;

#[derive(
    Queryable,
    Insertable,
    Identifiable,
    AsChangeset,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
#[diesel(primary_key(id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Bet {
    pub id: i32,
    oracle_announcement: Vec<u8>,
    user_a: Vec<u8>,
    unsigned_a: Value,
    user_b: Vec<u8>,
    unsigned_b: Value,
    oracle_event_id: Vec<u8>,
    pub needs_reply: bool,
    outcome_event_id: Option<Vec<u8>>,
    created_at: chrono::NaiveDateTime,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = bets)]
struct NewBet {
    oracle_announcement: Vec<u8>,
    user_a: Vec<u8>,
    unsigned_a: Value,
    user_b: Vec<u8>,
    unsigned_b: Value,
    oracle_event_id: Vec<u8>,
}

impl Bet {
    pub fn oracle_announcement(&self) -> OracleAnnouncement {
        let mut cursor = Cursor::new(&self.oracle_announcement);
        OracleAnnouncement::read(&mut cursor).expect("invalid oracle announcement")
    }

    pub fn user_a(&self) -> XOnlyPublicKey {
        XOnlyPublicKey::from_slice(&self.user_a).expect("invalid user_a")
    }

    pub fn unsigned_a(&self) -> UnsignedEvent {
        UnsignedEvent::from_json(self.unsigned_a.to_string()).expect("invalid unsigned_a")
    }

    pub fn user_b(&self) -> XOnlyPublicKey {
        XOnlyPublicKey::from_slice(&self.user_b).expect("invalid user_a")
    }

    pub fn unsigned_b(&self) -> UnsignedEvent {
        UnsignedEvent::from_json(self.unsigned_b.to_string()).expect("invalid unsigned_b")
    }

    pub fn oracle_event_id(&self) -> EventId {
        EventId::from_slice(&self.oracle_event_id).expect("invalid oracle_event_id")
    }

    pub fn outcome_event_id(&self) -> Option<EventId> {
        self.outcome_event_id
            .as_ref()
            .map(|b| EventId::from_slice(b).expect("invalid outcome_event_id"))
    }

    pub fn create(
        conn: &mut PgConnection,
        oracle_announcement: OracleAnnouncement,
        unsigned_a: UnsignedEvent,
        unsigned_b: UnsignedEvent,
        oracle_event_id: EventId,
    ) -> anyhow::Result<Self> {
        let new_bet = NewBet {
            oracle_announcement: oracle_announcement.encode(),
            user_a: unsigned_a.pubkey.serialize().to_vec(),
            unsigned_a: serde_json::to_value(unsigned_a)?,
            user_b: unsigned_b.pubkey.serialize().to_vec(),
            unsigned_b: serde_json::to_value(unsigned_b)?,
            oracle_event_id: oracle_event_id.to_bytes().to_vec(),
        };
        let res = diesel::insert_into(bets::table)
            .values(new_bet)
            .get_result::<Self>(conn)?;
        Ok(res)
    }

    pub fn get_by_id(conn: &mut PgConnection, id: i32) -> anyhow::Result<Option<Self>> {
        let res = bets::table.find(id).first::<Self>(conn).optional()?;
        Ok(res)
    }

    pub fn get_by_oracle_event(
        conn: &mut PgConnection,
        oracle_event_id: &EventId,
    ) -> anyhow::Result<Vec<Self>> {
        let bytes = oracle_event_id.to_bytes().to_vec();
        let res = bets::table
            .filter(bets::oracle_event_id.eq(bytes))
            .load::<Self>(conn)?;
        Ok(res)
    }

    pub fn get_pending_bets(
        conn: &mut PgConnection,
        user: XOnlyPublicKey,
    ) -> anyhow::Result<Vec<Bet>> {
        let res = bets::table
            .filter(bets::needs_reply.eq(true))
            .filter(bets::user_b.eq(user.serialize().to_vec()))
            .load::<Self>(conn)?;
        Ok(res)
    }

    pub fn get_unfinished_bets(conn: &mut PgConnection) -> anyhow::Result<HashSet<EventId>> {
        let res = bets::table
            .filter(bets::needs_reply.eq(false))
            .filter(bets::outcome_event_id.is_null())
            .select(bets::oracle_event_id)
            .load::<Vec<u8>>(conn)?
            .into_iter()
            .map(|b| EventId::from_slice(&b).expect("invalid oracle_event_id"))
            .collect();
        Ok(res)
    }

    pub fn set_needs_reply(conn: &mut PgConnection, id: i32) -> anyhow::Result<Self> {
        let res = diesel::update(bets::table.find(id))
            .set(bets::needs_reply.eq(false))
            .get_result::<Self>(conn)?;
        Ok(res)
    }

    pub fn set_outcome_event_id(
        conn: &mut PgConnection,
        id: i32,
        outcome_event_id: EventId,
    ) -> anyhow::Result<()> {
        diesel::update(bets::table.find(id))
            .set(bets::outcome_event_id.eq(outcome_event_id.to_bytes().to_vec()))
            .execute(conn)?;
        Ok(())
    }
}
