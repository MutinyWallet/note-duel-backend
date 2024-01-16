use super::bet::Bet;
use super::schema::sigs;
use diesel::prelude::*;
use schnorr_fun::adaptor::EncryptedSignature;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(
    Associations,
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
#[diesel(belongs_to(Bet, foreign_key = bet_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Sig {
    pub id: i32,
    pub bet_id: i32,
    pub is_party_a: bool,
    sig: Vec<u8>,
    outcome: String,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = sigs)]
struct NewSig {
    bet_id: i32,
    is_party_a: bool,
    sig: Vec<u8>,
    outcome: String,
}

impl Sig {
    pub fn sig(&self) -> EncryptedSignature {
        bincode::deserialize(&self.sig).expect("invalid sig")
    }

    pub fn create_all(
        conn: &mut PgConnection,
        bet_id: i32,
        is_party_a: bool,
        sigs: HashMap<String, EncryptedSignature>,
    ) -> anyhow::Result<Self> {
        let new_sigs = sigs
            .into_iter()
            .map(|(outcome, sig)| NewSig {
                bet_id,
                is_party_a,
                sig: bincode::serialize(&sig).expect("invalid sig"),
                outcome,
            })
            .collect::<Vec<_>>();

        Ok(diesel::insert_into(sigs::table)
            .values(new_sigs)
            .get_result(conn)?)
    }

    pub fn get_by_bet_id_and_outcome(
        conn: &mut PgConnection,
        bet_id: i32,
        outcome: &str,
    ) -> anyhow::Result<Option<Self>> {
        let res = sigs::table
            .filter(sigs::bet_id.eq(bet_id))
            .filter(sigs::outcome.eq(outcome))
            .first(conn)
            .optional()?;

        Ok(res)
    }
}
