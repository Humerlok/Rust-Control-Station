//! The replay vehicle's mission, fence and rally storage and its side of the MAVLink mission
//! microservice, so uploads and downloads can be exercised without a drone.
use control_station_mavlink::{
    dialect::{
        MavCmd, MavFrame, MavMissionResult, MavMissionType, MISSION_ACK_DATA, MISSION_COUNT_DATA,
        MISSION_ITEM_INT_DATA, MISSION_REQUEST_INT_DATA,
    },
    MavMessage, GCS_COMPONENT_ID, GCS_SYSTEM_ID,
};

struct Pending {
    kind: usize,
    expected: usize,
    items: Vec<MISSION_ITEM_INT_DATA>,
}

pub(super) struct ReplayMissions {
    /// Mission, fence, rally.
    stores: [Vec<MISSION_ITEM_INT_DATA>; 3],
    pending: Option<Pending>,
}

fn index(kind: MavMissionType) -> Option<usize> {
    match kind {
        MavMissionType::MAV_MISSION_TYPE_MISSION => Some(0),
        MavMissionType::MAV_MISSION_TYPE_FENCE => Some(1),
        MavMissionType::MAV_MISSION_TYPE_RALLY => Some(2),
        _ => None,
    }
}

fn ack(result: MavMissionResult, kind: MavMissionType) -> MavMessage {
    MavMessage::MISSION_ACK(MISSION_ACK_DATA {
        target_system: GCS_SYSTEM_ID,
        target_component: GCS_COMPONENT_ID,
        mavtype: result,
        mission_type: kind,
        ..MISSION_ACK_DATA::default()
    })
}

fn request(seq: usize, kind: MavMissionType) -> MavMessage {
    MavMessage::MISSION_REQUEST_INT(MISSION_REQUEST_INT_DATA {
        seq: seq as u16,
        target_system: GCS_SYSTEM_ID,
        target_component: GCS_COMPONENT_ID,
        mission_type: kind,
    })
}

impl ReplayMissions {
    /// A real ArduPilot always reports home as mission item 0, so the mission store starts with it.
    pub(super) fn new(home: Option<MISSION_ITEM_INT_DATA>) -> Self {
        Self {
            stores: [home.into_iter().collect(), Vec::new(), Vec::new()],
            pending: None,
        }
    }

    pub(super) fn home_item(lat: f64, lon: f64, alt_m: f64) -> MISSION_ITEM_INT_DATA {
        MISSION_ITEM_INT_DATA {
            x: (lat * 1e7).round() as i32,
            y: (lon * 1e7).round() as i32,
            z: alt_m as f32,
            command: MavCmd::MAV_CMD_NAV_WAYPOINT,
            frame: MavFrame::MAV_FRAME_GLOBAL,
            autocontinue: 1,
            ..MISSION_ITEM_INT_DATA::default()
        }
    }

    fn serve(&self, seq: u16, kind: MavMissionType) -> Vec<MavMessage> {
        let Some(k) = index(kind) else {
            return Vec::new();
        };
        match self.stores[k].get(usize::from(seq)) {
            Some(item) => vec![MavMessage::MISSION_ITEM_INT(MISSION_ITEM_INT_DATA {
                seq,
                target_system: GCS_SYSTEM_ID,
                target_component: GCS_COMPONENT_ID,
                current: 0,
                mission_type: kind,
                ..item.clone()
            })],
            None => vec![ack(MavMissionResult::MAV_MISSION_INVALID_SEQUENCE, kind)],
        }
    }

