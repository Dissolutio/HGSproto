use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, HashMap};
use std::io::{self, Read, Write};

pub const LARGE_FRAME_CAP: usize = 1024 * 1024;
pub const DEFAULT_FRAME_CAP: usize = 16 * 1024;
pub const FRAME_CAPS: &[(&str, usize)] = &[
    ("SETUP", LARGE_FRAME_CAP),
    ("SNAPSHOT", LARGE_FRAME_CAP),
    ("CATCH_UP", LARGE_FRAME_CAP),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Version;

impl TryFrom<u8> for Version {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value == 1 {
            Ok(Self)
        } else {
            Err("unsupported protocol version")
        }
    }
}

impl From<Version> for u8 {
    fn from(_: Version) -> Self {
        1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Seat {
    P1,
    P2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub v: Version,
    pub cmd_id: String,
    pub base_seq: u64,
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roll {
    pub k: u64,
    pub r_host: String,
    pub r_guest: String,
    pub roll_seed: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Accepted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultMessage {
    pub v: Version,
    pub seq: u64,
    pub cmd_id: String,
    pub seat: Seat,
    pub token: String,
    pub outcome: Outcome,
    pub roll: Option<Roll>,
    pub state_hash: String,
    pub chain: String,
    pub next_commit: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingRoll {
    pub v: Version,
    pub seq: u64,
    pub k: u64,
    pub cmd_id: String,
    pub seat: Seat,
    pub token: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalReason {
    Stale,
    RollPending,
    NotYourTurn,
    Illegal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refused {
    pub v: Version,
    pub cmd_id: String,
    pub base_seq: u64,
    pub reason: RefusalReason,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reveal {
    pub v: Version,
    pub seq: u64,
    pub k: u64,
    pub r: String,
    pub next_commit: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ack {
    pub v: Version,
    pub seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resume {
    pub v: Version,
    pub match_fp: String,
    pub last_seq: u64,
    pub last_chain: String,
    pub pending: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ResultFrame {
    #[serde(rename = "RESULT")]
    Result(ResultMessage),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatchUp {
    pub v: Version,
    pub from_seq: u64,
    pub results: Vec<ResultFrame>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub v: Version,
    pub seq: u64,
    pub chain: String,
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Message {
    #[serde(rename = "CMD")]
    Cmd(Command),
    #[serde(rename = "REVEAL")]
    Reveal(Reveal),
    #[serde(rename = "ACK")]
    Ack(Ack),
    #[serde(rename = "RESUME")]
    Resume(Resume),
    #[serde(rename = "RESULT")]
    Result(ResultMessage),
    #[serde(rename = "REFUSED")]
    Refused(Refused),
    #[serde(rename = "PENDING_ROLL")]
    PendingRoll(PendingRoll),
    #[serde(rename = "CATCH_UP")]
    CatchUp(CatchUp),
    #[serde(rename = "SNAPSHOT")]
    Snapshot(Snapshot),
}

#[derive(Debug)]
pub enum FrameError {
    TooLarge,
    Io(io::Error),
    Json(serde_json::Error),
    MissingType,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => f.write_str("E:FRAME_TOO_LARGE"),
            Self::Io(e) => e.fmt(f),
            Self::Json(e) => e.fmt(f),
            Self::MissingType => f.write_str("missing message type"),
        }
    }
}
impl std::error::Error for FrameError {}

fn check_cap(value: &serde_json::Value, len: usize) -> Result<(), FrameError> {
    let kind = value
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or(FrameError::MissingType)?;
    let cap = FRAME_CAPS
        .iter()
        .find(|(name, _)| *name == kind)
        .map_or(DEFAULT_FRAME_CAP, |(_, cap)| *cap);
    if len > cap {
        Err(FrameError::TooLarge)
    } else {
        Ok(())
    }
}

pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T, FrameError> {
    let mut prefix = [0; 4];
    reader.read_exact(&mut prefix).map_err(FrameError::Io)?;
    let len = u32::from_be_bytes(prefix) as usize;
    if len > LARGE_FRAME_CAP {
        return Err(FrameError::TooLarge);
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body).map_err(FrameError::Io)?;
    let value: serde_json::Value = serde_json::from_slice(&body).map_err(FrameError::Json)?;
    check_cap(&value, len)?;
    serde_json::from_slice(&body).map_err(FrameError::Json)
}

pub fn write_frame<T: Serialize>(writer: &mut impl Write, message: &T) -> Result<(), FrameError> {
    let body = serde_json::to_vec(message).map_err(FrameError::Json)?;
    if body.len() > LARGE_FRAME_CAP {
        return Err(FrameError::TooLarge);
    }
    let value: serde_json::Value = serde_json::from_slice(&body).map_err(FrameError::Json)?;
    check_cap(&value, body.len())?;
    writer
        .write_all(&(body.len() as u32).to_be_bytes())
        .map_err(FrameError::Io)?;
    writer.write_all(&body).map_err(FrameError::Io)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum HostReply {
    #[serde(rename = "RESULT")]
    Result(ResultMessage),
    #[serde(rename = "PENDING_ROLL")]
    PendingRoll(PendingRoll),
    #[serde(rename = "REFUSED")]
    Refused(Refused),
}

pub enum RefereeDecision {
    Accepted { state_hash: String, chain: String },
    PendingRoll { k: u64 },
    Illegal { message: String },
    NotYourTurn { message: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogEntry {
    Pending(PendingRoll),
    Result(ResultMessage),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostLog {
    head_seq: u64,
    entries: Vec<LogEntry>,
    replies: HashMap<String, HostReply>,
    pending: Option<PendingRoll>,
}

impl HostLog {
    pub fn head_seq(&self) -> u64 {
        self.head_seq
    }
    pub fn entries(&self) -> &[LogEntry] {
        &self.entries
    }

    pub fn command(
        &mut self,
        command: &Command,
        seat: Seat,
        referee: impl FnOnce(Seat, &str) -> RefereeDecision,
    ) -> HostReply {
        if let Some(reply) = self.replies.get(&command.cmd_id) {
            return reply.clone();
        }
        let refuse = |reason, message: String| {
            HostReply::Refused(Refused {
                v: Version,
                cmd_id: command.cmd_id.clone(),
                base_seq: command.base_seq,
                reason,
                message,
            })
        };
        if self.pending.is_some() {
            return refuse(RefusalReason::RollPending, "roll_pending".into());
        }
        if command.base_seq != self.head_seq {
            return refuse(RefusalReason::Stale, "stale".into());
        }
        let seq = self.head_seq.checked_add(1).expect("sequence exhausted");
        let reply = match referee(seat, &command.token) {
            RefereeDecision::Illegal { message } => refuse(RefusalReason::Illegal, message),
            RefereeDecision::NotYourTurn { message } => refuse(RefusalReason::NotYourTurn, message),
            RefereeDecision::PendingRoll { k } => {
                let pending = PendingRoll {
                    v: Version,
                    seq,
                    k,
                    cmd_id: command.cmd_id.clone(),
                    seat,
                    token: command.token.clone(),
                };
                self.head_seq = seq;
                self.entries.push(LogEntry::Pending(pending.clone()));
                self.pending = Some(pending.clone());
                HostReply::PendingRoll(pending)
            }
            RefereeDecision::Accepted { state_hash, chain } => {
                let result = ResultMessage {
                    v: Version,
                    seq,
                    cmd_id: command.cmd_id.clone(),
                    seat,
                    token: command.token.clone(),
                    outcome: Outcome::Accepted,
                    roll: None,
                    state_hash,
                    chain,
                    next_commit: None,
                };
                self.head_seq = seq;
                self.entries.push(LogEntry::Result(result.clone()));
                HostReply::Result(result)
            }
        };
        self.replies.insert(command.cmd_id.clone(), reply.clone());
        reply
    }

    pub fn resolve_roll(
        &mut self,
        roll: Roll,
        state_hash: String,
        chain: String,
        next_commit: String,
    ) -> Option<HostReply> {
        let pending = self.pending.as_ref()?;
        if pending.k != roll.k {
            return None;
        }
        let result = ResultMessage {
            v: Version,
            seq: pending.seq,
            cmd_id: pending.cmd_id.clone(),
            seat: pending.seat,
            token: pending.token.clone(),
            outcome: Outcome::Accepted,
            roll: Some(roll),
            state_hash,
            chain,
            next_commit: Some(next_commit),
        };
        self.entries.push(LogEntry::Result(result.clone()));
        let reply = HostReply::Result(result);
        self.replies.insert(pending.cmd_id.clone(), reply.clone());
        self.pending = None;
        Some(reply)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Outbound {
    command: Command,
    used: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestApplier {
    applied_seq: u64,
    chain: String,
    outbound: Vec<Outbound>,
    buffer: BTreeMap<u64, ResultMessage>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum GuestError<E> {
    Forged,
    ConflictingResult,
    Verification(E),
}

impl GuestApplier {
    pub fn new(applied_seq: u64, chain: String) -> Self {
        Self {
            applied_seq,
            chain,
            outbound: Vec::new(),
            buffer: BTreeMap::new(),
        }
    }
    pub fn applied_seq(&self) -> u64 {
        self.applied_seq
    }
    pub fn chain(&self) -> &str {
        &self.chain
    }
    pub fn resume_needed(&self) -> bool {
        !self.buffer.is_empty()
    }
    pub fn record_outbound(&mut self, command: Command) {
        let used = self
            .outbound
            .iter()
            .any(|entry| entry.command.cmd_id == command.cmd_id && entry.used);
        self.outbound.push(Outbound { command, used });
    }
    fn matches_outbound(&self, seq: u64, cmd_id: &str, token: &str) -> bool {
        self.outbound.iter().any(|entry| {
            !entry.used
                && entry.command.cmd_id == cmd_id
                && entry.command.token == token
                && seq.checked_sub(1) == Some(entry.command.base_seq)
        })
    }
    pub fn check_pending(&self, pending: &PendingRoll) -> bool {
        pending.seat != Seat::P2
            || self.matches_outbound(pending.seq, &pending.cmd_id, &pending.token)
    }
    pub fn receive<E>(
        &mut self,
        result: ResultMessage,
        mut verify_apply: impl FnMut(&ResultMessage) -> Result<(), E>,
    ) -> Result<(), GuestError<E>> {
        if result.seq <= self.applied_seq {
            return Ok(());
        }
        if result.seat == Seat::P2
            && !self.matches_outbound(result.seq, &result.cmd_id, &result.token)
        {
            return Err(GuestError::Forged);
        }
        if let Some(existing) = self.buffer.get(&result.seq) {
            if existing != &result {
                return Err(GuestError::ConflictingResult);
            }
        } else {
            self.buffer.insert(result.seq, result);
        }
        while let Some(next) = self.applied_seq.checked_add(1) {
            let Some(result) = self.buffer.get(&next) else {
                break;
            };
            if result.seat == Seat::P2
                && !self.matches_outbound(result.seq, &result.cmd_id, &result.token)
            {
                return Err(GuestError::Forged);
            }
            verify_apply(result).map_err(GuestError::Verification)?;
            let result = self.buffer.remove(&next).expect("buffered result");
            if result.seat == Seat::P2 {
                for entry in &mut self.outbound {
                    if entry.command.cmd_id == result.cmd_id {
                        entry.used = true;
                    }
                }
            }
            self.applied_seq = next;
            self.chain = result.chain;
        }
        Ok(())
    }
}
