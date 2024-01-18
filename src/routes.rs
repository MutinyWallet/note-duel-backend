use crate::models::bet::Bet;
use crate::models::sig::Sig;
use crate::models::Counts;
use crate::{models, utils, State};
use axum::extract::Query;
use axum::http::StatusCode;
use axum::{Extension, Json};
use dlc::secp256k1_zkp::hashes::hex::ToHex;
use dlc::secp256k1_zkp::hashes::sha256;
use dlc::OracleInfo;
use dlc_messages::oracle_msgs::EventDescriptor;
use lightning::util::ser::Writeable;
use log::error;
use nostr::{EventId, UnsignedEvent};
use schnorr_fun::adaptor::{Adaptor, EncryptedSignature};
use schnorr_fun::fun::marker::{EvenY, NonZero, Normal, Public};
use schnorr_fun::fun::Point;
use schnorr_fun::Message;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

pub async fn health_check() -> Result<Json<bool>, (StatusCode, String)> {
    Ok(Json(true))
}

#[derive(Deserialize)]
pub struct CreateBetRequest {
    oracle_announcement: String,
    oracle_event_id: EventId,
    win_event: UnsignedEvent,
    lose_event: UnsignedEvent,
    counterparty_win_event: UnsignedEvent,
    counterparty_lose_event: UnsignedEvent,
    sigs: HashMap<String, EncryptedSignature>,
}

async fn create_bet_impl(state: &State, request: CreateBetRequest) -> anyhow::Result<i32> {
    let oracle_announcement = utils::oracle_announcement_from_str(&request.oracle_announcement)?;
    let oracle_info = OracleInfo {
        public_key: oracle_announcement.oracle_public_key,
        nonces: oracle_announcement.oracle_event.oracle_nonces.clone(),
    };

    let all_outcomes = if let EventDescriptor::EnumEvent(ref desc) =
        oracle_announcement.oracle_event.event_descriptor
    {
        desc.outcomes.clone()
    } else {
        anyhow::bail!("Only enum events supported");
    };

    if request.sigs.len() != all_outcomes.len() {
        anyhow::bail!(
            "Incorrect number of sigs, {} != {}",
            request.sigs.len(),
            all_outcomes.len()
        );
    }

    let verification_key: Point<EvenY, Public, NonZero> =
        Point::from_xonly_bytes(request.win_event.pubkey.serialize())
            .ok_or(anyhow::anyhow!("invalid pubkey"))?;
    let win_message = Message::<Public>::raw(request.win_event.id.as_bytes());
    let lose_message = Message::<Public>::raw(request.lose_event.id.as_bytes());
    let mut sigs: HashMap<String, (EncryptedSignature, bool)> =
        HashMap::with_capacity(request.sigs.len());
    for (outcome, sig) in request.sigs {
        let msg =
            vec![dlc::secp256k1_zkp::Message::from_hashed_data::<sha256::Hash>(outcome.as_bytes())];
        let point =
            dlc::get_adaptor_point_from_oracle_info(&state.secp, &[oracle_info.clone()], &[msg])?;

        let encryption_key: Point<Normal, Public, NonZero> =
            Point::from_bytes(point.serialize()).ok_or(anyhow::anyhow!("invalid pubkey"))?;

        let is_win = state.schnorr.verify_encrypted_signature(
            &verification_key,
            &encryption_key,
            win_message,
            &sig,
        );

        let is_lose = state.schnorr.verify_encrypted_signature(
            &verification_key,
            &encryption_key,
            lose_message,
            &sig,
        );

        if !is_win && !is_lose {
            return Err(anyhow::anyhow!("invalid sig"));
        }

        sigs.insert(outcome, (sig, is_win));
    }

    let mut conn = state.db_pool.get()?;
    let id = models::create_bet(
        &mut conn,
        oracle_announcement,
        request.win_event,
        request.lose_event,
        request.counterparty_win_event,
        request.counterparty_lose_event,
        request.oracle_event_id,
        sigs,
    )?;

    Ok(id)
}

