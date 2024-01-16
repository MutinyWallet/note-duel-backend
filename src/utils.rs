use anyhow::Result;
use dlc_messages::oracle_msgs::{OracleAnnouncement, OracleAttestation};
use lightning::util::ser::Readable;
use nostr::hashes::hex::FromHex;
use std::io::Cursor;

fn decode_bytes(str: &str) -> Result<Vec<u8>> {
    match FromHex::from_hex(str) {
        Ok(bytes) => Ok(bytes),
        Err(_) => Ok(base64::decode(str)?),
    }
}

/// Parses a string into an oracle announcement.
pub(crate) fn oracle_announcement_from_str(str: &str) -> Result<OracleAnnouncement> {
    let bytes = decode_bytes(str)?;
    let mut cursor = Cursor::new(bytes);

    OracleAnnouncement::read(&mut cursor)
        .map_err(|_| anyhow::anyhow!("invalid oracle announcement"))
}

/// Parses a string into an oracle attestation.
pub(crate) fn oracle_attestation_from_str(str: &str) -> Result<OracleAttestation> {
    let bytes = decode_bytes(str)?;
    let mut cursor = Cursor::new(bytes);

    OracleAttestation::read(&mut cursor).map_err(|_| anyhow::anyhow!("invalid oracle attestation"))
}
