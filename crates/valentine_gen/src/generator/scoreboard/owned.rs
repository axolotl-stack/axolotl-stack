/// Operation applied to every row in one SetScore packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum SetScoreAction {
    #[default]
    Change = 0,
    Remove = 1,
}
impl crate::bedrock::codec::BedrockSized for SetScoreAction {
    fn encoded_size(&self) -> usize {
        1
    }
}
impl crate::bedrock::codec::BedrockCodec for SetScoreAction {
    type Args = ();
    const MIN_ENCODED_SIZE: usize = 1;
    fn encode<B: bytes::BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u8(*self as u8);
        Ok(())
    }
    fn decode<B: bytes::Buf>(
        buf: &mut B,
        _: (),
    ) -> Result<Self, crate::bedrock::error::DecodeError> {
        match <u8 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())? {
            0 => Ok(Self::Change),
            1 => Ok(Self::Remove),
            value => Err(crate::bedrock::error::DecodeError::InvalidEnumValue {
                enum_name: "SetScoreAction",
                value: i64::from(value),
            }),
        }
    }
}

/// Identity suffix present only when a score row is changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoreIdentity {
    Player(i64),
    Entity(i64),
    FakePlayer(crate::bedrock::codec::BedrockString),
}
impl crate::bedrock::codec::BedrockSized for ScoreIdentity {
    fn encoded_size(&self) -> usize {
        use crate::bedrock::codec::BedrockSized;
        1 + match self {
            Self::Player(id) | Self::Entity(id) => {
                crate::bedrock::codec::ZigZag64(*id).encoded_size()
            }
            Self::FakePlayer(name) => name.encoded_size(),
        }
    }
}
impl crate::bedrock::codec::BedrockCodec for ScoreIdentity {
    type Args = ();
    const MIN_ENCODED_SIZE: usize = 2;
    fn encode<B: bytes::BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        match self {
            Self::Player(id) => {
                buf.put_u8(1);
                crate::bedrock::codec::ZigZag64(*id).encode(buf)?;
            }
            Self::Entity(id) => {
                buf.put_u8(2);
                crate::bedrock::codec::ZigZag64(*id).encode(buf)?;
            }
            Self::FakePlayer(name) => {
                buf.put_u8(3);
                name.encode(buf)?;
            }
        }
        Ok(())
    }
    fn decode<B: bytes::Buf>(
        buf: &mut B,
        _: (),
    ) -> Result<Self, crate::bedrock::error::DecodeError> {
        match <u8 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())? {
            1 => Ok(Self::Player(
                crate::bedrock::codec::ZigZag64::decode(buf, ())?.0,
            )),
            2 => Ok(Self::Entity(
                crate::bedrock::codec::ZigZag64::decode(buf, ())?.0,
            )),
            3 => Ok(Self::FakePlayer(
                crate::bedrock::codec::BedrockString::decode(buf, ())?,
            )),
            value => Err(crate::bedrock::error::DecodeError::InvalidEnumValue {
                enum_name: "ScoreIdentity",
                value: i64::from(value),
            }),
        }
    }
}

/// Common row data, followed by an identity for Change and no suffix for Remove.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScoreEntry {
    pub scoreboard_id: i64,
    pub objective_name: crate::bedrock::codec::BedrockString,
    /// Raw little-endian score bits; signed scores use their two's-complement bits.
    pub score: u32,
    pub identity: Option<ScoreIdentity>,
}

/// Score operations with a packet-level byte discriminator.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SetScorePacket {
    pub action: SetScoreAction,
    pub entries: Vec<ScoreEntry>,
}
impl SetScorePacket {
    /// Reject inconsistent rows before writing any part of the packet.
    pub fn validate(&self) -> Result<(), std::io::Error> {
        if self.entries.len()>u32::MAX as usize || self.entries.iter().any(|row|
            row.identity.is_some() != (self.action==SetScoreAction::Change)
            || row.objective_name.as_bytes().len()>u32::MAX as usize
            || matches!(&row.identity,Some(ScoreIdentity::FakePlayer(name)) if name.as_bytes().len()>u32::MAX as usize)) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,"score action and row identity must agree and lengths must fit u32"));
        }
        Ok(())
    }
}
impl crate::bedrock::codec::BedrockSized for SetScorePacket {
    fn encoded_size(&self) -> usize {
        use crate::bedrock::codec::BedrockSized;
        1 + crate::bedrock::codec::VarUInt(self.entries.len() as u32).encoded_size()
            + self
                .entries
                .iter()
                .map(|row| {
                    crate::bedrock::codec::ZigZag64(row.scoreboard_id).encoded_size()
                        + row.objective_name.encoded_size()
                        + 4
                        + row
                            .identity
                            .as_ref()
                            .map_or(0, |identity| identity.encoded_size())
                })
                .sum::<usize>()
    }
}
impl crate::bedrock::codec::BedrockCodec for SetScorePacket {
    type Args = ();
    const MIN_ENCODED_SIZE: usize = 2;
    fn encode<B: bytes::BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        self.validate()?;
        self.action.encode(buf)?;
        crate::bedrock::codec::VarUInt(self.entries.len() as u32).encode(buf)?;
        for row in &self.entries {
            crate::bedrock::codec::ZigZag64(row.scoreboard_id).encode(buf)?;
            row.objective_name.encode(buf)?;
            buf.put_u32_le(row.score);
            if let Some(identity) = &row.identity {
                identity.encode(buf)?;
            }
        }
        Ok(())
    }
    fn decode<B: bytes::Buf>(
        buf: &mut B,
        _: (),
    ) -> Result<Self, crate::bedrock::error::DecodeError> {
        let action = SetScoreAction::decode(buf, ())?;
        let len = crate::bedrock::codec::VarUInt::decode(buf, ())?.0 as usize;
        let minimum = if action == SetScoreAction::Change {
            8
        } else {
            6
        };
        let mut entries =
            crate::bedrock::codec::prepare_decode_vec(len, buf.remaining(), Some(minimum))?;
        for _ in 0..len {
            let scoreboard_id = crate::bedrock::codec::ZigZag64::decode(buf, ())?.0;
            let objective_name = crate::bedrock::codec::BedrockString::decode(buf, ())?;
            let score = crate::bedrock::codec::U32LE::decode(buf, ())?.0;
            let identity = if action == SetScoreAction::Change {
                Some(ScoreIdentity::decode(buf, ())?)
            } else {
                None
            };
            entries.push(ScoreEntry {
                scoreboard_id,
                objective_name,
                score,
                identity,
            });
        }
        Ok(Self { action, entries })
    }
}
