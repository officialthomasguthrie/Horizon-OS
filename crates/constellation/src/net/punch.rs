//! UDP hole punching: a direct path between two peers that both sit behind NATs,
//! opened without sending their traffic through a relay.
//!
//! The relay ([`super::relay`]) always works, but every byte of the sync flows
//! through a third host. Often that is avoidable. A NAT that refuses unsolicited
//! inbound packets still lets a reply through to a mapping its own outbound packet
//! just created. So if both peers send toward each other's public address at the
//! same moment, each one's outbound packet opens its NAT's mapping, and the
//! other's packet, arriving at that fresh mapping, gets in. The connection then
//! runs directly between the two, no relay in the path.
//!
//! The hard part is coordination, and the rendezvous ([`super::rendezvous`])
//! already has what it needs: it observes each peer's public address (the source
//! of that peer's connection to it). A serving peer sends a PunchWait and the
//! rendezvous holds its connection; a dialer sends a PunchConnect; the rendezvous
//! hands each the other's observed address and a go signal, on the same instant,
//! so both fire at once. The serving peer fires a throwaway probe to open its
//! mapping and accepts the dialer's real connection; the dialer fires that real
//! connection. One socket per peer does double duty, signalling the rendezvous
//! and carrying the punch, so the mapping the rendezvous observed is the one the
//! peer is punched on.
//!
//! Identity stays where it always is. The rendezvous brokers by the non-secret
//! [`fingerprint`](crate::fingerprint) and never holds the master; the punched
//! connection runs the same Noise NNpsk0 handshake as a direct link, so a wrong
//! identity is refused at the peer. What punching adds over a direct dial is only
//! the coordinated simultaneous open; everything past it is the ordinary sync.
//!
//! Hole punching only succeeds against cone NATs, where the mapping a peer uses
//! toward the rendezvous is the same one it uses toward the other peer. A
//! symmetric NAT assigns a fresh mapping per destination, so the observed address
//! is useless to punch toward and the relay remains the fallback. The coordination
//! here is exercised over loopback in CI; crossing a real NAT is real-host work.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::label::fingerprint;
use crate::LocalTransport;

use super::noise::net;
use super::rendezvous::{Req, Resp};
use super::tls;
use super::wire::NoiseChannel;
use super::{accept_loop, NetworkTransport};

// A rendezvous control message (a PunchWait/PunchConnect or its reply) is a tag
// and at most a short address; this cap just stops a peer making us buffer
// without bound.
const MAX_CTL: usize = 4 * 1024;

// How long to drive a throwaway hole-punch probe before giving up on it. The
// probe is never meant to complete; this only bounds how long its task lingers.
const PROBE: Duration = Duration::from_secs(3);

// Wait at a rendezvous to be hole-punched by dialers of the same identity, and
// serve each punched connection with the same logic the direct endpoint uses.
// Binds one socket, signals the rendezvous over it, and accepts punched dialers
// on it. The returned handle keeps the wait live; dropping it closes the
// rendezvous connection, which withdraws the wait and stops accepting. Used by
// [`super::Server::punch_via_rendezvous`].
pub(super) fn listen(
    rendezvous_addr: SocketAddr,
    master: [u8; 32],
    transport: Arc<LocalTransport>,
) -> Result<PunchListener> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(net)?;

    let (endpoint, rz_conn) = rt.block_on(async {
        let fp = fingerprint(&master);
        let bind: SocketAddr = "0.0.0.0:0".parse().unwrap();
        // Server-capable, so it can accept the punched dialer's connection, and a
        // client, so it can reach the rendezvous: one socket does both, so the
        // address the rendezvous observes is exactly the one a dialer punches.
        let mut endpoint = quinn::Endpoint::server(tls::server_config()?, bind).map_err(net)?;
        endpoint.set_default_client_config(tls::client_config()?);
        let rz_conn = endpoint
            .connect(rendezvous_addr, "horizon")
            .map_err(net)?
            .await
            .map_err(net)?;
        let (mut send, mut recv) = rz_conn.open_bi().await.map_err(net)?;
        write_req(&mut send, &Req::PunchWait { fp }).await?;
        match Resp::decode(&recv.read_to_end(MAX_CTL).await.map_err(net)?)? {
            Resp::PunchWaiting => {}
            other => return Err(unexpected(other)),
        }
        Ok::<_, Error>((endpoint, rz_conn))
    })?;

    // Serve any inbound connection (a punched dialer landing on our socket)
    // exactly like the direct server does. The Noise handshake authenticates it.
    rt.spawn(accept_loop(endpoint.clone(), master, transport));

    // The rendezvous opens a stream back to us per dialer, carrying that dialer's
    // observed address. Fire a throwaway connection at it: the handshake packets
    // it sends open our NAT mapping toward the dialer, so the dialer's real
    // connection gets in at the accept loop above. We never expect the throwaway
    // to complete; the dialer is a client only and answers it with nothing.
    let punch_ep = endpoint.clone();
    let pushes = rz_conn.clone();
    rt.spawn(async move {
        while let Ok((_s, mut recv)) = pushes.accept_bi().await {
            let punch_ep = punch_ep.clone();
            tokio::spawn(async move {
                let buf = match recv.read_to_end(MAX_CTL).await {
                    Ok(b) => b,
                    Err(_) => return,
                };
                if let Ok(Resp::PunchGo(dialer)) = Resp::decode(&buf) {
                    if let Ok(connecting) = punch_ep.connect(dialer, "horizon") {
                        let _ = tokio::time::timeout(PROBE, connecting).await;
                    }
                }
            });
        }
    });

    Ok(PunchListener {
        conn: rz_conn,
        _rt: rt,
        _endpoint: endpoint,
    })
}

