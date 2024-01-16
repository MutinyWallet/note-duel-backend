use crate::models::bet::Bet;
use crate::{models, utils, State};
use axum::extract::Query;
use axum::http::StatusCode;
use axum::{Extension, Json};
use dlc::secp256k1_zkp::hashes::sha256;
use dlc::OracleInfo;
use lightning::util::ser::Writeable;
use log::error;
use nostr::hashes::hex::FromHex;
use nostr::{EventId, UnsignedEvent};
use schnorr_fun::adaptor::{Adaptor, EncryptedSignature};
use schnorr_fun::fun::marker::{EvenY, NonZero, Normal, Public};
use schnorr_fun::fun::Point;
use schnorr_fun::Message;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

pub async fn health_check() -> Result<Json<bool>, (StatusCode, String)> {
    Ok(Json(true))
}

#[derive(Deserialize)]
pub struct CreateBetRequest {
    oracle_announcement: String,
    oracle_event_id: String,
    unsigned_event: UnsignedEvent,
    counterparty_unsigned_event: UnsignedEvent,
    sigs: HashMap<String, String>,
}

async fn create_bet_impl(state: &State, request: CreateBetRequest) -> anyhow::Result<()> {
    let oracle_announcement = utils::oracle_announcement_from_str(&request.oracle_announcement)?;
    let oracle_info = OracleInfo {
        public_key: oracle_announcement.oracle_public_key,
        nonces: oracle_announcement.oracle_event.oracle_nonces.clone(),
    };

    if request.sigs.len() > oracle_info.nonces.len() {
        return Err(anyhow::anyhow!("too many sigs"));
    }

    let verification_key: Point<EvenY, Public, NonZero> =
        Point::from_xonly_bytes(request.unsigned_event.pubkey.serialize())
            .ok_or(anyhow::anyhow!("invalid pubkey"))?;
    let message = Message::<Public>::raw(request.unsigned_event.id.as_bytes());
    let mut sigs: HashMap<String, EncryptedSignature> = HashMap::with_capacity(request.sigs.len());
    for (outcome, sig) in request.sigs {
        let bytes: Vec<u8> = FromHex::from_hex(&sig)?;
        let enc: EncryptedSignature = bincode::deserialize(&bytes)?;

        let msg =
            vec![dlc::secp256k1_zkp::Message::from_hashed_data::<sha256::Hash>(outcome.as_bytes())];
        let point =
            dlc::get_adaptor_point_from_oracle_info(&state.secp, &[oracle_info.clone()], &[msg])?;

        let encryption_key: Point<Normal, Public, NonZero> =
            Point::from_bytes(point.serialize()).ok_or(anyhow::anyhow!("invalid pubkey"))?;

        if !state.schnorr.verify_encrypted_signature(
            &verification_key,
            &encryption_key,
            message,
            &enc,
        ) {
            return Err(anyhow::anyhow!("invalid sig"));
        }
        sigs.insert(outcome, enc);
    }

    let oracle_event_id = EventId::from_str(&request.oracle_event_id)?;

    let mut conn = state.db_pool.get()?;
    models::create_bet(
        &mut conn,
        oracle_announcement,
        request.unsigned_event,
        request.counterparty_unsigned_event,
        oracle_event_id,
        sigs,
    )?;

    Ok(())
}

pub async fn create_bet(
    Extension(state): Extension<State>,
    Json(request): Json<CreateBetRequest>,
) -> Result<Json<bool>, (StatusCode, String)> {
    match create_bet_impl(&state, request).await {
        Ok(_) => Ok(Json(true)),
        Err(e) => {
            error!("Error creating bet: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

#[derive(Deserialize)]
pub struct AddSigsRequest {
    id: i32,
    sigs: HashMap<String, String>,
}

async fn add_sigs_impl(state: &State, request: AddSigsRequest) -> anyhow::Result<()> {
    let mut conn = state.db_pool.get()?;
    let bet = Bet::get_by_id(&mut conn, request.id)?.ok_or(anyhow::anyhow!("bet not found"))?;

    if !bet.needs_reply {
        anyhow::bail!("bet already setup")
    }

    let oracle_announcement = bet.oracle_announcement();
    let oracle_info = OracleInfo {
        public_key: oracle_announcement.oracle_public_key,
        nonces: oracle_announcement.oracle_event.oracle_nonces,
    };

    let verification_key: Point<EvenY, Public, NonZero> =
        Point::from_xonly_bytes(bet.user_b().serialize())
            .ok_or(anyhow::anyhow!("invalid pubkey"))?;
    let unsigned_b = bet.unsigned_b();
    let message = Message::<Public>::raw(unsigned_b.id.as_bytes());
    let mut sigs: HashMap<String, EncryptedSignature> = HashMap::with_capacity(request.sigs.len());
    for (outcome, sig) in request.sigs {
        let bytes: Vec<u8> = FromHex::from_hex(&sig)?;
        let enc: EncryptedSignature = bincode::deserialize(&bytes)?;

        let msg =
            vec![dlc::secp256k1_zkp::Message::from_hashed_data::<sha256::Hash>(outcome.as_bytes())];
        let point =
            dlc::get_adaptor_point_from_oracle_info(&state.secp, &[oracle_info.clone()], &[msg])?;

        let encryption_key: Point<Normal, Public, NonZero> =
            Point::from_bytes(point.serialize()).ok_or(anyhow::anyhow!("invalid pubkey"))?;

        if !state.schnorr.verify_encrypted_signature(
            &verification_key,
            &encryption_key,
            message,
            &enc,
        ) {
            return Err(anyhow::anyhow!("invalid sig"));
        }
        sigs.insert(outcome, enc);
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
pub struct ListPendingEventsRequest {
    pub pubkey: String,
}

#[derive(Serialize)]
pub struct PendingBet {
    unsigned_a: UnsignedEvent,
    unsigned_b: UnsignedEvent,
    oracle_announcement: String,
}

pub async fn list_pending_events_impl(
    state: &State,
    request: ListPendingEventsRequest,
) -> anyhow::Result<Vec<PendingBet>> {
    let pubkey = nostr::key::XOnlyPublicKey::from_str(&request.pubkey)?;
    let mut conn = state.db_pool.get()?;
    let bets = Bet::get_pending_bets(&mut conn, pubkey)?;

    let mut pending_bets = Vec::with_capacity(bets.len());
    for bet in bets {
        let oracle_announcement = bet.oracle_announcement();
        let unsigned_a = bet.unsigned_a();
        let unsigned_b = bet.unsigned_b();

        pending_bets.push(PendingBet {
            unsigned_a,
            unsigned_b,
            oracle_announcement: base64::encode(oracle_announcement.encode()),
        });
    }

    Ok(pending_bets)
}

pub async fn list_pending_events(
    Extension(state): Extension<State>,
    Query(request): Query<ListPendingEventsRequest>,
) -> Result<Json<Vec<PendingBet>>, (StatusCode, String)> {
    match list_pending_events_impl(&state, request).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => {
            error!("Error listing pending events: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}
