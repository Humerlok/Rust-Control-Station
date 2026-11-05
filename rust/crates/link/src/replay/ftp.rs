//! The replay vehicle's MAVFTP server over an in-memory file system, so the file screen and
//! uploads can be exercised without a drone. Contents live only as long as the replay.
use std::collections::BTreeMap;

use control_station_mavftp::{crc32, Frame, Opcode, MAX_DATA};
use control_station_mavlink::{
    dialect::FILE_TRANSFER_PROTOCOL_DATA, MavMessage, GCS_COMPONENT_ID, GCS_SYSTEM_ID,
};

/// Seconds since the epoch reported as every entry's modification time.
const MTIME: u32 = 1_700_000_000;
/// Frames the vehicle streams per burst before asking for a new request.
const BURST_FRAMES: usize = 8;

pub(super) struct ReplayFtp {
    files: BTreeMap<String, Vec<u8>>,
    dirs: Vec<String>,
    /// Open sessions: id -> path.
    sessions: BTreeMap<u8, String>,
    next_session: u8,
}

fn wrap(frame: Frame) -> MavMessage {
    MavMessage::FILE_TRANSFER_PROTOCOL(FILE_TRANSFER_PROTOCOL_DATA {
        target_network: 0,
        target_system: GCS_SYSTEM_ID,
        target_component: GCS_COMPONENT_ID,
        payload: frame.encode().expect("replay frames fit"),
    })
}

fn ack(request: &Frame) -> Frame {
    let mut frame = Frame::request(request.seq.wrapping_add(1), Opcode::Ack);
    frame.req_opcode = request.opcode;
    frame.session = request.session;
    frame
}

fn nak(request: &Frame, data: &[u8]) -> Vec<MavMessage> {
    let mut frame = Frame::request(request.seq.wrapping_add(1), Opcode::Nak).with_data(data);
    frame.req_opcode = request.opcode;
    frame.session = request.session;
    vec![wrap(frame)]
}

const ERR_FAIL_ERRNO: u8 = 2;
const ERR_INVALID_SESSION: u8 = 4;
const ERR_EOF: u8 = 6;
const ERR_EXISTS: u8 = 8;
const ERR_NOT_FOUND: u8 = 10;
const ENOTEMPTY: u8 = 39;

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_string()
}

fn parent(path: &str) -> String {
    match path.rsplit_once('/') {
        Some(("", _)) | None => "/".into(),
        Some((dir, _)) => dir.into(),
    }
}

impl ReplayFtp {
    pub(super) fn new() -> Self {
        let mut files = BTreeMap::new();
        files.insert(
            "/APM/scripts/example.lua".to_string(),
            b"-- Replay vehicle example script\ngcs:send_text(6, \"hello from Lua\")\n".to_vec(),
        );
        files.insert(
            "/APM/README.txt".to_string(),
            b"Files on the replay vehicle live in memory only.\n".to_vec(),
        );
        Self {
            files,
            dirs: ["/APM", "/APM/scripts", "/APM/logs"]
                .map(String::from)
                .to_vec(),
            sessions: BTreeMap::new(),
            next_session: 1,
        }
    }

    fn dir_exists(&self, path: &str) -> bool {
        path == "/" || self.dirs.iter().any(|d| d == path)
    }

    fn records(&self, dir: &str, timed: bool) -> Vec<Vec<u8>> {
        let prefix = if dir == "/" {
            "/".to_string()
        } else {
            format!("{dir}/")
        };
        let direct = |path: &str| path.starts_with(&prefix) && !path[prefix.len()..].contains('/');
        let mut records = Vec::new();
        for d in self.dirs.iter().filter(|d| direct(d) && d.as_str() != dir) {
            let name = &d[prefix.len()..];
            records.push(if timed {
                format!("D{name}\t0\t{MTIME}\0")
            } else {
                format!("D{name}\0")
            });
        }
        for (path, bytes) in self.files.iter().filter(|(p, _)| direct(p)) {
            let name = &path[prefix.len()..];
            records.push(if timed {
                format!("F{name}\t{}\t{MTIME}\0", bytes.len())
            } else {
                format!("F{name}\t{}\0", bytes.len())
            });
        }
        records.into_iter().map(String::into_bytes).collect()
    }

