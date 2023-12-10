use std::time::Duration;

use crate::{
    enet_peer_disconnect, enet_peer_disconnect_later, enet_peer_disconnect_now, enet_peer_ping,
    enet_peer_ping_interval, enet_peer_reset, enet_peer_send, enet_peer_throttle_configure,
    enet_peer_timeout, ENetPeer, Error, Packet, Socket, ENET_PEER_STATE_ACKNOWLEDGING_CONNECT,
    ENET_PEER_STATE_ACKNOWLEDGING_DISCONNECT, ENET_PEER_STATE_CONNECTED,
    ENET_PEER_STATE_CONNECTING, ENET_PEER_STATE_CONNECTION_PENDING,
    ENET_PEER_STATE_CONNECTION_SUCCEEDED, ENET_PEER_STATE_DISCONNECTED,
    ENET_PEER_STATE_DISCONNECTING, ENET_PEER_STATE_DISCONNECT_LATER, ENET_PEER_STATE_ZOMBIE,
    ENET_PROTOCOL_MAXIMUM_PEER_ID,
};

/// A newtype around a `usize`, representing a unique identifier for a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerID(pub usize);

impl PeerID {
    /// The minimum valid value a [`PeerID`]` can be.
    pub const MIN: usize = 0;
    /// The maximum valid value a [`PeerID`]` can be.
    pub const MAX: usize = ENET_PROTOCOL_MAXIMUM_PEER_ID as usize;
}

/// The state of a [`Peer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum PeerState {
    Disconnected,
    Connecting,
    AcknowledgingConnect,
    ConnectionPending,
    ConnectionSucceeded,
    Connected,
    DisconnectLater,
    Disconnecting,
    AcknowledgingDisconnect,
    Zombie,
}

/// A peer, associated with a [`Host`](`crate::Host`), which may or may not be connected.
pub struct Peer<S: Socket>(pub(crate) *mut ENetPeer<S>);

impl<S: Socket> Peer<S> {
    /// Get the [`PeerID`] of this peer.
    pub fn id(&self) -> PeerID {
        PeerID(unsafe { self.0.offset_from((*(*self.0).host).peers) as usize })
    }

    /// Sends a ping request to a peer.
    ///
    /// Ping requests factor into the mean round trip time as acquired by
    /// [`Peer::round_trip_time`]. ENet automatically pings all connected peers at regular
    /// intervals, however, this function may be called to ensure more frequent ping requests.
    ///
    /// The ping interval can be changed with [`Self::set_ping_interval`].
    pub fn ping(&mut self) {
        unsafe { enet_peer_ping(self.0) }
    }

    /// Queues a packet to be sent to this peer on the specified channel.
    ///
    /// # Errors
    ///
    /// Returns [`Error::FailedToSend`] if the underlying ENet call failed.
    pub fn send(&mut self, channel_id: u8, packet: Packet) -> Result<(), Error> {
        unsafe {
            if enet_peer_send(self.0, channel_id, packet.packet) == 0 {
                Ok(())
            } else {
                Err(Error::FailedToSend)
            }
        }
    }

    /// Request a disconnection from a peer.
    ///
    /// An [`Event::Disconnect`](`crate::Event::Disconnect`) event will be generated by
    /// [`Host::service`](`crate::Host::service`) once the disconnection is complete.
    pub fn disconnect(&mut self, data: u32) {
        unsafe { enet_peer_disconnect(self.0, data) }
    }

    /// Force an immediate disconnection from a peer.
    ///
    /// No [`Event::Disconnect`](`crate::Event::Disconnect`) event will be generated. The foreign
    /// peer is not guaranteed to receive the disconnect notification, and is reset immediately upon
    /// return from this function.
    pub fn disconnect_now(&mut self, data: u32) {
        unsafe { enet_peer_disconnect_now(self.0, data) }
    }

    /// Request a disconnection from a peer, but only after all queued outgoing packets are sent.
    ///
    /// An [`Event::Disconnect`](`crate::Event::Disconnect`) event will be generated by
    /// [`Host::service`](`crate::Host::service`) once the disconnection is complete.
    pub fn disconnect_later(&mut self, data: u32) {
        unsafe { enet_peer_disconnect_later(self.0, data) }
    }