    /// The vehicle's answers to one message from the GCS (empty for unrelated messages).
    pub(super) fn handle(&mut self, message: &MavMessage) -> Vec<MavMessage> {
        match message {
            MavMessage::MISSION_REQUEST_LIST(r) => {
                let Some(k) = index(r.mission_type) else {
                    return Vec::new();
                };
                vec![MavMessage::MISSION_COUNT(MISSION_COUNT_DATA {
                    count: self.stores[k].len() as u16,
                    target_system: GCS_SYSTEM_ID,
                    target_component: GCS_COMPONENT_ID,
                    mission_type: r.mission_type,
                    ..MISSION_COUNT_DATA::default()
                })]
            }
            MavMessage::MISSION_REQUEST_INT(r) => self.serve(r.seq, r.mission_type),
            #[allow(deprecated)]
            MavMessage::MISSION_REQUEST(r) => self.serve(r.seq, r.mission_type),
            MavMessage::MISSION_COUNT(c) => {
                let Some(k) = index(c.mission_type) else {
                    return Vec::new();
                };
                if c.count == 0 {
                    self.pending = None;
                    self.stores[k].clear();
                    return vec![ack(MavMissionResult::MAV_MISSION_ACCEPTED, c.mission_type)];
                }
                self.pending = Some(Pending {
                    kind: k,
                    expected: usize::from(c.count),
                    items: Vec::new(),
                });
                vec![request(0, c.mission_type)]
            }
            MavMessage::MISSION_ITEM_INT(item) => {
                let Some(k) = index(item.mission_type) else {
                    return Vec::new();
                };
                let Some(pending) = self.pending.as_mut().filter(|p| p.kind == k) else {
                    return Vec::new();
                };
                let seq = usize::from(item.seq);
                if seq == pending.items.len() {
                    pending.items.push(item.clone());
                    if pending.items.len() == pending.expected {
                        if let Some(done) = self.pending.take() {
                            self.stores[k] = done.items;
                        }
                        return vec![ack(
                            MavMissionResult::MAV_MISSION_ACCEPTED,
                            item.mission_type,
                        )];
                    }
                    vec![request(pending.items.len(), item.mission_type)]
                } else if seq < pending.items.len() {
                    // A repeated item: ask again for the one we are still missing.
                    vec![request(pending.items.len(), item.mission_type)]
                } else {
                    vec![ack(
                        MavMissionResult::MAV_MISSION_INVALID_SEQUENCE,
                        item.mission_type,
                    )]
                }
            }
            MavMessage::MISSION_CLEAR_ALL(c) => {
                let Some(k) = index(c.mission_type) else {
                    return Vec::new();
                };
                // Home survives a clear, as on a real vehicle.
                self.stores[k].truncate(usize::from(k == 0));
                vec![ack(MavMissionResult::MAV_MISSION_ACCEPTED, c.mission_type)]
            }
            _ => Vec::new(),
        }
    }

