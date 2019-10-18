//! # Psila Service

#![no_std]

use bbqueue;

use psila_data::{CapabilityInformation, ExtendedAddress};

use psila_crypto::CryptoBackend;

mod error;
mod indentity;
mod mac;

pub use error::Error;
use mac::MacService;

/// Short address size
pub const PACKET_BUFFER_MAX: usize = 128;

pub struct PsilaService<CB> {
    crypto: CB,
    mac: MacService,
    capability: CapabilityInformation,
    tx_queue: bbqueue::Producer,
}

impl<CB> PsilaService<CB>
where
    CB: CryptoBackend,
{
    pub fn new(crypto: CB, tx_queue: bbqueue::Producer, address: ExtendedAddress) -> Self {
        let capability = CapabilityInformation {
            alternate_pan_coordinator: false,
            router_capable: false,
            mains_power: true,
            idle_receive: true,
            frame_protection: false,
            allocate_address: true,
        };
        Self {
            crypto,
            mac: MacService::new(address, capability),
            capability,
            tx_queue,
        }
    }

    /// Push a packet onto the queue
    fn queue_packet(&mut self, data: &[u8]) -> Result<(), Error> {
        assert!(data.len() < (u8::max_value() as usize));
        let length = data.len() + 1;
        match self.tx_queue.grant(length) {
            Ok(mut grant) => {
                grant[0] = length as u8;
                grant[1..].copy_from_slice(&data);
                self.tx_queue.commit(data.len(), grant);
                Ok(())
            }
            Err(_) => Err(Error::NotEnoughSpace),
        }
    }

    /// Receive, call this method when new data has been received by the radio
    /// ### Return
    /// A new timeout value that the timer shall be configured with, a timeout
    /// value of zero (0) shall be ignored
    pub fn receive(&mut self, data: &[u8]) -> Result<u32, Error> {
        let mut buffer = [0u8; PACKET_BUFFER_MAX];
        match mac::Frame::decode(data, false) {
            Ok(frame) => {
                if self.mac.requests_acknowledge(&frame) {
                    // If the frame is a data request frame, send an acknowledge with pending set
                    // Use the frame sequence number from the received frame in the acknowledge
                    let packet_length =
                        self.mac
                            .build_acknowledge(frame.header.seq, false, &mut buffer);
                    self.queue_packet(&buffer[..packet_length])?;
                }
                let (packet_length, timeout) = self.mac.handle_frame(&frame, &mut buffer)?;
                if packet_length > 0 {
                    self.queue_packet(&buffer[..packet_length])?;
                }
                Ok(timeout)
            }
            Err(_) => Err(Error::MalformedPacket),
        }
    }

    /// Timeout, call this method when the timer has triggered a time-out
    /// ### Return
    /// A new timeout value that the timer shall be configured with, a timeout
    /// value of zero (0) shall be ignored
    pub fn timeout(&mut self) -> Result<u32, Error> {
        let mut buffer = [0u8; PACKET_BUFFER_MAX];
        let (packet_length, timeout) = self.mac.timeout(&mut buffer)?;
        if packet_length > 0 {
            self.queue_packet(&buffer[..packet_length])?;
        }
        Ok(timeout)
    }
}