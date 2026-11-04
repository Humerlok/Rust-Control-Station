//! Mission, fence and rally transfer (the MAVLink mission microservice) as a service next to
//! [`LinkManager`], using only its public API. Items travel as MISSION_ITEM_INT.
use std::{sync::Arc, time::Duration};

use control_station_mavlink::{
    dialect::{
        MavCmd, MavFrame, MavMissionResult, MavMissionType, MISSION_ACK_DATA,
        MISSION_CLEAR_ALL_DATA, MISSION_COUNT_DATA, MISSION_ITEM_INT_DATA,
        MISSION_REQUEST_INT_DATA, MISSION_REQUEST_LIST_DATA,
    },
    MavMessage, Packet,
};
use control_station_mission::MissionItem;
use control_station_vehicle_state::VehicleId;
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{broadcast, Mutex as AsyncMutex},
    time::{timeout_at, Instant},
};

use crate::{LinkError, LinkManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MissionKind {
    Mission,
    Fence,
    Rally,
}

impl MissionKind {
    fn wire(self) -> MavMissionType {
        match self {
            Self::Mission => MavMissionType::MAV_MISSION_TYPE_MISSION,
            Self::Fence => MavMissionType::MAV_MISSION_TYPE_FENCE,
            Self::Rally => MavMissionType::MAV_MISSION_TYPE_RALLY,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MissionPhase {
    Download,
    Upload,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionProgress {
    pub kind: MissionKind,
    pub phase: MissionPhase,
    pub done: u32,
    pub total: u32,
    pub finished: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct MissionTiming {
    /// Wait for MISSION_COUNT / the first item request, per try.
    pub count_timeout: Duration,
    pub download_count_tries: u32,
    pub upload_count_tries: u32,
    /// Wait for one item (download) or the next request (upload), per try.
    pub item_timeout: Duration,
    pub item_tries: u32,
    /// Wait for the ACK of MISSION_CLEAR_ALL, per try.
    pub clear_timeout: Duration,
    pub clear_tries: u32,
}

impl Default for MissionTiming {
    fn default() -> Self {
        Self {
            count_timeout: Duration::from_millis(700),
            download_count_tries: 6,
            upload_count_tries: 3,
            item_timeout: Duration::from_millis(2500),
            item_tries: 5,
            clear_timeout: Duration::from_millis(1500),
            clear_tries: 3,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MissionError {
    #[error("{0}")]
    Link(#[from] LinkError),
    #[error("another mission transfer is already in progress")]
    Busy,
    #[error("the vehicle did not answer the request for the item count (no MISSION_COUNT)")]
    NoCount,
    #[error("the vehicle did not request the first mission item")]
    NoRequest,
    #[error("no answer from the vehicle for mission item {seq}")]
    ItemTimeout { seq: u16 },
    #[error("timed out waiting for the vehicle after item {seq}")]
    UploadStalled { seq: u16 },
    #[error("the vehicle did not confirm the mission (no MISSION_ACK)")]
    NoAck,
    #[error("the vehicle refused the mission: {0}")]
    Refused(String),
    #[error("item {seq}: command {command} is not a known MAV_CMD")]
    UnknownCommand { seq: usize, command: u16 },
    #[error("item {seq}: frame {frame} is not a known MAV_FRAME")]
    UnknownFrame { seq: usize, frame: u8 },
    #[error("too many items ({0}); a mission holds at most 65535")]
    TooMany(usize),
    #[error("the vehicle asked for item {requested} but the list has {count}")]
    BadRequest { requested: u16, count: usize },
}

struct Inner {
    link: LinkManager,
    progress: broadcast::Sender<MissionProgress>,
    op_lock: AsyncMutex<()>,
    timing: MissionTiming,
}

#[derive(Clone)]
pub struct MissionService {
    inner: Arc<Inner>,
}

type Packets = broadcast::Receiver<Arc<Packet>>;

fn degrees_to_e7(degrees: f64) -> i32 {
    (degrees * 1e7).round() as i32
}

fn to_wire(
    index: usize,
    item: &MissionItem,
    kind: MissionKind,
    vehicle: VehicleId,
) -> Result<MISSION_ITEM_INT_DATA, MissionError> {
    let command =
        MavCmd::from_u32(u32::from(item.command)).ok_or(MissionError::UnknownCommand {
            seq: index,
            command: item.command,
        })?;
    let frame = MavFrame::from_u32(u32::from(item.frame)).ok_or(MissionError::UnknownFrame {
        seq: index,
        frame: item.frame,
    })?;
    Ok(MISSION_ITEM_INT_DATA {
        param1: item.param1 as f32,
        param2: item.param2 as f32,
        param3: item.param3 as f32,
        param4: item.param4 as f32,
        x: degrees_to_e7(item.x),
        y: degrees_to_e7(item.y),
        z: item.z as f32,
        seq: index as u16,
        command,
        target_system: vehicle.system_id,
        target_component: vehicle.component_id,
        frame,
        current: 0,
        autocontinue: u8::from(item.autocontinue),
        mission_type: kind.wire(),
    })
}

fn from_wire(item: &MISSION_ITEM_INT_DATA) -> MissionItem {
    MissionItem {
        seq: item.seq,
        frame: item.frame as u32 as u8,
        command: item.command as u32 as u16,
        param1: f64::from(item.param1),
        param2: f64::from(item.param2),
        param3: f64::from(item.param3),
        param4: f64::from(item.param4),
        x: f64::from(item.x) / 1e7,
        y: f64::from(item.y) / 1e7,
        z: f64::from(item.z),
        autocontinue: item.autocontinue != 0,
    }
}

fn result_text(result: MavMissionResult) -> String {
    use MavMissionResult::*;
    match result {
        MAV_MISSION_ERROR => "generic error".into(),
        MAV_MISSION_UNSUPPORTED_FRAME => "unsupported frame".into(),
        MAV_MISSION_UNSUPPORTED => "unsupported command".into(),
        MAV_MISSION_NO_SPACE => "no space for that many items".into(),
        MAV_MISSION_INVALID => "invalid item".into(),
        MAV_MISSION_INVALID_SEQUENCE => "invalid sequence".into(),
        MAV_MISSION_DENIED => "denied (the vehicle is busy or not accepting changes)".into(),
        MAV_MISSION_OPERATION_CANCELLED => "operation cancelled".into(),
        other => format!("{other:?}")
            .trim_start_matches("MAV_MISSION_")
            .replace('_', " ")
            .to_lowercase(),
    }
}

impl MissionService {
    pub fn new(link: LinkManager) -> Self {
        let (progress, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(Inner {
                link,
                progress,
                op_lock: AsyncMutex::new(()),
                timing: MissionTiming::default(),
            }),
        }
    }

    pub fn with_timing(self, timing: MissionTiming) -> Self {
        Self {
            inner: Arc::new(Inner {
                link: self.inner.link.clone(),
                progress: self.inner.progress.clone(),
                op_lock: AsyncMutex::new(()),
                timing,
            }),
        }
    }

    pub fn subscribe_progress(&self) -> broadcast::Receiver<MissionProgress> {
        self.inner.progress.subscribe()
    }

    fn emit(
        &self,
        kind: MissionKind,
        phase: MissionPhase,
        done: usize,
        total: usize,
        finished: bool,
        error: Option<String>,
    ) {
        let _ = self.inner.progress.send(MissionProgress {
            kind,
            phase,
            done: done as u32,
            total: total as u32,
            finished,
            error,
        });
    }

    fn finish<T>(
        &self,
        kind: MissionKind,
        phase: MissionPhase,
        total: usize,
        result: &Result<T, MissionError>,
    ) {
        match result {
            Ok(_) => self.emit(kind, phase, total, total, true, None),
            Err(error) => self.emit(kind, phase, 0, total, true, Some(error.to_string())),
        }
    }

    /// The next packet from `vehicle` before `deadline`, `None` on timeout.
    async fn next_packet(
        packets: &mut Packets,
        deadline: Instant,
        vehicle: VehicleId,
    ) -> Option<Arc<Packet>> {
        loop {
            match timeout_at(deadline, packets.recv()).await {
                Err(_) | Ok(Err(broadcast::error::RecvError::Closed)) => return None,
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Ok(packet)) => {
                    if packet.system_id() == vehicle.system_id
                        && packet.component_id() == vehicle.component_id
                    {
                        return Some(packet);
                    }
                }
            }
        }
    }

    fn send(&self, message: MavMessage) -> Result<(), MissionError> {
        self.inner.link.send(&message)?;
        Ok(())
    }

    /// Downloads the items of `kind`. For a mission, item 0 is home exactly as the vehicle
    /// reports it.
    pub async fn download(&self, kind: MissionKind) -> Result<Vec<MissionItem>, MissionError> {
        let _guard = self
            .inner
            .op_lock
            .try_lock()
            .map_err(|_| MissionError::Busy)?;
        self.emit(kind, MissionPhase::Download, 0, 0, false, None);
        let mut total = 0;
        let result = self.do_download(kind, &mut total).await;
        self.finish(kind, MissionPhase::Download, total, &result);
        result
    }

    async fn do_download(
        &self,
        kind: MissionKind,
        total: &mut usize,
    ) -> Result<Vec<MissionItem>, MissionError> {
        let timing = self.inner.timing;
        let (vehicle, _) = self.inner.link.selected_vehicle()?;
        let mut packets = self.inner.link.subscribe_packets();
        let wire = kind.wire();

        let mut count = None;
        'count: for _ in 0..timing.download_count_tries {
            self.send(MavMessage::MISSION_REQUEST_LIST(
                MISSION_REQUEST_LIST_DATA {
                    target_system: vehicle.system_id,
                    target_component: vehicle.component_id,
                    mission_type: wire,
                },
            ))?;
            let deadline = Instant::now() + timing.count_timeout;
            while let Some(packet) = Self::next_packet(&mut packets, deadline, vehicle).await {
                match &packet.message {
                    MavMessage::MISSION_COUNT(c) if c.mission_type == wire => {
                        count = Some(usize::from(c.count));
                        break 'count;
                    }
                    MavMessage::MISSION_ACK(a)
                        if a.mission_type == wire
                            && a.mavtype != MavMissionResult::MAV_MISSION_ACCEPTED =>
                    {
                        return Err(MissionError::Refused(result_text(a.mavtype)));
                    }
                    _ => {}
                }
            }
        }
        let count = count.ok_or(MissionError::NoCount)?;
        *total = count;
        self.emit(kind, MissionPhase::Download, 0, count, false, None);

        let mut items = Vec::with_capacity(count);
        for seq in 0..count as u16 {
            let mut received = None;
            'tries: for _ in 0..timing.item_tries {
                self.send(MavMessage::MISSION_REQUEST_INT(MISSION_REQUEST_INT_DATA {
                    seq,
                    target_system: vehicle.system_id,
                    target_component: vehicle.component_id,
                    mission_type: wire,
                }))?;
                let deadline = Instant::now() + timing.item_timeout;
                while let Some(packet) = Self::next_packet(&mut packets, deadline, vehicle).await {
                    if let MavMessage::MISSION_ITEM_INT(item) = &packet.message {
                        if item.mission_type == wire && item.seq == seq {
                            received = Some(from_wire(item));
                            break 'tries;
                        }
                    }
                }
            }
            items.push(received.ok_or(MissionError::ItemTimeout { seq })?);
            self.emit(
                kind,
                MissionPhase::Download,
                items.len(),
                count,
                false,
                None,
            );
        }

        self.send(MavMessage::MISSION_ACK(MISSION_ACK_DATA {
            target_system: vehicle.system_id,
            target_component: vehicle.component_id,
            mavtype: MavMissionResult::MAV_MISSION_ACCEPTED,
            mission_type: wire,
            ..MISSION_ACK_DATA::default()
        }))?;
        Ok(items)
    }

    /// Uploads `items` (for a mission, `items[0]` is home). Items are numbered by position.
    pub async fn upload(
        &self,
        kind: MissionKind,
        items: &[MissionItem],
    ) -> Result<(), MissionError> {
        let _guard = self
            .inner
            .op_lock
            .try_lock()
            .map_err(|_| MissionError::Busy)?;
        self.emit(kind, MissionPhase::Upload, 0, items.len(), false, None);
        let result = self.do_upload(kind, items).await;
        self.finish(kind, MissionPhase::Upload, items.len(), &result);
        result
    }

    async fn do_upload(
        &self,
        kind: MissionKind,
        items: &[MissionItem],
    ) -> Result<(), MissionError> {
        let timing = self.inner.timing;
        if items.len() > usize::from(u16::MAX) {
            return Err(MissionError::TooMany(items.len()));
        }
        let (vehicle, _) = self.inner.link.selected_vehicle()?;
        let wire = kind.wire();
        let wire_items = items
            .iter()
            .enumerate()
            .map(|(index, item)| to_wire(index, item, kind, vehicle))
            .collect::<Result<Vec<_>, _>>()?;
        let count = wire_items.len();
        let mut packets = self.inner.link.subscribe_packets();

        let send_count = || {
            self.send(MavMessage::MISSION_COUNT(MISSION_COUNT_DATA {
                count: count as u16,
                target_system: vehicle.system_id,
                target_component: vehicle.component_id,
                mission_type: wire,
                ..MISSION_COUNT_DATA::default()
            }))
        };

        let mut count_tries = 1;
        let mut item_tries = 0;
        let mut last_served: Option<u16> = None;
        send_count()?;
        let mut deadline = Instant::now() + timing.count_timeout;
        loop {
            let Some(packet) = Self::next_packet(&mut packets, deadline, vehicle).await else {
                // Silence: the request or our item was lost, so repeat the last thing we said.
                match last_served {
                    None if count_tries < timing.upload_count_tries => {
                        count_tries += 1;
                        send_count()?;
                        deadline = Instant::now() + timing.count_timeout;
                    }
                    None => return Err(MissionError::NoRequest),
                    Some(seq) if item_tries < timing.item_tries => {
                        item_tries += 1;
                        self.send(MavMessage::MISSION_ITEM_INT(
                            wire_items[usize::from(seq)].clone(),
                        ))?;
                        deadline = Instant::now() + timing.item_timeout;
                    }
                    Some(seq) => return Err(MissionError::UploadStalled { seq }),
                }
                continue;
            };
            let requested = match &packet.message {
                MavMessage::MISSION_REQUEST_INT(r) if r.mission_type == wire => Some(r.seq),
                #[allow(deprecated)]
                MavMessage::MISSION_REQUEST(r) if r.mission_type == wire => Some(r.seq),
                MavMessage::MISSION_ACK(a) if a.mission_type == wire => {
                    match a.mavtype {
                        MavMissionResult::MAV_MISSION_ACCEPTED => {
                            let all_sent = count == 0
                                || last_served.is_some_and(|seq| usize::from(seq) + 1 == count);
                            if all_sent {
                                return Ok(());
                            }
                        }
                        // The vehicle skipped ahead or repeated itself: keep answering requests.
                        MavMissionResult::MAV_MISSION_INVALID_SEQUENCE => {}
                        other => return Err(MissionError::Refused(result_text(other))),
                    }
                    None
                }
                _ => None,
            };
            let Some(seq) = requested else { continue };
            let Some(item) = wire_items.get(usize::from(seq)) else {
                return Err(MissionError::BadRequest {
                    requested: seq,
                    count,
                });
            };
            self.send(MavMessage::MISSION_ITEM_INT(item.clone()))?;
            last_served = Some(seq);
            item_tries = 0;
            self.emit(
                kind,
                MissionPhase::Upload,
                usize::from(seq) + 1,
                count,
                false,
                None,
            );
            deadline = Instant::now() + timing.item_timeout;
        }
    }

    /// Removes every item of `kind` from the vehicle.
    pub async fn clear(&self, kind: MissionKind) -> Result<(), MissionError> {
        let _guard = self
            .inner
            .op_lock
            .try_lock()
            .map_err(|_| MissionError::Busy)?;
        let timing = self.inner.timing;
        let (vehicle, _) = self.inner.link.selected_vehicle()?;
        let mut packets = self.inner.link.subscribe_packets();
        let wire = kind.wire();
        for _ in 0..timing.clear_tries {
            self.send(MavMessage::MISSION_CLEAR_ALL(MISSION_CLEAR_ALL_DATA {
                target_system: vehicle.system_id,
                target_component: vehicle.component_id,
                mission_type: wire,
            }))?;
            let deadline = Instant::now() + timing.clear_timeout;
            while let Some(packet) = Self::next_packet(&mut packets, deadline, vehicle).await {
                if let MavMessage::MISSION_ACK(a) = &packet.message {
                    if a.mission_type != wire {
                        continue;
                    }
                    return match a.mavtype {
                        MavMissionResult::MAV_MISSION_ACCEPTED => Ok(()),
                        other => Err(MissionError::Refused(result_text(other))),
                    };
                }
            }
        }
        Err(MissionError::NoAck)
    }
}