pub async fn create_bet(
    Extension(state): Extension<State>,
    Json(request): Json<CreateBetRequest>,
) -> Result<Json<i32>, (StatusCode, String)> {
    match create_bet_impl(&state, request).await {
        Ok(id) => Ok(Json(id)),
        Err(e) => {
            error!("Error creating bet: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

#[derive(Deserialize)]
pub struct AddSigsRequest {
    id: i32,
    sigs: HashMap<String, EncryptedSignature>,
}

async fn add_sigs_impl(state: &State, request: AddSigsRequest) -> anyhow::Result<()> {
    let mut conn = state.db_pool.get()?;
    let bet = Bet::get_by_id(&mut conn, request.id)?.ok_or(anyhow::anyhow!("bet not found"))?;

    if !bet.needs_reply {
        anyhow::bail!("bet already setup")
    }

    let all_outcomes = if let EventDescriptor::EnumEvent(ref desc) =
        bet.oracle_announcement().oracle_event.event_descriptor
    {
        desc.outcomes.clone()
    } else {
        anyhow::bail!("Only enum events supported");
    };

    if request.sigs.len() != all_outcomes.len() {
        anyhow::bail!(
            "Incorrect number of sigs, {} != {}",
            request.sigs.len(),
            all_outcomes.len()
        );
    }

    let oracle_announcement = bet.oracle_announcement();
    let oracle_info = OracleInfo {
        public_key: oracle_announcement.oracle_public_key,
        nonces: oracle_announcement.oracle_event.oracle_nonces,
    };

    let verification_key: Point<EvenY, Public, NonZero> =
        Point::from_xonly_bytes(bet.user_b().serialize())
            .ok_or(anyhow::anyhow!("invalid pubkey"))?;
    let win_b = bet.win_b();
    let lose_b = bet.lose_b();
    let win_message = Message::<Public>::raw(win_b.id.as_bytes());
    let lose_message = Message::<Public>::raw(lose_b.id.as_bytes());
    let mut sigs: HashMap<String, (EncryptedSignature, bool)> =
        HashMap::with_capacity(request.sigs.len());
    for (outcome, sig) in request.sigs {
        let msg =
            vec![dlc::secp256k1_zkp::Message::from_hashed_data::<sha256::Hash>(outcome.as_bytes())];
        let point =
            dlc::get_adaptor_point_from_oracle_info(&state.secp, &[oracle_info.clone()], &[msg])?;

        let encryption_key: Point<Normal, Public, NonZero> =
            Point::from_bytes(point.serialize()).ok_or(anyhow::anyhow!("invalid pubkey"))?;

        let is_lose = state.schnorr.verify_encrypted_signature(
            &verification_key,
            &encryption_key,
            lose_message,
            &sig,
        );

        let is_win = state.schnorr.verify_encrypted_signature(
            &verification_key,
            &encryption_key,
            win_message,
            &sig,
        );

        if !is_win && !is_lose {
            return Err(anyhow::anyhow!("invalid sig"));
        }

        sigs.insert(outcome, (sig, is_win));
    }

    let bet = models::add_sigs(&mut conn, request.id, sigs)?;

    // notify new oracle event
    let sender = state.event_channel.lock().await;
    sender.send_if_modified(|current| current.insert(bet.oracle_event_id()));

    Ok(())
}

pub async fn add_sigs(
    Extension(state): Extension<State>,
    Json(request): Json<AddSigsRequest>,
) -> Result<Json<bool>, (StatusCode, String)> {
    match add_sigs_impl(&state, request).await {
        Ok(_) => Ok(Json(true)),
        Err(e) => {
            error!("Error adding sigs: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

#[derive(Deserialize)]
pub struct ListEventsRequest {
    pub pubkey: String,
}

#[derive(Serialize)]
pub struct UserBet {
    id: i32,
    win_a: UnsignedEvent,
    lose_a: UnsignedEvent,
    win_b: UnsignedEvent,
    lose_b: UnsignedEvent,
    oracle_announcement: String,
    oracle_event_id: EventId,
    user_outcomes: HashSet<String>,
    counterparty_outcomes: HashSet<String>,
    win_outcome_event_id: Option<EventId>,
    lose_outcome_event_id: Option<EventId>,
}

pub async fn list_pending_events_impl(
    state: &State,
    request: ListEventsRequest,
) -> anyhow::Result<Vec<UserBet>> {
    let pubkey = nostr::key::XOnlyPublicKey::from_str(&request.pubkey)?;
    let mut conn = state.db_pool.get()?;
    let bets = Bet::get_pending_bets(&mut conn, pubkey)?;

    let mut pending_bets = Vec::with_capacity(bets.len());
    for bet in bets {
        let oracle_announcement = bet.oracle_announcement();
        let win_a = bet.win_a();
        let lose_a = bet.lose_a();
        let win_b = bet.win_b();
        let lose_b = bet.lose_b();
        let sigs = Sig::get_by_bet_id(&mut conn, bet.id)?;
        let is_a = win_a.pubkey.to_hex() == request.pubkey;
        let outcomes_a = sigs
            .into_iter()
            .filter(|s| s.is_party_a == is_a && s.is_win)
            .map(|s| s.outcome)
            .collect::<HashSet<_>>();

        let mut outcomes_b = match oracle_announcement.oracle_event.event_descriptor {
            EventDescriptor::EnumEvent(ref events) => HashSet::from_iter(events.outcomes.clone()),
            EventDescriptor::DigitDecompositionEvent(_) => continue,
        };
        outcomes_b.retain(|o| !outcomes_a.contains(o));

        let (user_outcomes, counterparty_outcomes) = if is_a {
            (outcomes_a, outcomes_b)
        } else {
            (outcomes_b, outcomes_a)
        };

        pending_bets.push(UserBet {
            id: bet.id,
            win_a,
            lose_a,
            win_b,
            lose_b,
            oracle_announcement: base64::encode(oracle_announcement.encode()),
            oracle_event_id: bet.oracle_event_id(),
            user_outcomes,
            counterparty_outcomes,
            win_outcome_event_id: None,
            lose_outcome_event_id: None,
        });
    }

    Ok(pending_bets)
}

pub async fn list_pending_events(
    Extension(state): Extension<State>,
    Query(request): Query<ListEventsRequest>,
) -> Result<Json<Vec<UserBet>>, (StatusCode, String)> {
    match list_pending_events_impl(&state, request).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => {
            error!("Error listing pending events: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

pub async fn list_events_impl(
    state: &State,
    request: ListEventsRequest,
) -> anyhow::Result<Vec<UserBet>> {
    let pubkey = nostr::key::XOnlyPublicKey::from_str(&request.pubkey)?;
    let mut conn = state.db_pool.get()?;
    let bets = Bet::get_active_bets(&mut conn, pubkey)?;

    let mut pending_bets = Vec::with_capacity(bets.len());
    for bet in bets {
        let oracle_announcement = bet.oracle_announcement();
        let win_a = bet.win_a();
        let lose_a = bet.lose_a();
        let win_b = bet.win_b();
        let lose_b = bet.lose_b();
        let sigs = Sig::get_by_bet_id(&mut conn, bet.id)?;
        let user_a_outcomes = sigs
            .iter()
            .filter(|s| s.is_party_a)
            .map(|s| s.outcome.clone())
            .collect::<HashSet<_>>();
        let user_b_outcomes = sigs
            .into_iter()
            .filter(|s| !s.is_party_a)
            .map(|s| s.outcome)
            .collect::<HashSet<_>>();

        let (user, counterparty) = if bet.user_a() == pubkey {
            (user_a_outcomes, user_b_outcomes)
        } else {
            (user_b_outcomes, user_a_outcomes)
        };

        pending_bets.push(UserBet {
            id: bet.id,
            win_a,
            lose_a,
            win_b,
            lose_b,
            oracle_announcement: base64::encode(oracle_announcement.encode()),
            oracle_event_id: bet.oracle_event_id(),
            user_outcomes: user,
            counterparty_outcomes: counterparty,
            win_outcome_event_id: bet.win_outcome_event_id(),
            lose_outcome_event_id: bet.lose_outcome_event_id(),
        });
    }

    Ok(pending_bets)
}

pub async fn list_events(
    Extension(state): Extension<State>,
    Query(request): Query<ListEventsRequest>,
) -> Result<Json<Vec<UserBet>>, (StatusCode, String)> {
    match list_events_impl(&state, request).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => {
            error!("Error listing events: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

pub async fn get_counts(
    Extension(state): Extension<State>,
) -> Result<Json<Counts>, (StatusCode, String)> {
    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    match models::get_counts(&mut conn) {
        Ok(res) => Ok(Json(res)),
        Err(e) => {
            error!("Error listing counts: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

pub async fn get_event_ids(
    Extension(state): Extension<State>,
) -> Result<Json<Vec<EventId>>, (StatusCode, String)> {
    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    match Bet::get_event_ids(&mut conn) {
        Ok(res) => Ok(Json(res)),
        Err(e) => {
            error!("Error listing event_ids: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}