// Reach a serving peer by hole punching, brokered by a rendezvous. Asks the
// rendezvous to broker a punch to this identity's fingerprint; on a match it gets
// the peer's observed address and fires a connection toward it while the peer
// fires its probe back, then runs the Noise handshake over the connection that
// forms, returning a transport indistinguishable from a direct one. Errors if no
// peer of this identity is waiting to be punched (the caller falls back to a
// relay). Used by [`super::NetworkTransport::connect_via_punch`].
pub(super) fn connect(rendezvous_addr: SocketAddr, master: [u8; 32]) -> Result<NetworkTransport> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(net)?;

    let (endpoint, conn, ch) = rt.block_on(async {
        let fp = fingerprint(&master);
        let bind: SocketAddr = "0.0.0.0:0".parse().unwrap();
        // Client only: it dials the rendezvous and then the peer. Having no server
        // config means the peer's probe (which it fires at us to open its own
        // mapping) finds no listener and is harmlessly dropped, rather than
        // opening a second, confused connection alongside our real one.
        let mut endpoint = quinn::Endpoint::client(bind).map_err(net)?;
        endpoint.set_default_client_config(tls::client_config()?);
        let rz_conn = endpoint
            .connect(rendezvous_addr, "horizon")
            .map_err(net)?
            .await
            .map_err(net)?;
        let (mut send, mut recv) = rz_conn.open_bi().await.map_err(net)?;
        write_req(&mut send, &Req::PunchConnect { fp }).await?;
        let peer = match Resp::decode(&recv.read_to_end(MAX_CTL).await.map_err(net)?)? {
            Resp::PunchGo(addr) => addr,
            Resp::PunchNoPeer => return Err(net("rendezvous has no peer waiting to punch")),
            other => return Err(unexpected(other)),
        };
        // Fire the real connection toward the peer's observed address. Our
        // outgoing handshake opens our own mapping; the peer is firing its probe
        // at us at the same moment, opening its mapping; QUIC retransmits the
        // handshake for a few seconds, covering the skew between the two fires.
        let conn = endpoint
            .connect(peer, "horizon")
            .map_err(net)?
            .await
            .map_err(net)?;
        let (csend, crecv) = conn.open_bi().await.map_err(net)?;
        let ch = NoiseChannel::initiator(csend, crecv, &master).await?;
        Ok::<_, Error>((endpoint, conn, ch))
    })?;

    Ok(NetworkTransport::from_parts(rt, endpoint, conn, ch))
}

// A live wait at a rendezvous, held for as long as a peer wants to be reachable
// by hole punching. Dropping it closes the rendezvous connection: the rendezvous
// withdraws the wait and the push loop ends, then the runtime drops with the
// handle, stopping the serve loop. A serving process killed (ctrl-c) never runs
// this drop, which is fine: the rendezvous withdraws the wait when it sees the
// connection drop regardless.
pub struct PunchListener {
    conn: quinn::Connection,
    // Both kept alive for the life of the wait: the runtime runs the serve and
    // push loops, and dropping the endpoint would tear down the connection.
    _rt: tokio::runtime::Runtime,
    _endpoint: quinn::Endpoint,
}

impl Drop for PunchListener {
    fn drop(&mut self) {
        // Close the rendezvous connection, then let the close frame reach it
        // before the runtime stops, so the wait is withdrawn promptly rather than
        // waiting for the idle timeout. Bounded so a dead rendezvous cannot stall.
        self.conn.close(0u32.into(), b"unwait");
        let endpoint = self._endpoint.clone();
        let _ = self._rt.block_on(async move {
            tokio::time::timeout(Duration::from_secs(1), endpoint.wait_idle()).await
        });
    }
}

// Write one rendezvous request and half-close the stream, the convention the
// rendezvous reads with (one request per stream, terminated by the finish); the
// reply is then read to end on the same stream.
async fn write_req(send: &mut quinn::SendStream, req: &Req) -> Result<()> {
    send.write_all(&req.encode()).await.map_err(net)?;
    send.finish().map_err(net)?;
    Ok(())
}

fn unexpected(r: Resp) -> Error {
    match r {
        Resp::Err(s) => Error::Net(s),
        _ => Error::Net("unexpected reply from rendezvous".into()),
    }
}