    #[cfg(test)]
    fn len(&self, kind: MavMissionType) -> usize {
        index(kind).map_or(0, |k| self.stores[k].len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use control_station_mavlink::dialect::{MISSION_CLEAR_ALL_DATA, MISSION_REQUEST_LIST_DATA};

    const MISSION: MavMissionType = MavMissionType::MAV_MISSION_TYPE_MISSION;
    const FENCE: MavMissionType = MavMissionType::MAV_MISSION_TYPE_FENCE;

    fn item(seq: u16, kind: MavMissionType) -> MavMessage {
        MavMessage::MISSION_ITEM_INT(MISSION_ITEM_INT_DATA {
            seq,
            x: 100 + i32::from(seq),
            mission_type: kind,
            ..MISSION_ITEM_INT_DATA::default()
        })
    }

    fn count(n: u16, kind: MavMissionType) -> MavMessage {
        MavMessage::MISSION_COUNT(MISSION_COUNT_DATA {
            count: n,
            mission_type: kind,
            ..MISSION_COUNT_DATA::default()
        })
    }

    fn list(kind: MavMissionType) -> MavMessage {
        MavMessage::MISSION_REQUEST_LIST(MISSION_REQUEST_LIST_DATA {
            mission_type: kind,
            ..MISSION_REQUEST_LIST_DATA::default()
        })
    }

    #[test]
    fn mission_starts_with_home_and_fence_empty() {
        let mut m = ReplayMissions::new(Some(ReplayMissions::home_item(-20.5, -47.4, 900.0)));
        let reply = m.handle(&list(MISSION));
        assert!(matches!(&reply[..], [MavMessage::MISSION_COUNT(c)] if c.count == 1));
        let reply = m.handle(&list(FENCE));
        assert!(
            matches!(&reply[..], [MavMessage::MISSION_COUNT(c)] if c.count == 0 && c.mission_type == FENCE)
        );
        match &m.handle(&MavMessage::MISSION_REQUEST_INT(MISSION_REQUEST_INT_DATA {
            seq: 0,
            mission_type: MISSION,
            ..MISSION_REQUEST_INT_DATA::default()
        }))[..]
        {
            [MavMessage::MISSION_ITEM_INT(i)] => assert_eq!(i.x, -205_000_000),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn upload_requests_each_item_then_accepts() {
        let mut m = ReplayMissions::new(None);
        assert!(matches!(
            &m.handle(&count(3, MISSION))[..],
            [MavMessage::MISSION_REQUEST_INT(r)] if r.seq == 0
        ));
        assert!(matches!(
            &m.handle(&item(0, MISSION))[..],
            [MavMessage::MISSION_REQUEST_INT(r)] if r.seq == 1
        ));
        assert!(matches!(
            &m.handle(&item(1, MISSION))[..],
            [MavMessage::MISSION_REQUEST_INT(r)] if r.seq == 2
        ));
        let done = m.handle(&item(2, MISSION));
        assert!(matches!(
            &done[..],
            [MavMessage::MISSION_ACK(a)] if a.mavtype == MavMissionResult::MAV_MISSION_ACCEPTED
        ));
        assert_eq!(m.len(MISSION), 3);
        match &m.handle(&MavMessage::MISSION_REQUEST_INT(MISSION_REQUEST_INT_DATA {
            seq: 2,
            mission_type: MISSION,
            ..MISSION_REQUEST_INT_DATA::default()
        }))[..]
        {
            [MavMessage::MISSION_ITEM_INT(i)] => assert_eq!((i.seq, i.x), (2, 102)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn repeated_and_skipped_items_are_handled() {
        let mut m = ReplayMissions::new(None);
        m.handle(&count(3, MISSION));
        m.handle(&item(0, MISSION));
        assert!(matches!(
            &m.handle(&item(0, MISSION))[..],
            [MavMessage::MISSION_REQUEST_INT(r)] if r.seq == 1
        ));
        assert!(matches!(
            &m.handle(&item(2, MISSION))[..],
            [MavMessage::MISSION_ACK(a)] if a.mavtype == MavMissionResult::MAV_MISSION_INVALID_SEQUENCE
        ));
        assert_eq!(
            m.len(MISSION),
            0,
            "nothing is committed until the last item"
        );
    }

    #[test]
    fn kinds_do_not_mix_and_clear_keeps_home() {
        let mut m = ReplayMissions::new(Some(ReplayMissions::home_item(1.0, 2.0, 3.0)));
        m.handle(&count(2, FENCE));
        m.handle(&item(0, FENCE));
        m.handle(&item(1, FENCE));
        assert_eq!(m.len(FENCE), 2);
        assert_eq!(m.len(MISSION), 1);
        m.handle(&count(2, MISSION));
        m.handle(&item(0, MISSION));
        m.handle(&item(1, MISSION));
        assert_eq!(m.len(MISSION), 2);

        let reply = m.handle(&MavMessage::MISSION_CLEAR_ALL(MISSION_CLEAR_ALL_DATA {
            mission_type: MISSION,
            ..MISSION_CLEAR_ALL_DATA::default()
        }));
        assert!(matches!(&reply[..], [MavMessage::MISSION_ACK(_)]));
        assert_eq!(m.len(MISSION), 1, "home survives");
        assert_eq!(m.len(FENCE), 2, "other kinds untouched");
        m.handle(&MavMessage::MISSION_CLEAR_ALL(MISSION_CLEAR_ALL_DATA {
            mission_type: FENCE,
            ..MISSION_CLEAR_ALL_DATA::default()
        }));
        assert_eq!(m.len(FENCE), 0);
    }

    #[test]
    fn an_empty_upload_clears_and_is_accepted() {
        let mut m = ReplayMissions::new(Some(ReplayMissions::home_item(1.0, 2.0, 3.0)));
        let reply = m.handle(&count(0, MISSION));
        assert!(matches!(
            &reply[..],
            [MavMessage::MISSION_ACK(a)] if a.mavtype == MavMissionResult::MAV_MISSION_ACCEPTED
        ));
        assert_eq!(m.len(MISSION), 0);
    }

    #[test]
    fn out_of_range_requests_get_invalid_sequence_and_noise_is_ignored() {
        let mut m = ReplayMissions::new(None);
        let reply = m.handle(&MavMessage::MISSION_REQUEST_INT(MISSION_REQUEST_INT_DATA {
            seq: 5,
            ..MISSION_REQUEST_INT_DATA::default()
        }));
        assert!(matches!(
            &reply[..],
            [MavMessage::MISSION_ACK(a)] if a.mavtype == MavMissionResult::MAV_MISSION_INVALID_SEQUENCE
        ));
        assert!(
            m.handle(&item(0, MISSION)).is_empty(),
            "no upload in progress"
        );
        assert!(m
            .handle(&MavMessage::HEARTBEAT(Default::default()))
            .is_empty());
    }
}
