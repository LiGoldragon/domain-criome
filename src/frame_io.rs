use std::io::{Read, Write};

use signal_frame::{
    HandshakeRejectionReason, HandshakeReply, ProtocolVersion, SIGNAL_FRAME_PROTOCOL_VERSION,
};

use crate::{Error, Result};

const LENGTH_PREFIX_BYTES: usize = 4;

pub struct FrameBytes {
    bytes: Vec<u8>,
}

impl FrameBytes {
    pub fn read_from(reader: &mut impl Read) -> Result<Self> {
        let mut prefix = [0_u8; LENGTH_PREFIX_BYTES];
        match reader.read_exact(&mut prefix) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(Error::ConnectionClosed);
            }
            Err(error) => return Err(error.into()),
        }
        let length = u32::from_be_bytes(prefix) as usize;
        let mut bytes = Vec::with_capacity(LENGTH_PREFIX_BYTES + length);
        bytes.extend_from_slice(&prefix);
        let mut body = vec![0_u8; length];
        reader.read_exact(&mut body)?;
        bytes.extend_from_slice(&body);
        Ok(Self { bytes })
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
}

pub struct OrdinaryFrameIo<'stream, Stream> {
    stream: &'stream mut Stream,
}

impl<'stream, Stream> OrdinaryFrameIo<'stream, Stream> {
    pub fn new(stream: &'stream mut Stream) -> Self {
        Self { stream }
    }
}

impl<Stream> OrdinaryFrameIo<'_, Stream>
where
    Stream: Read,
{
    pub fn read(&mut self) -> Result<signal_domain_criome::Frame> {
        let bytes = FrameBytes::read_from(self.stream)?;
        Ok(signal_domain_criome::Frame::decode_length_prefixed(
            bytes.as_slice(),
        )?)
    }
}

impl<Stream> OrdinaryFrameIo<'_, Stream>
where
    Stream: Write,
{
    pub fn write(&mut self, frame: &signal_domain_criome::Frame) -> Result<()> {
        let bytes = frame.encode_length_prefixed()?;
        self.stream.write_all(&bytes)?;
        Ok(())
    }
}

pub struct MetaFrameIo<'stream, Stream> {
    stream: &'stream mut Stream,
}

impl<'stream, Stream> MetaFrameIo<'stream, Stream> {
    pub fn new(stream: &'stream mut Stream) -> Self {
        Self { stream }
    }
}

impl<Stream> MetaFrameIo<'_, Stream>
where
    Stream: Read,
{
    pub fn read(&mut self) -> Result<meta_signal_domain_criome::Frame> {
        let bytes = FrameBytes::read_from(self.stream)?;
        Ok(meta_signal_domain_criome::Frame::decode_length_prefixed(
            bytes.as_slice(),
        )?)
    }
}

impl<Stream> MetaFrameIo<'_, Stream>
where
    Stream: Write,
{
    pub fn write(&mut self, frame: &meta_signal_domain_criome::Frame) -> Result<()> {
        let bytes = frame.encode_length_prefixed()?;
        self.stream.write_all(&bytes)?;
        Ok(())
    }
}

pub struct HandshakeCompatibility {
    local: ProtocolVersion,
}

impl HandshakeCompatibility {
    pub fn current() -> Self {
        Self {
            local: SIGNAL_FRAME_PROTOCOL_VERSION,
        }
    }

    pub fn reply_for(&self, peer: ProtocolVersion) -> HandshakeReply {
        if self.local.accepts(peer) {
            HandshakeReply::Accepted(self.local)
        } else {
            HandshakeReply::Rejected(HandshakeRejectionReason::IncompatibleVersion {
                local: self.local,
                peer,
            })
        }
    }
}