    fn open_session(&mut self, path: String) -> u8 {
        let id = self.next_session;
        self.next_session = self.next_session.wrapping_add(1).max(1);
        self.sessions.insert(id, path);
        id
    }

    pub(super) fn handle(&mut self, message: &MavMessage) -> Vec<MavMessage> {
        let MavMessage::FILE_TRANSFER_PROTOCOL(data) = message else {
            return Vec::new();
        };
        let Ok(request) = Frame::decode(&data.payload) else {
            return Vec::new();
        };
        let path = text(&request.data);
        match request.opcode {
            Opcode::None => vec![wrap(ack(&request))],
            Opcode::ListDirectory | Opcode::ListDirectoryWithTime => {
                if !self.dir_exists(&path) {
                    return nak(&request, &[ERR_NOT_FOUND]);
                }
                let records = self.records(&path, request.opcode == Opcode::ListDirectoryWithTime);
                let skip = request.offset as usize;
                if skip >= records.len() {
                    return nak(&request, &[ERR_EOF]);
                }
                let mut body = Vec::new();
                for record in &records[skip..] {
                    if body.len() + record.len() > MAX_DATA {
                        break;
                    }
                    body.extend_from_slice(record);
                }
                vec![wrap(ack(&request).with_data(body))]
            }
            Opcode::OpenFileRO => match self.files.get(&path) {
                None => nak(&request, &[ERR_NOT_FOUND]),
                Some(bytes) => {
                    let size = (bytes.len() as u32).to_le_bytes();
                    let mut frame = ack(&request).with_data(size);
                    frame.session = self.open_session(path);
                    vec![wrap(frame)]
                }
            },
            Opcode::ReadFile | Opcode::BurstReadFile => {
                let burst = request.opcode == Opcode::BurstReadFile;
                let Some(file) = self
                    .sessions
                    .get(&request.session)
                    .and_then(|p| self.files.get(p))
                else {
                    return nak(&request, &[ERR_INVALID_SESSION]);
                };
                let step = usize::from(request.size).clamp(1, MAX_DATA);
                let mut offset = request.offset as usize;
                if offset >= file.len() {
                    return nak(&request, &[ERR_EOF]);
                }
                let mut out = Vec::new();
                while offset < file.len() && out.len() < if burst { BURST_FRAMES } else { 1 } {
                    let end = (offset + step).min(file.len());
                    let mut frame = ack(&request).with_data(file[offset..end].to_vec());
                    frame.offset = offset as u32;
                    frame.burst_complete =
                        burst && (end == file.len() || out.len() + 1 == BURST_FRAMES);
                    out.push(wrap(frame));
                    offset = end;
                }
                out
            }
            Opcode::CreateFile | Opcode::OpenFileWO => {
                if !self.dir_exists(&parent(&path)) {
                    return nak(&request, &[ERR_NOT_FOUND]);
                }
                self.files.entry(path.clone()).or_default();
                if request.opcode == Opcode::CreateFile {
                    self.files.insert(path.clone(), Vec::new());
                }
                let mut frame = ack(&request);
                frame.session = self.open_session(path);
                vec![wrap(frame)]
            }
            Opcode::WriteFile => {
                let Some(file) = self
                    .sessions
                    .get(&request.session)
                    .and_then(|p| self.files.get_mut(p))
                else {
                    return nak(&request, &[ERR_INVALID_SESSION]);
                };
                let at = request.offset as usize;
                if file.len() < at + request.data.len() {
                    file.resize(at + request.data.len(), 0);
                }
                file[at..at + request.data.len()].copy_from_slice(&request.data);
                vec![wrap(ack(&request))]
            }
            Opcode::TerminateSession => {
                self.sessions.remove(&request.session);
                vec![wrap(ack(&request))]
            }
            Opcode::ResetSessions => {
                self.sessions.clear();
                vec![wrap(ack(&request))]
            }
            Opcode::CalcFileCRC32 => match self.files.get(&path) {
                None => nak(&request, &[ERR_NOT_FOUND]),
                Some(bytes) => vec![wrap(ack(&request).with_data(crc32(0, bytes).to_le_bytes()))],
            },
            Opcode::RemoveFile => {
                if self.files.remove(&path).is_some() {
                    vec![wrap(ack(&request))]
                } else {
                    nak(&request, &[ERR_NOT_FOUND])
                }
            }
            Opcode::CreateDirectory => {
                if self.dir_exists(&path) {
                    return nak(&request, &[ERR_EXISTS]);
                }
                if !self.dir_exists(&parent(&path)) {
                    return nak(&request, &[ERR_NOT_FOUND]);
                }
                self.dirs.push(path);
                vec![wrap(ack(&request))]
            }
            Opcode::RemoveDirectory => {
                if !self.dirs.contains(&path) {
                    return nak(&request, &[ERR_NOT_FOUND]);
                }
                let prefix = format!("{path}/");
                let busy = self.files.keys().any(|f| f.starts_with(&prefix))
                    || self.dirs.iter().any(|d| d.starts_with(&prefix));
                if busy {
                    return nak(&request, &[ERR_FAIL_ERRNO, ENOTEMPTY]);
                }
                self.dirs.retain(|d| *d != path);
                vec![wrap(ack(&request))]
            }
            Opcode::Rename => {
                let mut parts = request.data.split(|&b| b == 0);
                let from = text(parts.next().unwrap_or_default());
                let to = text(parts.next().unwrap_or_default());
                match self.files.remove(&from) {
                    Some(bytes) if self.dir_exists(&parent(&to)) => {
                        self.files.insert(to, bytes);
                        vec![wrap(ack(&request))]
                    }
                    Some(bytes) => {
                        self.files.insert(from, bytes);
                        nak(&request, &[ERR_NOT_FOUND])
                    }
                    None => nak(&request, &[ERR_NOT_FOUND]),
                }
            }
            Opcode::TruncateFile => match self.files.get_mut(&path) {
                Some(bytes) => {
                    bytes.truncate(request.offset as usize);
                    vec![wrap(ack(&request))]
                }
                None => nak(&request, &[ERR_NOT_FOUND]),
            },
            Opcode::Ack | Opcode::Nak => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(ftp: &mut ReplayFtp, frame: Frame) -> Vec<Frame> {
        let message = MavMessage::FILE_TRANSFER_PROTOCOL(FILE_TRANSFER_PROTOCOL_DATA {
            target_network: 0,
            target_system: 1,
            target_component: 1,
            payload: frame.encode().unwrap(),
        });
        ftp.handle(&message)
            .iter()
            .map(|m| match m {
                MavMessage::FILE_TRANSFER_PROTOCOL(d) => Frame::decode(&d.payload).unwrap(),
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    }

    fn call(ftp: &mut ReplayFtp, opcode: Opcode, data: &[u8]) -> Frame {
        let mut replies = ask(ftp, Frame::request(10, opcode).with_data(data));
        assert_eq!(replies.len(), 1);
        replies.remove(0)
    }

    #[test]
    fn lists_with_and_without_time() {
        let mut ftp = ReplayFtp::new();
        let timed = call(&mut ftp, Opcode::ListDirectoryWithTime, b"/APM");
        assert_eq!(timed.opcode, Opcode::Ack);
        assert_eq!(timed.seq, 11);
        let (entries, _) = control_station_mavftp::parse_listing(&timed.data, true);
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["scripts", "logs", "README.txt"]);
        assert_eq!(entries[2].modified, Some(MTIME));
        let plain = call(&mut ftp, Opcode::ListDirectory, b"/APM");
        assert_eq!(
            control_station_mavftp::parse_listing(&plain.data, false)
                .0
                .len(),
            3
        );
        // past the end, and a missing directory
        let eof = ask(
            &mut ftp,
            Frame::request(1, Opcode::ListDirectory)
                .with_data(b"/APM".to_vec())
                .with_offset(3),
        );
        assert_eq!(eof[0].nak(), Some(control_station_mavftp::FtpError::Eof));
        assert_eq!(
            call(&mut ftp, Opcode::ListDirectory, b"/nope").nak(),
            Some(control_station_mavftp::FtpError::FileNotFound)
        );
    }

    #[test]
    fn upload_then_burst_download_round_trips() {
        let mut ftp = ReplayFtp::new();
        let data: Vec<u8> = (0..1000u32).map(|i| (i % 253) as u8).collect();
        let created = call(&mut ftp, Opcode::CreateFile, b"/APM/scripts/a.lua");
        assert_eq!(created.opcode, Opcode::Ack);
        let session = created.session;
        for (n, chunk) in data.chunks(80).enumerate() {
            let reply = ask(
                &mut ftp,
                Frame::request(1, Opcode::WriteFile)
                    .with_session(session)
                    .with_offset((n * 80) as u32)
                    .with_data(chunk.to_vec()),
            );
            assert_eq!(reply[0].opcode, Opcode::Ack);
        }
        let crc = call(&mut ftp, Opcode::CalcFileCRC32, b"/APM/scripts/a.lua");
        assert_eq!(crc.data, crc32(0, &data).to_le_bytes());

        let open = call(&mut ftp, Opcode::OpenFileRO, b"/APM/scripts/a.lua");
        assert_eq!(open.data, 1000u32.to_le_bytes());
        let mut got = vec![0u8; 1000];
        let mut offset = 0;
        while offset < 1000 {
            let frames = ask(
                &mut ftp,
                Frame::request(1, Opcode::BurstReadFile)
                    .with_session(open.session)
                    .with_offset(offset as u32)
                    .with_size(100),
            );
            assert!(frames.len() <= BURST_FRAMES);
            assert!(frames.last().unwrap().burst_complete);
            for f in &frames {
                let at = f.offset as usize;
                got[at..at + f.data.len()].copy_from_slice(&f.data);
                offset = at + f.data.len();
            }
        }
        assert_eq!(got, data);
    }

    #[test]
    fn file_operations_and_errors() {
        let mut ftp = ReplayFtp::new();
        assert_eq!(
            call(&mut ftp, Opcode::CreateFile, b"/missing/x").nak(),
            Some(control_station_mavftp::FtpError::FileNotFound)
        );
        assert_eq!(
            call(&mut ftp, Opcode::CreateDirectory, b"/APM/scripts").nak(),
            Some(control_station_mavftp::FtpError::FileExists)
        );
        assert_eq!(
            call(&mut ftp, Opcode::RemoveDirectory, b"/APM/scripts").nak(),
            Some(control_station_mavftp::FtpError::FailErrno(ENOTEMPTY))
        );
        assert_eq!(
            call(&mut ftp, Opcode::RemoveFile, b"/APM/scripts/example.lua").opcode,
            Opcode::Ack
        );
        assert_eq!(
            call(&mut ftp, Opcode::RemoveDirectory, b"/APM/scripts").opcode,
            Opcode::Ack
        );
        let renamed = call(
            &mut ftp,
            Opcode::Rename,
            b"/APM/README.txt\0/APM/logs/r.txt\0",
        );
        assert_eq!(renamed.opcode, Opcode::Ack);
        assert!(ftp.files.contains_key("/APM/logs/r.txt"));
    }
}
