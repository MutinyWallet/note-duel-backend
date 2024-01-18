use crate::models::bet::Bet;
use crate::models::sig::Sig;
use crate::utils::oracle_attestation_from_str;
use crate::State;
use anyhow::anyhow;
use diesel::PgConnection;
use dlc_messages::oracle_msgs::OracleAttestation;
use log::{debug, error, info, warn};
use nostr::{Event, EventId, Filter, Keys, Kind, Tag};
use nostr_sdk::{Client, RelayPoolNotification};
use schnorr_fun::adaptor::Adaptor;
use schnorr_fun::fun::marker::Public;
use schnorr_fun::fun::Scalar;
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::watch::Receiver;

pub async fn start_listener(
    relays: Vec<String>,
    state: State,
    mut event_receiver: Receiver<HashSet<EventId>>,
) -> anyhow::Result<()> {
    debug!("Using relays: {:?}", relays);

    let keys = Keys::generate();
    loop {
        let client = Client::new(&keys);
        client.add_relays(relays.clone()).await?;
        client.connect().await;

        let event_ids = event_receiver.borrow().clone();

        let filter = Filter::new().kind(Kind::Custom(89)).events(event_ids);

        client.subscribe(vec![filter]).await;

        info!("Listening for events...");

        let mut notifications = client.notifications();
        loop {
            tokio::select! {
                Ok(notification) = notifications.recv() => {
                    match notification {
                        RelayPoolNotification::Event {
                            relay_url: _,
                            event,
                        } => {
                            if event.kind.as_u64() == 89 && event.verify().is_ok() {
                                let state_clone = state.clone();
                                let client_clone = client.clone();
                                tokio::spawn({
                                    async move {
                                        let fut = handle_event(
                                            state_clone,
                                            client_clone,
                                            event,
                                        );

                                        match tokio::time::timeout(Duration::from_secs(120), fut).await {
                                            Ok(Ok(_)) => {}
                                            Ok(Err(e)) => error!("Error: {e}"),
                                            Err(_) => error!("Timeout"),
                                        }
                                    }
                                });
                            }
                        }
                        RelayPoolNotification::Shutdown => {
                            warn!("Relay pool shutdown");
                            break;
                        }
                        RelayPoolNotification::RelayStatus { .. } => {},
                        RelayPoolNotification::Stop => {}
                        RelayPoolNotification::Message { .. } => {}
                    }
                }
                _ = event_receiver.changed() => {
                    break;
                }
            }
        }

        client.disconnect().await?;
    }
}

async fn handle_event(state: State, client: Client, event: Event) -> anyhow::Result<()> {
    let e_tag = event.tags.into_iter().find_map(|t| {
        if let Tag::Event { event_id, .. } = t {
            Some(event_id)
        } else {
            None
        }
    });
    let e_tag = e_tag.ok_or(anyhow!("No e_tag found"))?;

    let attestation = oracle_attestation_from_str(&event.content)?;

    let mut conn = state.db_pool.get()?;
    let bets = Bet::get_by_oracle_event(&mut conn, &e_tag)?;

    for bet in bets {
        if let Err(e) = handle_bet(&mut conn, &state, &client, &attestation, bet).await {
            error!("Error handling bet: {e}");
        }
    }

    Ok(())
}

async fn handle_bet(
    conn: &mut PgConnection,
    state: &State,
    client: &Client,
    attestation: &OracleAttestation,
    bet: Bet,
) -> anyhow::Result<()> {
    let outcome = attestation.outcomes.first().ok_or(anyhow!("No outcomes"))?;
    let sig_a = Sig::get_by_params(conn, bet.id, outcome, true)?;
    let sig_b = Sig::get_by_params(conn, bet.id, outcome, false)?;

    if sig_a.is_none() && sig_b.is_none() {
        Bet::set_win_outcome_event_id(conn, bet.id, EventId::all_zeros())?; // if no sig, set outcome to 0s
        Bet::set_lose_outcome_event_id(conn, bet.id, EventId::all_zeros())?; // if no sig, set outcome to 0s
        return Ok(warn!("No sigs found for event"));
    }

    match sig_a {
        None => warn!("Sig A not found!"),
        Some(sig) => {
            let (_, s_value) = dlc::secp_utils::schnorrsig_decompose(&attestation.signatures[0])?;

            let scalar: Scalar<Public> = Scalar::from_slice(s_value)
                .ok_or(anyhow!("invalid scalar"))?
                .non_zero()
                .ok_or(anyhow!("zero scalar"))?;

            let valid_sig = state.schnorr.decrypt_signature(scalar, sig.sig());

            let unsigned = if sig.is_win {
                bet.win_a()
            } else {
                bet.lose_a()
            };

            let signature =
                nostr::secp256k1::schnorr::Signature::from_slice(&valid_sig.to_bytes())?;
            let signed_event = unsigned.add_signature(signature)?;

            if sig.is_win {
                Bet::set_win_outcome_event_id(conn, bet.id, signed_event.id)?;
            } else {
                Bet::set_lose_outcome_event_id(conn, bet.id, signed_event.id)?;
            }

            let event_id = client.send_event(signed_event).await?;
            info!("Sent event with id: {event_id}")
        }
    }

    match sig_b {
        None => warn!("Sig B not found!"),
        Some(sig) => {
            let (_, s_value) = dlc::secp_utils::schnorrsig_decompose(&attestation.signatures[0])?;

            let scalar: Scalar<Public> = Scalar::from_slice(s_value)
                .ok_or(anyhow!("invalid scalar"))?
                .non_zero()
                .ok_or(anyhow!("zero scalar"))?;

            let valid_sig = state.schnorr.decrypt_signature(scalar, sig.sig());

            let unsigned = if sig.is_win {
                bet.win_b()
            } else {
                bet.lose_b()
            };

            let signature =
                nostr::secp256k1::schnorr::Signature::from_slice(&valid_sig.to_bytes())?;
            let signed_event = unsigned.add_signature(signature)?;

            if sig.is_win {
                Bet::set_win_outcome_event_id(conn, bet.id, signed_event.id)?;
            } else {
                Bet::set_lose_outcome_event_id(conn, bet.id, signed_event.id)?;
            }

            let event_id = client.send_event(signed_event).await?;
            info!("Sent event with id: {event_id}")
        }
    }

    Ok(())
}
