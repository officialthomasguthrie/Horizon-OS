//! The identity binding for a Constellation link.
//!
//! Every device of one identity holds the same 32-byte master key. There is no
//! asymmetric "device key" to authenticate against: the secret all your devices
//! share is the master itself. So the honest construction is a pre-shared-key
//! Noise handshake. We derive a domain-separated PSK from the master and run
//! `Noise_NNpsk0`: ephemeral keys on both sides give forward secrecy, and the
//! PSK, mixed in before the first message, means only a peer that holds the same
//! identity can complete the handshake. A wrong identity fails on the first
//! message (its tag will not verify), so it is turned away at the door rather
//! than after objects have moved.
//!
//! This sits inside the QUIC stream. QUIC's own TLS here is just the transport
//! envelope (a throwaway self-signed cert, see [`super::tls`]); this layer is
//! what actually authenticates the peer as a holder of the identity and adds a
//! second AEAD over every framed message.

use snow::{HandshakeState, TransportState};

use crate::error::{Error, Result};

// 25519 for the DH, ChaChaPoly for the AEAD, BLAKE2s for the hash. NNpsk0 has no
// static keys; psk0 mixes the pre-shared key in ahead of the first token.
const PATTERN: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";

// The link PSK is not the master itself but a key derived from it for this one
// purpose, so a Constellation handshake can never leak or be replayed against
// the Lifestream's addressing or record keys (which derive from the same master
// under their own labels).
fn link_psk(master: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key("horizon constellation noise psk v1", master)
}

// The side that dials. Writes message one, reads message two.
pub fn initiator(master: &[u8; 32]) -> Result<HandshakeState> {
    let psk = link_psk(master);
    let params = PATTERN.parse().map_err(net)?;
    snow::Builder::new(params)
        .psk(0, &psk)
        .build_initiator()
        .map_err(net)
}

// The side that listens. Reads message one, writes message two.
pub fn responder(master: &[u8; 32]) -> Result<HandshakeState> {
    let psk = link_psk(master);
    let params = PATTERN.parse().map_err(net)?;
    snow::Builder::new(params)
        .psk(0, &psk)
        .build_responder()
        .map_err(net)
}

pub fn into_transport(hs: HandshakeState) -> Result<TransportState> {
    hs.into_transport_mode().map_err(net)
}

pub fn net(e: impl std::fmt::Display) -> Error {
    Error::Net(e.to_string())
}
