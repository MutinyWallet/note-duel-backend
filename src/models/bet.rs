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
    win_a: Value,
    lose_a: Value,
    user_b: Vec<u8>,
    win_b: Value,
    lose_b: Value,
    oracle_event_id: Vec<u8>,
    pub needs_reply: bool,
    win_outcome_event_id: Option<Vec<u8>>,
    lose_outcome_event_id: Option<Vec<u8>>,
    created_at: chrono::NaiveDateTime,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = bets)]
struct NewBet {
    oracle_announcement: Vec<u8>,
    user_a: Vec<u8>,
    win_a: Value,
    lose_a: Value,
    user_b: Vec<u8>,
    win_b: Value,
    lose_b: Value,
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

    pub fn win_a(&self) -> UnsignedEvent {
        UnsignedEvent::from_json(self.win_a.to_string()).expect("invalid win_a")
    }

    pub fn lose_a(&self) -> UnsignedEvent {
        UnsignedEvent::from_json(self.lose_a.to_string()).expect("invalid lose_a")
    }

    pub fn user_b(&self) -> XOnlyPublicKey {
        XOnlyPublicKey::from_slice(&self.user_b).expect("invalid user_a")
    }

    pub fn win_b(&self) -> UnsignedEvent {
        UnsignedEvent::from_json(self.win_b.to_string()).expect("invalid win_b")
    }

    pub fn lose_b(&self) -> UnsignedEvent {
        UnsignedEvent::from_json(self.lose_b.to_string()).expect("invalid lose_b")
    }

    pub fn oracle_event_id(&self) -> EventId {
        EventId::from_slice(&self.oracle_event_id).expect("invalid oracle_event_id")
    }

    pub fn win_outcome_event_id(&self) -> Option<EventId> {
        self.win_outcome_event_id
            .as_ref()
            .map(|b| EventId::from_slice(b).expect("invalid win_outcome_event_id"))
    }

    pub fn lose_outcome_event_id(&self) -> Option<EventId> {
        self.lose_outcome_event_id
            .as_ref()
            .map(|b| EventId::from_slice(b).expect("invalid lose_outcome_event_id"))
    }

    pub fn create(
        conn: &mut PgConnection,
        oracle_announcement: OracleAnnouncement,
        win_a: UnsignedEvent,
        lose_a: UnsignedEvent,
        win_b: UnsignedEvent,
        lose_b: UnsignedEvent,
        oracle_event_id: EventId,
    ) -> anyhow::Result<Self> {
        let new_bet = NewBet {
            oracle_announcement: oracle_announcement.encode(),
            user_a: win_a.pubkey.serialize().to_vec(),
            win_a: serde_json::to_value(win_a)?,
            lose_a: serde_json::to_value(lose_a)?,
            user_b: win_b.pubkey.serialize().to_vec(),
            win_b: serde_json::to_value(win_b)?,
            lose_b: serde_json::to_value(lose_b)?,
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

    pub fn get_active_bets(
        conn: &mut PgConnection,
        user: XOnlyPublicKey,
    ) -> anyhow::Result<Vec<Bet>> {
        let bytes = user.serialize().to_vec();
        let res = bets::table
            .filter(bets::needs_reply.eq(false))
            .filter(bets::user_b.eq(&bytes).or(bets::user_a.eq(&bytes)))
            .load::<Self>(conn)?;
        Ok(res)
    }

    pub fn get_unfinished_bets(conn: &mut PgConnection) -> anyhow::Result<HashSet<EventId>> {
        let res = bets::table
            .filter(bets::needs_reply.eq(false))
            .filter(bets::win_outcome_event_id.is_null())
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

    pub fn set_win_outcome_event_id(
        conn: &mut PgConnection,
        id: i32,
        win_outcome_event_id: EventId,
    ) -> anyhow::Result<()> {
        diesel::update(bets::table.find(id))
            .set(bets::win_outcome_event_id.eq(win_outcome_event_id.to_bytes().to_vec()))
            .execute(conn)?;
        Ok(())
    }

    pub fn set_lose_outcome_event_id(
        conn: &mut PgConnection,
        id: i32,
        lose_outcome_event_id: EventId,
    ) -> anyhow::Result<()> {
        diesel::update(bets::table.find(id))
            .set(bets::lose_outcome_event_id.eq(lose_outcome_event_id.to_bytes().to_vec()))
            .execute(conn)?;
        Ok(())
    }

    pub fn get_active_event_count(conn: &mut PgConnection) -> anyhow::Result<i64> {
        let res = bets::table
            .filter(bets::needs_reply.eq(false))
            .filter(bets::win_outcome_event_id.is_null())
            .count()
            .get_result::<i64>(conn)?;

        Ok(res)
    }

    pub fn get_completed_event_count(conn: &mut PgConnection) -> anyhow::Result<i64> {
        let res = bets::table
            .filter(bets::win_outcome_event_id.is_not_null())
            .count()
            .get_result::<i64>(conn)?;

        Ok(res)
    }

    pub fn get_event_ids(conn: &mut PgConnection) -> anyhow::Result<Vec<EventId>> {
        let events = bets::table
            .select((bets::win_outcome_event_id, bets::lose_outcome_event_id))
            .load::<(Option<Vec<u8>>, Option<Vec<u8>>)>(conn)?
            .into_iter()
            .flat_map(|(w, l)| {
                let w = w.map(|w| vec![EventId::from_slice(&w).expect("event_id")]);
                let l = l.map(|l| vec![EventId::from_slice(&l).expect("event_id")]);

                let mut vec = w.unwrap_or_default();
                vec.extend(l.unwrap_or_default());

                vec
            })
            .collect();

        Ok(events)
    }
}