    /// Forcefully disconnects a peer.
    ///
    /// The foreign host represented by the peer is not notified of the disconnection and will
    /// timeout on its connection to the local host.
    pub fn reset(&mut self) {
        unsafe {
            enet_peer_reset(self.0);
        }
    }

    /// Timeout parameters to control how and when a peer will timeout from a failure to
    /// acknowledge reliable traffic.
    ///
    /// Timeout values use an exponential backoff mechanism, where if a reliable packet is not
    /// acknowledge within some multiple of the average RTT plus a variance tolerance, the timeout
    /// will be doubled until it reaches a set limit. If the timeout is thus at this limit and
    /// reliable packets have been sent but not acknowledged within a certain minimum time period,
    /// the peer will be disconnected. Alternatively, if reliable packets have been sent but not
    /// acknowledged for a certain maximum time period, the peer will be disconnected regardless of
    /// the current timeout limit value.
    ///
    /// - `limit` - the timeout limit; defaults to
    /// [`ENET_PEER_TIMEOUT_LIMIT`](`crate::consts::ENET_PEER_TIMEOUT_LIMIT`) if 0
    /// - `minimum` - the timeout minimum; defaults to
    /// [`ENET_PEER_TIMEOUT_MINIMUM`](`crate::consts::ENET_PEER_TIMEOUT_MINIMUM`) if 0
    /// - `maximum` - the timeout maximum; defaults to
    /// [`ENET_PEER_TIMEOUT_MAXIMUM`](`crate::consts::ENET_PEER_TIMEOUT_MAXIMUM`) if 0
    pub fn set_timeout(&mut self, limit: u32, minimum: u32, maximum: u32) {
        unsafe { enet_peer_timeout(self.0, limit, minimum, maximum) }
    }

    /// Sets the interval at which pings will be sent to a peer in milliseconds.
    ///
    /// Pings are used both to monitor the liveness of the connection and also to dynamically adjust
    /// the throttle during periods of low traffic so that the throttle has reasonable
    /// responsiveness during traffic spikes.
    ///
    /// See [`Peer::ping`].
    pub fn set_ping_interval(&mut self, ping_interval: u32) {
        unsafe { enet_peer_ping_interval(self.0, ping_interval) }
    }

    /// Configure the peer's throttle parameters.
    ///
    /// Unreliable packets are dropped by ENet in response to the varying conditions of the
    /// Internet connection to the peer. The throttle represents a probability that an unreliable
    /// packet should not be dropped and thus sent by ENet to the peer. The lowest mean round trip
    /// time from the sending of a reliable packet to the receipt of its acknowledgement is measured
    /// over an amount of time specified by the interval parameter in milliseconds. If a measured
    /// round trip time happens to be significantly less than the mean round trip time measured over
    /// the interval, then the throttle probability is increased to allow more traffic by an amount
    /// specified in the acceleration parameter, which is a ratio to the
    /// [`ENET_PEER_PACKET_THROTTLE_SCALE`](`crate::consts::ENET_PEER_PACKET_THROTTLE_SCALE`)
    /// constant. If a measured round trip time happens to be significantly greater than the mean
    /// round trip time measured over the interval, then the throttle probability is decreased to
    /// limit traffic by an amount specified in the deceleration parameter, which is a ratio to the
    /// [`ENET_PEER_PACKET_THROTTLE_SCALE`](`crate::consts::ENET_PEER_PACKET_THROTTLE_SCALE`) When
    /// the throttle has a value of
    /// [`ENET_PEER_PACKET_THROTTLE_SCALE`](`crate::consts::ENET_PEER_PACKET_THROTTLE_SCALE`) When
    /// no unreliable packets are dropped by ENet, and so 100% of all unreliable packets will be
    /// sent. When the throttle has a value of 0, all unreliable packets are dropped by ENet, and so
    /// 0% of all unreliable packets will be sent. Intermediate values for the throttle represent
    /// intermediate probabilities between 0% and 100% of unreliable packets being sent. The
    /// bandwidth limits of the local and foreign hosts are taken into account to determine a
    /// sensible limit for the throttle probability above which it should not raise even in the best
    /// of conditions.
    ///
    /// - `interval` - interval, in milliseconds, over which to measure lowest mean RTT; the default
    /// value is
    /// [`ENET_PEER_PACKET_THROTTLE_INTERVAL`](`crate::consts::ENET_PEER_PACKET_THROTTLE_INTERVAL`)
    /// - `acceleration` - rate at which to increase the throttle probability as mean RTT declines
    /// - `deceleration` - rate at which to decrease the throttle probability as mean RTT increases
    pub fn set_throttle(&mut self, interval: u32, acceleration: u32, deceleration: u32) {
        unsafe { enet_peer_throttle_configure(self.0, interval, acceleration, deceleration) }
    }

