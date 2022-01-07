use std::io;

use bytes::BytesMut;
use log::{debug, info};
use mqttbytes::{v4::*, *};

// use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use crate::rumq::{Incoming, MqttState, StateError};

/// Network transforms packets <-> frames efficiently. It takes
/// advantage of pre-allocation, buffering and vectorization when
/// appropriate to achieve performance
pub struct Network {
    /// Socket for IO
    socket: Box<dyn N>,
    /// Buffered reads
    read: BytesMut,
    /// Maximum packet size
    max_incoming_size: usize,
    /// Maximum readv count
    max_readb_count: usize,
}

impl Network {
    pub fn new(socket: impl N + 'static, max_incoming_size: usize) -> Network {
        let socket = Box::new(socket) as Box<dyn N>;
        Network {
            socket,
            read: BytesMut::with_capacity(10 * 1024),
            max_incoming_size,
            max_readb_count: 10,
        }
    }

    /// Reads more than 'required' bytes to frame a packet into self.read buffer
    fn read_bytes(&mut self, required: usize) -> io::Result<usize> {
        let mut total_read = 0;
        loop {
            let read = self.socket.read(&mut self.read)?;
            // TODO recreate error handling

            // if 0 == read {
            //     return if self.read.is_empty() {
            //         Err(io::Error::new(
            //             io::ErrorKind::ConnectionAborted,
            //             "connection closed by peer",
            //         ))
            //     } else {
            //         Err(io::Error::new(
            //             io::ErrorKind::ConnectionReset,
            //             "connection reset by peer",
            //         ))
            //     };
            // }

            total_read += read;
            if total_read >= required {
                return Ok(total_read);
            }
        }
    }

    pub fn read(&mut self) -> Result<Incoming, io::Error> {
        loop {
            info!("hey read()");
            let required = match read(&mut self.read, self.max_incoming_size) {
                Ok(packet) => return Ok(packet),
                Err(mqttbytes::Error::InsufficientBytes(required)) => required,
                Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
            };

            // read more packets until a frame can be created. This function
            // blocks until a frame can be created. Use this in a select! branch
            self.read_bytes(required)?;
        }
    }

    /// Read packets in bulk. This allow replies to be in bulk. This method is used
    /// after the connection is established to read a bunch of incoming packets
    pub fn readb(&mut self, state: &mut MqttState) -> Result<(), StateError> {
        let mut count = 0;
        loop {
            match read(&mut self.read, self.max_incoming_size) {
                Ok(packet) => {
                    state.handle_incoming_packet(packet)?;

                    count += 1;
                    if count >= self.max_readb_count {
                        return Ok(());
                    }
                }
                // If some packets are already framed, return those
                Err(Error::InsufficientBytes(_)) if count > 0 => return Ok(()),
                // Wait for more bytes until a frame can be created
                Err(Error::InsufficientBytes(required)) => {
                    self.read_bytes(required)?;
                }
                Err(e) => return Err(StateError::Deserialization(e)),
            };
        }
    }

    pub fn connect(&mut self, connect: Connect) -> Result<usize, io::Error> {
        let mut write = BytesMut::new();
        let len = match connect.write(&mut write) {
            Ok(size) => size,
            Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
        };

        self.socket.write_all(&write[..])?;
        Ok(len)
    }

    pub fn flush(&mut self, write: &mut BytesMut) -> Result<(), io::Error> {
        if write.is_empty() {
            return Ok(());
        }

        self.socket.write_all(&write[..])?;
        write.clear();
        Ok(())
    }
}

use std::io::prelude::*;
pub trait N: Read + Write + Send {}
impl<T> N for T where T: Read + Write + Send {}
