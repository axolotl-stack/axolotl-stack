/// Borrowed identity suffix for one changed scoreboard row.
#[derive(Debug, Clone, PartialEq)]
pub enum ScoreIdentityView {
    Player(i64),
    Entity(i64),
    FakePlayer(crate::bedrock::borrowed::BorrowedStr),
}
/// Borrowed common row fields and optional action-dependent identity suffix.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreEntryView {
    pub scoreboard_id: i64,
    pub objective_name: crate::bedrock::borrowed::BorrowedStr,
    pub score: u32,
    pub identity: Option<ScoreIdentityView>,
}
/// Borrowed SetScore body, retaining string bytes until converted to owned values.
#[derive(Debug, Clone, PartialEq)]
pub struct SetScorePacketView {
    pub action: SetScoreAction,
    pub entries: Vec<ScoreEntryView>,
}
impl SetScorePacketView {
    /// Decode one body using the packet action to select each row's suffix.
    pub fn decode<B: bytes::Buf>(buf: &mut B) -> Result<Self, crate::bedrock::error::DecodeError> {
        <Self as crate::bedrock::borrowed::BedrockBorrowDecode>::borrow_decode(buf, ())
    }
    /// Encode checked rows without allocating owned copies of their strings.
    pub fn encode<B: bytes::BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        if self.entries.len()>u32::MAX as usize || self.entries.iter().any(|row|
            row.identity.is_some() != (self.action==SetScoreAction::Change)
            || row.objective_name.as_bytes().len()>u32::MAX as usize
            || matches!(&row.identity,Some(ScoreIdentityView::FakePlayer(name)) if name.as_bytes().len()>u32::MAX as usize)) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,"score action and row identity must agree and lengths must fit u32"));
        }
        self.action.encode(buf)?;
        crate::bedrock::codec::VarUInt(self.entries.len() as u32).encode(buf)?;
        for row in &self.entries {
            crate::bedrock::codec::ZigZag64(row.scoreboard_id).encode(buf)?;
            crate::bedrock::codec::VarUInt(row.objective_name.as_bytes().len() as u32)
                .encode(buf)?;
            buf.put_slice(row.objective_name.as_bytes());
            buf.put_u32_le(row.score);
            match &row.identity {
                Some(ScoreIdentityView::Player(id)) => {
                    buf.put_u8(1);
                    crate::bedrock::codec::ZigZag64(*id).encode(buf)?;
                }
                Some(ScoreIdentityView::Entity(id)) => {
                    buf.put_u8(2);
                    crate::bedrock::codec::ZigZag64(*id).encode(buf)?;
                }
                Some(ScoreIdentityView::FakePlayer(name)) => {
                    buf.put_u8(3);
                    crate::bedrock::codec::VarUInt(name.as_bytes().len() as u32).encode(buf)?;
                    buf.put_slice(name.as_bytes());
                }
                None => {}
            }
        }
        Ok(())
    }
}
impl crate::bedrock::codec::BedrockSized for SetScorePacketView {
    fn encoded_size(&self) -> usize {
        use crate::bedrock::codec::BedrockSized;
        1 + crate::bedrock::codec::VarUInt(self.entries.len() as u32).encoded_size()
            + self
                .entries
                .iter()
                .map(|row| {
                    crate::bedrock::codec::ZigZag64(row.scoreboard_id).encoded_size()
                        + crate::bedrock::codec::VarUInt(row.objective_name.as_bytes().len() as u32)
                            .encoded_size()
                        + row.objective_name.as_bytes().len()
                        + 4
                        + match &row.identity {
                            Some(ScoreIdentityView::Player(id) | ScoreIdentityView::Entity(id)) => {
                                1 + crate::bedrock::codec::ZigZag64(*id).encoded_size()
                            }
                            Some(ScoreIdentityView::FakePlayer(name)) => {
                                1 + crate::bedrock::codec::VarUInt(name.as_bytes().len() as u32)
                                    .encoded_size()
                                    + name.as_bytes().len()
                            }
                            None => 0,
                        }
                })
                .sum::<usize>()
    }
}
impl crate::bedrock::borrowed::BedrockBorrowDecode for SetScorePacketView {
    type Args = ();
    fn borrow_decode<B: bytes::Buf>(
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
            let objective_name = crate::bedrock::borrowed::take_var_u32_prefixed_string(buf)?;
            let score = crate::bedrock::codec::U32LE::decode(buf, ())?.0;
            let identity = if action == SetScoreAction::Change {
                Some(
                    match <u8 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())? {
                        1 => ScoreIdentityView::Player(
                            crate::bedrock::codec::ZigZag64::decode(buf, ())?.0,
                        ),
                        2 => ScoreIdentityView::Entity(
                            crate::bedrock::codec::ZigZag64::decode(buf, ())?.0,
                        ),
                        3 => ScoreIdentityView::FakePlayer(
                            crate::bedrock::borrowed::take_var_u32_prefixed_string(buf)?,
                        ),
                        value => {
                            return Err(crate::bedrock::error::DecodeError::InvalidEnumValue {
                                enum_name: "ScoreIdentity",
                                value: i64::from(value),
                            });
                        }
                    },
                )
            } else {
                None
            };
            entries.push(ScoreEntryView {
                scoreboard_id,
                objective_name,
                score,
                identity,
            });
        }
        Ok(Self { action, entries })
    }
}
impl From<SetScorePacketView> for SetScorePacket {
    fn from(value: SetScorePacketView) -> Self {
        Self {
            action: value.action,
            entries: value
                .entries
                .into_iter()
                .map(|row| ScoreEntry {
                    scoreboard_id: row.scoreboard_id,
                    objective_name: row.objective_name.to_string_lossy().into_owned().into(),
                    score: row.score,
                    identity: row.identity.map(|identity| match identity {
                        ScoreIdentityView::Player(id) => ScoreIdentity::Player(id),
                        ScoreIdentityView::Entity(id) => ScoreIdentity::Entity(id),
                        ScoreIdentityView::FakePlayer(name) => {
                            ScoreIdentity::FakePlayer(name.to_string_lossy().into_owned().into())
                        }
                    }),
                })
                .collect(),
        }
    }
}
pub type BorrowedSetScorePacket = SetScorePacketView;