    /// Get the current state of the peer.
    pub fn state(&self) -> PeerState {
        unsafe {
            match (*self.0).state {
                ENET_PEER_STATE_ZOMBIE => PeerState::Zombie,
                ENET_PEER_STATE_ACKNOWLEDGING_DISCONNECT => PeerState::AcknowledgingDisconnect,
                ENET_PEER_STATE_DISCONNECTING => PeerState::Disconnecting,
                ENET_PEER_STATE_DISCONNECT_LATER => PeerState::DisconnectLater,
                ENET_PEER_STATE_CONNECTED => PeerState::Connected,
                ENET_PEER_STATE_CONNECTION_SUCCEEDED => PeerState::ConnectionSucceeded,
                ENET_PEER_STATE_CONNECTION_PENDING => PeerState::ConnectionPending,
                ENET_PEER_STATE_ACKNOWLEDGING_CONNECT => PeerState::AcknowledgingConnect,
                ENET_PEER_STATE_CONNECTING => PeerState::Connecting,
                ENET_PEER_STATE_DISCONNECTED => PeerState::Disconnected,
                _ => unreachable!(),
            }
        }
    }

    /// Check if this peer's state is [`PeerState::Connected`].
    pub fn connected(self) -> bool {
        self.state() == PeerState::Connected
    }

    /// Number of channels allocated for communication with peer.
    pub fn channel_count(&self) -> usize {
        unsafe { (*self.0).channelCount }
    }

    /// Downstream bandwidth of the client in bytes/second.
    pub fn incoming_bandwidth(&self) -> u32 {
        unsafe { (*self.0).incomingBandwidth }
    }

    /// Upstream bandwidth of the client in bytes/second.
    pub fn outgoing_bandwidth(&self) -> u32 {
        unsafe { (*self.0).outgoingBandwidth }
    }

    /// Total amount of downstream data received.
    pub fn incoming_data_total(&self) -> u32 {
        unsafe { (*self.0).incomingDataTotal }
    }

    /// Total amount of upstream data sent.
    pub fn outgoing_data_total(&self) -> u32 {
        unsafe { (*self.0).outgoingDataTotal }
    }

    /// Total number of packets sent.
    pub fn packets_sent(&self) -> u32 {
        unsafe { (*self.0).packetsSent }
    }

    /// Total number of packets lost.
    pub fn packets_lost(&self) -> u32 {
        unsafe { (*self.0).packetsLost }
    }

    /// Mean packet loss of reliable packets as a ratio with respect to the constant
    /// [`ENET_PEER_PACKET_LOSS_SCALE`](crate::consts::ENET_PEER_PACKET_LOSS_SCALE).
    pub fn packet_loss(&self) -> u32 {
        unsafe { (*self.0).packetLoss }
    }

    /// Variance of the mean packet loss.
    pub fn packet_loss_variance(&self) -> u32 {
        unsafe { (*self.0).packetLossVariance }
    }

    /// Ping interval. See [`Peer::set_ping_interval`].
    pub fn ping_interval(&self) -> Duration {
        Duration::from_millis(unsafe { (*self.0).pingInterval } as u64)
    }

    /// Mean round trip time (RTT), between sending a reliable packet and receiving its
    /// acknowledgement.
    pub fn round_trip_time(&self) -> Duration {
        Duration::from_millis(unsafe { (*self.0).roundTripTime } as u64)
    }

    /// Round trip time (RTT) variance. See [`Peer::round_trip_time`].
    pub fn round_trip_time_variance(&self) -> Duration {
        Duration::from_millis(unsafe { (*self.0).roundTripTimeVariance } as u64)
    }
}
