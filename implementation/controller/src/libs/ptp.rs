use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::mem;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::raw::c_void;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use log::{debug, info, warn};
use rbfrt::table::MatchValue;
use rbfrt::table::Request;
use rbfrt::thrift_client::ThriftInputProtocol;
use rbfrt::thrift_client::ThriftOutputProtocol;
use rbfrt::thrift_generated::ts::{TTsSyncClient, TsSyncClient};
use rbfrt::SwitchConnection;
use serde::Serialize;
use tokio::io::unix::AsyncFd;
use tokio::sync::watch;
use tokio::time;

use super::packet_generator::PacketGenerator;

#[derive(Clone, Debug)]
pub enum PtpSyncStatus {
    Initializing,
    Syncing { offset_ns: i64, ptp_time_ns: u64 },
    Synchronized { offset_ns: i64, ptp_time_ns: u64 },
}

/// Tracks recent offset values for computing a moving average
struct OffsetHistory {
    values: VecDeque<i64>,
    capacity: usize,
}

impl OffsetHistory {
    fn new(capacity: usize) -> Self {
        Self {
            values: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn push(&mut self, offset: i64) {
        if self.values.len() >= self.capacity {
            self.values.pop_front();
        }
        self.values.push_back(offset);
    }

    fn average(&self) -> Option<i64> {
        if self.values.is_empty() {
            return None;
        }
        let sum: i64 = self.values.iter().sum();
        Some(sum / self.values.len() as i64)
    }
}

#[repr(u8)]
pub enum PtpMessageTypes {
    Sync = 0,
    FollowUp = 8,
    DelayResp = 9,
    DelayReq = 1,
}

const CONTROL_PLANE_DEV_PORT: u32 = 4; // FP port 33
const PTP_TO_MASTER_DEV_PORT: u32 = 320; // FP port 9
const PTP_TO_SLAVE_DEV_PORT: u32 = 8; // FP port 31

const CLOCK_IDENTITY_MASTER: u64 = 0x98039bfffe84aa8e;
const CLOCK_IDENTITY_SELF: u64 = 0x0290fbfffe770bb9;
const CLOCK_IDENTITY_SLAVE: u64 = 0x0290fbfffe79209f;

/// Ethertype for IEEE 1588 over L2 Ethernet
const ETH_P_1588: u16 = 0x88F7;
const ETH_HEADER_LEN: usize = 14;
const APPENDED_TS_LEN: usize = 8;

/// PTPv2 message types (lower 4 bits of first header byte)
const PTP_SYNC: u8 = 0x0;
const PTP_FOLLOW_UP: u8 = 0x8;
const PTP_DELAY_REQ: u8 = 0x1;
const PTP_DELAY_RESP: u8 = 0x9;

// Polling intervals for PTP synchronization process
const PTP_ACTIVATION_SPIN_WINDOW_NS: u64 = 50_000;
const PTP_ACTIVATION_CHECK_INTERVAL_NS: u64 = 10;

pub fn populate_ptp_table() -> io::Result<Vec<Request>> {
    let mut reqs = vec![];

    // Sync (msg_type 0): capture ingress TS for T2 extraction by control plane
    let req = Request::new("ingress.ptp_c.ptp")
        .match_key(
            "hdr.ptp_1.msg_type",
            MatchValue::exact(PtpMessageTypes::Sync as u8),
        )
        .match_key(
            "hdr.ptp_2.clock_identity",
            MatchValue::exact(CLOCK_IDENTITY_MASTER),
        )
        .action("ingress.ptp_c.add_ingress_ts");
    reqs.push(req);

    // Follow_Up (msg_type 8): capture ingress TS for residence time correction in egress
    let req = Request::new("ingress.ptp_c.ptp")
        .match_key(
            "hdr.ptp_1.msg_type",
            MatchValue::exact(PtpMessageTypes::FollowUp as u8),
        )
        .match_key(
            "hdr.ptp_2.clock_identity",
            MatchValue::exact(CLOCK_IDENTITY_MASTER),
        )
        .action("ingress.ptp_c.write_ingress_ts_to_correction_field");
    reqs.push(req);
    // Delay_Req (msg_type 1): capture ingress TS for residence time correction in egress
    let req = Request::new("ingress.ptp_c.ptp")
        .match_key(
            "hdr.ptp_1.msg_type",
            MatchValue::exact(PtpMessageTypes::DelayReq as u8),
        )
        .match_key(
            "hdr.ptp_2.clock_identity",
            MatchValue::exact(CLOCK_IDENTITY_SLAVE),
        )
        .action("ingress.ptp_c.write_ingress_ts_to_correction_field");
    reqs.push(req);
    // Delay_Resp (msg_type 9): capture ingress TS for residence time correction in egress
    let req = Request::new("ingress.ptp_c.ptp")
        .match_key(
            "hdr.ptp_1.msg_type",
            MatchValue::exact(PtpMessageTypes::DelayResp as u8),
        )
        .match_key(
            "hdr.ptp_2.clock_identity",
            MatchValue::exact(CLOCK_IDENTITY_MASTER),
        )
        .action("ingress.ptp_c.write_ingress_ts_to_correction_field");
    reqs.push(req);

    // Enable ts6 (correctionField) for FollowUp->Slave_Port
    let req = Request::new("egress.ptp")
        .match_key(
            "hdr.ptp_1.msg_type",
            MatchValue::exact(PtpMessageTypes::FollowUp as u8),
        )
        .match_key(
            "hdr.ptp_2.clock_identity",
            MatchValue::exact(CLOCK_IDENTITY_MASTER),
        )
        .match_key(
            "eg_intr_md.egress_port",
            MatchValue::exact(PTP_TO_SLAVE_DEV_PORT),
        )
        .action("egress.enable_ts6")
        .action_data("cf_byte_offset", 22u8);
    reqs.push(req);
    // Enable ts6 (correctionField) for DelayReq->Master_Port
    let req = Request::new("egress.ptp")
        .match_key(
            "hdr.ptp_1.msg_type",
            MatchValue::exact(PtpMessageTypes::DelayReq as u8),
        )
        .match_key(
            "hdr.ptp_2.clock_identity",
            MatchValue::exact(CLOCK_IDENTITY_SLAVE),
        )
        .match_key(
            "eg_intr_md.egress_port",
            MatchValue::exact(PTP_TO_MASTER_DEV_PORT),
        )
        .action("egress.enable_ts6")
        .action_data("cf_byte_offset", 22u8);
    reqs.push(req);
    // Enable ts6 (correctionField) for DelayResp->Slave_Port
    let req = Request::new("egress.ptp")
        .match_key(
            "hdr.ptp_1.msg_type",
            MatchValue::exact(PtpMessageTypes::DelayResp as u8),
        )
        .match_key(
            "hdr.ptp_2.clock_identity",
            MatchValue::exact(CLOCK_IDENTITY_MASTER),
        )
        .match_key(
            "eg_intr_md.egress_port",
            MatchValue::exact(PTP_TO_SLAVE_DEV_PORT),
        )
        .action("egress.enable_ts6")
        .action_data("cf_byte_offset", 22u8);
    reqs.push(req);

    // Only capture TX timestamp for our OWN Delay_Req, not transit Delay_Reqs
    // from other switches sharing the same egress port toward the GM.
    /*
    let iface_mac = read_interface_mac(ifname)?;
    //let clock_identity = mac_to_clock_identity(&iface_mac);
    info!(
        "Egress PTP table: matching clock_identity={:02x?} (from {})",
        clock_identity, ifname
    );
     */
    let req = Request::new("egress.ptp")
        .match_key(
            "hdr.ptp_1.msg_type",
            MatchValue::exact(PtpMessageTypes::DelayReq as u8),
        )
        .match_key(
            "hdr.ptp_2.clock_identity",
            MatchValue::exact(CLOCK_IDENTITY_SELF),
        )
        .match_key(
            "eg_intr_md.egress_port",
            MatchValue::exact(PTP_TO_MASTER_DEV_PORT),
        )
        .action("egress.enable_tstamp_capture");
    reqs.push(req);

    Ok(reqs)
}

#[derive(Debug)]
pub struct DelayReqTimestamp {
    pub valid: bool,
    pub raw_hw_ns: u64,
    pub id: u32,
}
pub fn retrieve_delay_req_egress_ts(
    thrift_client: &mut TsSyncClient<ThriftInputProtocol, ThriftOutputProtocol>,
    dev_port: u32,
) -> io::Result<DelayReqTimestamp> {
    let ts = thrift_client
        .ts_1588_timestamp_tx_get(0, dev_port as i32)
        .map_err(|e| io::Error::other(format!("Thrift error: {}", e)))?;

    let valid = ts.ts_valid.unwrap_or(false);
    let raw_hw_ns = ts.ts.unwrap_or(0) as u64;
    let id = ts.ts_id.unwrap_or(0) as u32;

    debug!(
        "ts_1588_timestamp_tx_get dev_port={} valid={} raw_hw_ns=0x{:012x} id={}",
        dev_port, valid, raw_hw_ns, id
    );

    Ok(DelayReqTimestamp {
        valid,
        raw_hw_ns,
        id,
    })
}

#[derive(Debug, Clone)]
struct PtpHeader {
    msg_type: u8,
    version: u8,
    message_length: u16,
    domain: u8,
    flags: u16,
    correction_field: i64,
    source_port_identity: [u8; 10], // clockIdentity(8) + portNumber(2)
    sequence_id: u16,
    log_message_interval: i8,
    origin_timestamp_seconds: u64,
    origin_timestamp_nanoseconds: u32,
    requesting_port_identity: Option<[u8; 10]>, // Delay_Resp only: bytes 44-53
}

impl PtpHeader {
    fn parse(buf: &[u8]) -> Option<Self> {
        // Need at least 44 bytes to access preciseOriginTimestamp
        if buf.len() < 44 {
            return None;
        }

        let b0 = buf[0];
        let msg_type = b0 & 0x0F;
        let version = buf[1] & 0x0F;

        let message_length = u16::from_be_bytes([buf[2], buf[3]]);
        let domain = buf[4];
        let flags = u16::from_be_bytes([buf[6], buf[7]]);

        let correction_field = i64::from_be_bytes([
            buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
        ]);

        let mut source_port_identity = [0u8; 10];
        source_port_identity.copy_from_slice(&buf[20..30]);

        let sequence_id = u16::from_be_bytes([buf[30], buf[31]]);
        let log_message_interval = buf[33] as i8;
        let origin_timestamp_seconds = decode_u48(&buf[34..40]);
        let origin_timestamp_nanoseconds = u32::from_be_bytes([buf[40], buf[41], buf[42], buf[43]]);

        let requesting_port_identity = if msg_type == PTP_DELAY_RESP && buf.len() >= 54 {
            let mut rpi = [0u8; 10];
            rpi.copy_from_slice(&buf[44..54]);
            Some(rpi)
        } else {
            None
        };

        Some(Self {
            msg_type,
            version,
            message_length,
            domain,
            flags,
            correction_field,
            source_port_identity,
            sequence_id,
            log_message_interval,
            origin_timestamp_seconds,
            origin_timestamp_nanoseconds,
            requesting_port_identity,
        })
    }

    /// IEEE 1588-2008 flagField bit 9 = twoStepFlag (0x0200)
    fn two_step(&self) -> bool {
        (self.flags & 0x0200) != 0
    }
}

#[derive(Debug)]
enum SyncState {
    Idle,
    WaitingFollowUp {
        seq: u16,
        src: [u8; 10],
        deadline: Instant,
        t2: Option<PtpTime>,
    },
}

/// Tracks upper 16 bits of the Tofino timestamp that are not sent over the wire.
struct TimestampTracker {
    sw_ts_ns: u16,
    last_hw_ts: Option<u64>,
}

impl TimestampTracker {
    fn new() -> Self {
        Self {
            sw_ts_ns: Self::initial_sw_ts_ns(),
            last_hw_ts: None,
        }
    }

    fn initial_sw_ts_ns() -> u16 {
        let unix_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs().saturating_mul(1_000_000_000) + d.subsec_nanos() as u64)
            .unwrap_or(0);
        ((unix_ns >> 48) & 0xFFFF) as u16
    }

    /// Combine `hw_ts` lower 48 bits with the maintained upper 16 bits.
    fn combine(&mut self, hw_ts: u64) -> u64 {
        const MASK_48: u64 = (1u64 << 48) - 1;
        let hw_ts = hw_ts & MASK_48;
        if let Some(prev) = self.last_hw_ts {
            if hw_ts < prev {
                self.sw_ts_ns = self.sw_ts_ns.wrapping_add(1);
                debug!(
                    "Detected hardware timestamp wrap; sw_ts_ns now 0x{:04x}",
                    self.sw_ts_ns
                );
            }
        }
        self.last_hw_ts = Some(hw_ts);
        ((self.sw_ts_ns as u64) << 48) | hw_ts
    }

    fn last_components(&self) -> Option<(u16, u64)> {
        self.last_hw_ts.map(|hw| (self.sw_ts_ns, hw))
    }

    fn last_full_timestamp(&self) -> Option<u64> {
        self.last_components()
            .map(|(sw, hw)| ((sw as u64) << 48) | hw)
    }
}

#[derive(Debug, Default, Clone)]
struct SyncTimes {
    t1: Option<PtpTime>,
    t2: Option<PtpTime>,
    last_frame: Option<Vec<u8>>,
    last_header: Option<PtpHeader>,
    t3: Option<PtpTime>,
    t4: Option<PtpTime>,
    cf_followup_ns: i64,
    cf_delayresp_ns: i64,
    /// Sequence ID of the Delay_Req we sent, used to verify Delay_Resp matches
    delay_req_seq: Option<u16>,
    /// Last seen egress timestamp ID, used to detect stale reads
    last_egress_ts_id: Option<u32>,
}

impl SyncTimes {
    fn take_complete_sample(&mut self) -> Option<PtpServoSample> {
        let (t1, t2, t3, t4) = match (self.t1, self.t2, self.t3, self.t4) {
            (Some(t1), Some(t2), Some(t3), Some(t4)) => (t1, t2, t3, t4),
            _ => return None,
        };
        let sample = PtpServoSample {
            t1,
            t2,
            t3,
            t4,
            cf_followup_ns: self.cf_followup_ns,
            cf_delayresp_ns: self.cf_delayresp_ns,
        };
        self.t1 = None;
        self.t2 = None;
        self.t3 = None;
        self.t4 = None;
        self.cf_followup_ns = 0;
        self.cf_delayresp_ns = 0;
        self.delay_req_seq = None;
        Some(sample)
    }
}

#[derive(Debug, Clone, Copy)]
struct PtpTime {
    raw_ns: u64,
    seconds: u64,
    nanoseconds: u32,
}

impl PtpTime {
    fn from_ns(ns: u64) -> Self {
        Self {
            raw_ns: ns,
            seconds: ns / 1_000_000_000,
            nanoseconds: (ns % 1_000_000_000) as u32,
        }
    }

    fn from_components(seconds: u64, nanoseconds: u32) -> Self {
        let raw_ns = seconds
            .saturating_mul(1_000_000_000)
            .saturating_add(nanoseconds as u64);
        Self {
            raw_ns,
            seconds,
            nanoseconds,
        }
    }
}

impl fmt::Display for PtpTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:09}s", self.seconds, self.nanoseconds)
    }
}

#[derive(Debug, Clone, Copy)]
struct PtpServoSample {
    t1: PtpTime,
    t2: PtpTime,
    t3: PtpTime,
    t4: PtpTime,
    cf_followup_ns: i64,
    cf_delayresp_ns: i64,
}

#[derive(Serialize)]
struct PtpOffsetEntry {
    timestamp_ms: u64,
    offset_ns: i64,
}

struct PtpOffsetLog {
    entries: Vec<PtpOffsetEntry>,
    last_measurement: Option<Instant>,
}

impl PtpOffsetLog {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            last_measurement: None,
        }
    }

    fn record(&mut self, offset_ns: i64) {
        let now = Instant::now();
        let timestamp_ms = match self.last_measurement {
            Some(last) => now.duration_since(last).as_millis() as u64,
            None => 0,
        };
        self.last_measurement = Some(now);
        self.entries.push(PtpOffsetEntry {
            timestamp_ms,
            offset_ns,
        });
    }

    fn write_to_file(&self, path: &str) {
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(path, serde_json::to_string(&self.entries).unwrap()) {
            Ok(_) => {}
            Err(e) => warn!("Failed to write PTP offset log: {}", e),
        }
    }
}

/// Minimal Ethernet header parse (14 bytes)
fn parse_ethertype(frame: &[u8]) -> Option<u16> {
    if frame.len() < 14 {
        return None;
    }
    Some(u16::from_be_bytes([frame[12], frame[13]]))
}

fn decode_u48(bytes: &[u8]) -> u64 {
    let mut value = 0u64;
    for &b in bytes.iter().take(6) {
        value = (value << 8) | b as u64;
    }
    value
}

fn read_interface_mac(ifname: &str) -> io::Result<[u8; 6]> {
    let path = format!("/sys/class/net/{ifname}/address");
    let contents = std::fs::read_to_string(&path)?;
    let trimmed = contents.trim();
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() != 6 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected MAC format in {}", path),
        ));
    }
    let mut mac = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        mac[i] = u8::from_str_radix(part, 16).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid MAC component {part}: {e}"),
            )
        })?;
    }
    Ok(mac)
}

fn mac_to_clock_identity(mac: &[u8; 6]) -> [u8; 8] {
    let mut identity = [0u8; 8];
    identity[0] = mac[0] ^ 0x02; // flip U/L bit to indicate local admin
    identity[1] = mac[1];
    identity[2] = mac[2];
    identity[3] = 0xFF;
    identity[4] = 0xFE;
    identity[5] = mac[3];
    identity[6] = mac[4];
    identity[7] = mac[5];
    identity
}

/// Create an AF_PACKET raw socket bound to `ifname`.
fn open_packet_socket_bound(ifname: &str) -> io::Result<(std::fs::File, i32)> {
    // SAFETY: libc calls; check return values.
    unsafe {
        let fd: RawFd = libc::socket(
            libc::AF_PACKET,
            libc::SOCK_RAW,
            (ETH_P_1588 as i32).to_be(), // protocol in network byte order
        );
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        // Get interface index
        let ifname_c = std::ffi::CString::new(ifname)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "ifname contains NUL"))?;

        let ifindex = libc::if_nametoindex(ifname_c.as_ptr());
        if ifindex == 0 {
            let e = io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }

        // Bind socket to interface + protocol
        let sll = libc::sockaddr_ll {
            sll_family: libc::AF_PACKET as u16,
            sll_protocol: (ETH_P_1588).to_be(), // u16 in network byte order
            sll_ifindex: ifindex as i32,
            sll_hatype: 0,
            sll_pkttype: 0,
            sll_halen: 0,
            sll_addr: [0; 8],
        };

        let rc = libc::bind(
            fd,
            &sll as *const libc::sockaddr_ll as *const libc::sockaddr,
            mem::size_of::<libc::sockaddr_ll>() as u32,
        );
        if rc < 0 {
            let e = io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }

        // Wrap fd as File (owns fd) and return with interface index
        Ok((std::fs::File::from_raw_fd(fd), ifindex as i32))
    }
}

/// Continuously listen for L2 PTP (Ethertype 0x88F7), and switch between Sync and Follow_Up state.
///
/// Spawn it with tokio::spawn(async move { monitor_ptp_l2("eth0", Some(0)).await?; })
pub async fn monitor_ptp_l2(
    ifname: &str,
    domain_filter: Option<u8>,
    thrift_ts_client: &mut TsSyncClient<ThriftInputProtocol, ThriftOutputProtocol>,
    sync_status_tx: watch::Sender<PtpSyncStatus>,
) -> io::Result<()> {
    let (file, ifindex) = open_packet_socket_bound(ifname)?;
    let iface_mac = read_interface_mac(ifname)?;
    let own_clock_identity = mac_to_clock_identity(&iface_mac);
    let async_fd = AsyncFd::new(file)?;
    let raw_fd = async_fd.as_raw_fd();
    let mut delay_req_seq: u16 = 1;
    let mut sync_counter = 0;

    // First, read the current increment value to see what's set
    match thrift_ts_client.ts_global_ts_inc_value_get(0) {
        Ok(current_inc) => {
            debug!(
                "Current clock increment value: 0x{:08x} ({})",
                current_inc, current_inc
            );
        }
        Err(e) => {
            warn!("Failed to read current clock increment: {}", e);
        }
    }

    // Reset clock increment to correct value
    // EMPIRICALLY DETERMINED through iterative calibration:
    // Round 1: 0x0D1D6086 (220,029,062) - too fast by 7.6%
    // Round 2: 0x0C2FC5BB (204,457,403) - too slow by 307 µs/s
    // Round 3: 0x0C30C002 (204,520,194) - too fast by 6 µs/s
    // Round 4: 204,520,194 / 1.000006 = 204,518,967 = 0x0C30B5B7
    let default_increment = 0x0c30bb0fi32; // Current: ±100ns accuracy
    if let Err(e) = thrift_ts_client.ts_global_ts_inc_value_set(0, default_increment) {
        warn!("Failed to reset clock increment to default: {}", e);
    } else {
        debug!(
            "Set clock increment to 0x{:08x} ({})",
            default_increment, default_increment
        );
    }

    // Verify it was set correctly
    match thrift_ts_client.ts_global_ts_inc_value_get(0) {
        Ok(new_inc) => {
            debug!(
                "Verified clock increment value: 0x{:08x} ({})",
                new_inc, new_inc
            );
            if new_inc != default_increment as i64 {
                warn!(
                    "Clock increment mismatch! Set 0x{:08x} but read 0x{:08x}",
                    default_increment, new_inc
                );
            }
        }
        Err(e) => {
            warn!("Failed to verify clock increment: {}", e);
        }
    }

    // Reset any existing offset correction
    if let Err(e) = thrift_ts_client.ts_global_ts_offset_value_set(0, 0) {
        warn!("Failed to reset clock offset: {}", e);
    } else {
        debug!("Reset clock offset to 0");
    }

    debug!("Listening for L2 PTP (Ethertype 0x88F7) on interface {ifname}");

    // Send initial status
    let _ = sync_status_tx.send(PtpSyncStatus::Initializing);

    let mut state = SyncState::Idle;
    let mut last_sync = SyncTimes::default();
    let mut ts_tracker = TimestampTracker::new();
    let mut frame = vec![0u8; 2048];
    let mut applied_correction: i64 = 0;
    let mut base_epoch_offset: Option<i128> = None;
    let mut current_increment = default_increment;
    let mut offset_log = PtpOffsetLog::new();
    let mut offset_history = OffsetHistory::new(3);

    // Periodically expire WaitingFollowUp even if traffic stops
    let mut ticker = time::interval(Duration::from_millis(50));

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if let SyncState::WaitingFollowUp { deadline, seq, .. } = &state {
                    if Instant::now() > *deadline {
                        warn!("Timed out waiting for Follow_Up for seq={seq}; returning to Idle");
                        state = SyncState::Idle;
                    }
                }
            }

            ready = async_fd.readable() => {
                let mut guard = ready?;

                // Read as many frames as available (non-blocking)
                loop {
                    match recv_frame(async_fd.as_raw_fd(), &mut frame) {
                        Ok(n) => {
                            if n == 0 { break; }
                            let should_send_delay_req = handle_frame(
                                &mut state,
                                &mut ts_tracker,
                                &mut last_sync,
                                &mut sync_counter,
                                &mut applied_correction,
                                &mut base_epoch_offset,
                                &mut current_increment,
                                &mut offset_log,
                                &mut offset_history,
                                &frame[..n],
                                domain_filter,
                                &own_clock_identity,
                                thrift_ts_client,
                                &sync_status_tx,
                            );
                            if should_send_delay_req {
                                if let Err(e) = send_delay_request(
                                    raw_fd,
                                    ifindex,
                                    &iface_mac,
                                    &mut ts_tracker,
                                    &mut last_sync,
                                    &mut delay_req_seq,
                                    thrift_ts_client,
                                )
                                .await
                                {
                                    warn!("Failed to send Delay_Req: {e}");
                                }
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }

                guard.clear_ready();
            }
        }
    }
}

fn recv_frame(fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
    unsafe {
        let n = libc::recv(
            fd,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len(),
            libc::MSG_DONTWAIT,
        );
        if n < 0 {
            let e = io::Error::last_os_error();
            // Map EAGAIN/EWOULDBLOCK to WouldBlock for tokio loop
            if e.raw_os_error() == Some(libc::EAGAIN) || e.raw_os_error() == Some(libc::EWOULDBLOCK)
            {
                return Err(io::Error::new(io::ErrorKind::WouldBlock, e));
            }
            return Err(e);
        }
        Ok(n as usize)
    }
}

fn transmit_frame(fd: RawFd, ifindex: i32, frame: &[u8]) -> io::Result<()> {
    if frame.len() < ETH_HEADER_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame too short to transmit",
        ));
    }

    let mut addr = libc::sockaddr_ll {
        sll_family: libc::AF_PACKET as u16,
        sll_protocol: (ETH_P_1588).to_be(),
        sll_ifindex: ifindex,
        sll_hatype: 0,
        sll_pkttype: 0,
        sll_halen: 6,
        sll_addr: [0; 8],
    };
    addr.sll_addr[..6].copy_from_slice(&frame[..6]);

    let rc = unsafe {
        libc::sendto(
            fd,
            frame.as_ptr() as *const c_void,
            frame.len(),
            0,
            &addr as *const libc::sockaddr_ll as *const libc::sockaddr,
            mem::size_of::<libc::sockaddr_ll>() as u32,
        )
    };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn handle_frame(
    state: &mut SyncState,
    ts_tracker: &mut TimestampTracker,
    sync_times: &mut SyncTimes,
    sync_counter: &mut u32,
    applied_correction: &mut i64,
    base_epoch_offset: &mut Option<i128>,
    current_increment: &mut i32,
    offset_log: &mut PtpOffsetLog,
    offset_history: &mut OffsetHistory,
    frame: &[u8],
    domain_filter: Option<u8>,
    own_clock_identity: &[u8; 8],
    thrift_ts_client: &mut TsSyncClient<ThriftInputProtocol, ThriftOutputProtocol>,
    sync_status_tx: &watch::Sender<PtpSyncStatus>,
) -> bool {
    // Ethernet header (14 bytes) + PTP header (34 bytes) minimum
    if frame.len() < ETH_HEADER_LEN + 34 {
        return false;
    }

    let ethertype = match parse_ethertype(frame) {
        Some(et) => et,
        None => return false,
    };

    if ethertype != ETH_P_1588 {
        return false;
    }

    // PTP payload starts after Ethernet header
    let ptp = &frame[ETH_HEADER_LEN..];

    let h = match PtpHeader::parse(ptp) {
        Some(h) => h,
        None => return false,
    };

    if h.version != 2 {
        debug!("Ignoring non-PTPv2 packet (version={})", h.version);
        return false;
    }

    if let Some(dom) = domain_filter {
        if h.domain != dom {
            return false;
        }
    }

    let ptp_frame_len = ETH_HEADER_LEN + h.message_length as usize;
    if frame.len() < ptp_frame_len {
        debug!(
            "Frame truncated: len={} expected >= {} for seq={}",
            frame.len(),
            ptp_frame_len,
            h.sequence_id
        );
        return false;
    }
    let ptp_frame = &frame[..ptp_frame_len];

    match h.msg_type {
        PTP_SYNC => {
            let t2 = reconstruct_t2(ts_tracker, frame, &h);
            handle_sync(state, sync_times, &h, ptp_frame, t2);
            false
        }
        PTP_FOLLOW_UP => handle_follow_up(state, sync_times, &h),
        PTP_DELAY_RESP => {
            handle_delay_resp(
                sync_times,
                &h,
                own_clock_identity,
                sync_counter,
                applied_correction,
                base_epoch_offset,
                current_increment,
                offset_log,
                offset_history,
                thrift_ts_client,
                sync_status_tx,
            );
            false
        }
        other => {
            debug!(
                "Got other PTP msg_type=0x{other:x} seq={} domain={}",
                h.sequence_id, h.domain
            );
            false
        }
    }
}

fn reconstruct_t2(
    ts_tracker: &mut TimestampTracker,
    frame: &[u8],
    header: &PtpHeader,
) -> Option<PtpTime> {
    let hw_ts = extract_ingress_timestamp(frame, header)?;
    let full_ts = ts_tracker.combine(hw_ts);
    Some(PtpTime::from_ns(full_ts))
}

fn extract_ingress_timestamp(frame: &[u8], header: &PtpHeader) -> Option<u64> {
    let ptp_end = ETH_HEADER_LEN + header.message_length as usize;
    if frame.len() < ptp_end + APPENDED_TS_LEN {
        debug!(
            "PTP Sync seq={} missing appended ingress timestamp (len={} expected >= {})",
            header.sequence_id,
            frame.len(),
            ptp_end + APPENDED_TS_LEN
        );
        return None;
    }

    let mut bytes = [0u8; APPENDED_TS_LEN];
    bytes.copy_from_slice(&frame[ptp_end..ptp_end + APPENDED_TS_LEN]);
    Some(u64::from_be_bytes(bytes))
}

fn handle_sync(
    state: &mut SyncState,
    sync_times: &mut SyncTimes,
    h: &PtpHeader,
    frame_without_ts: &[u8],
    t2: Option<PtpTime>,
) {
    if let Some(ts) = t2 {
        sync_times.t2 = Some(ts);
    }
    sync_times.last_frame = Some(frame_without_ts.to_vec());
    sync_times.last_header = Some(h.clone());

    if h.two_step() {
        if let Some(ts) = &t2 {
            debug!(
                "Sync(two-step) domain={} seq={} src={:02x?}; T2={} ({} ns); waiting for Follow_Up",
                h.domain, h.sequence_id, h.source_port_identity, ts, ts.raw_ns
            );
        } else {
            warn!(
                "Sync(two-step) domain={} seq={} src={:02x?}; missing ingress timestamp/T2; waiting for Follow_Up",
                h.domain, h.sequence_id, h.source_port_identity
            );
        }
        *state = SyncState::WaitingFollowUp {
            seq: h.sequence_id,
            src: h.source_port_identity,
            deadline: Instant::now() + Duration::from_millis(300),
            t2,
        };
    } else {
        if let Some(ts) = &t2 {
            debug!(
                "Sync(one-step) domain={} seq={} src={:02x?}; T2={} ({} ns); no Follow_Up expected",
                h.domain, h.sequence_id, h.source_port_identity, ts, ts.raw_ns
            );
        } else {
            warn!(
                "Sync(one-step) domain={} seq={} src={:02x?}; missing ingress timestamp/T2; no Follow_Up expected",
                h.domain, h.sequence_id, h.source_port_identity
            );
        }
        *state = SyncState::Idle;
    }
}

fn handle_follow_up(state: &mut SyncState, sync_times: &mut SyncTimes, h: &PtpHeader) -> bool {
    let precise_origin =
        PtpTime::from_components(h.origin_timestamp_seconds, h.origin_timestamp_nanoseconds);
    debug!(
        "Follow_Up domain={} seq={} src={:02x?}; originTimestamp={} ({} ns)",
        h.domain, h.sequence_id, h.source_port_identity, precise_origin, precise_origin.raw_ns
    );
    match state {
        SyncState::WaitingFollowUp { seq, src, t2, .. } => {
            if h.sequence_id == *seq && h.source_port_identity == *src {
                let t1 = PtpTime::from_components(
                    h.origin_timestamp_seconds,
                    h.origin_timestamp_nanoseconds,
                );
                sync_times.t1 = Some(t1);
                // correctionField is in scaled nanoseconds (2^-16 ns); convert to ns
                let cf_ns = h.correction_field >> 16;
                sync_times.cf_followup_ns = cf_ns;
                debug!(
                    "Follow_Up correctionField={} scaled_ns ({} ns)",
                    h.correction_field, cf_ns
                );

                if let Some(ts) = t2.as_ref() {
                    sync_times.t2 = Some(*ts);
                    debug!(
                        "Follow_Up matched domain={} seq={} src={:02x?}; T1={} ({} ns) T2={} ({} ns); returning to Idle",
                        h.domain, h.sequence_id, h.source_port_identity, t1, t1.raw_ns, ts, ts.raw_ns
                    );
                } else {
                    warn!(
                        "Follow_Up matched domain={} seq={} src={:02x?}; stored T1={} ({} ns) but T2 unavailable; returning to Idle",
                        h.domain, h.sequence_id, h.source_port_identity, t1, t1.raw_ns
                    );
                }
                *state = SyncState::Idle;
                return true;
            } else {
                debug!(
                    "Follow_Up unmatched: got seq={}, src={:02x?}; expected seq={}, src={:02x?}",
                    h.sequence_id, h.source_port_identity, *seq, *src
                );
            }
        }
        SyncState::Idle => {
            debug!(
                "Follow_Up while Idle: seq={} src={:02x?} (ignoring)",
                h.sequence_id, h.source_port_identity
            );
        }
    }
    false
}

fn handle_delay_resp(
    sync_times: &mut SyncTimes,
    h: &PtpHeader,
    own_clock_identity: &[u8; 8],
    sync_counter: &mut u32,
    applied_correction: &mut i64,
    base_epoch_offset: &mut Option<i128>,
    current_increment: &mut i32,
    offset_log: &mut PtpOffsetLog,
    offset_history: &mut OffsetHistory,
    thrift_ts_client: &mut TsSyncClient<ThriftInputProtocol, ThriftOutputProtocol>,
    sync_status_tx: &watch::Sender<PtpSyncStatus>,
) {
    // Filter: only process Delay_Resp addressed to our clock
    if let Some(rpi) = &h.requesting_port_identity {
        if rpi[0..8] != own_clock_identity[..] {
            debug!(
                "Delay_Resp seq={} REJECTED: requesting={:02x?}, own={:02x?}",
                h.sequence_id,
                &rpi[0..8],
                own_clock_identity
            );
            return;
        }
        debug!(
            "Delay_Resp seq={} ACCEPTED: requesting={:02x?}, own={:02x?}",
            h.sequence_id,
            &rpi[0..8],
            own_clock_identity
        );
    } else {
        warn!(
            "Delay_Resp seq={} has NO requesting_port_identity (frame too short?), accepting unfiltered",
            h.sequence_id
        );
    }

    // Verify sequence ID matches our Delay_Req
    if let Some(expected_seq) = sync_times.delay_req_seq {
        if h.sequence_id != expected_seq {
            warn!(
                "Delay_Resp seq={} MISMATCH: expected seq={}, discarding to avoid timestamp corruption",
                h.sequence_id, expected_seq
            );
            return;
        }
    } else {
        warn!(
            "Delay_Resp seq={} received but no Delay_Req was sent, discarding",
            h.sequence_id
        );
        return;
    }

    let t4 = PtpTime::from_components(h.origin_timestamp_seconds, h.origin_timestamp_nanoseconds);
    sync_times.t4 = Some(t4);
    // correctionField is in scaled nanoseconds (2^-16 ns); convert to ns
    let cf_ns = h.correction_field >> 16;
    sync_times.cf_delayresp_ns = cf_ns;
    debug!(
        "Delay_Resp domain={} seq={} src={:02x?}; T4={} ({} ns) CF={} ns",
        h.domain, h.sequence_id, h.source_port_identity, t4, t4.raw_ns, cf_ns
    );

    if let Some(sample) = sync_times.take_complete_sample() {
        process_servo_sample(
            sample,
            sync_counter,
            applied_correction,
            base_epoch_offset,
            current_increment,
            offset_log,
            offset_history,
            thrift_ts_client,
            sync_status_tx,
        );
    }
}

async fn send_delay_request(
    fd: RawFd,
    ifindex: i32,
    iface_mac: &[u8; 6],
    ts_tracker: &mut TimestampTracker,
    sync_times: &mut SyncTimes,
    delay_req_seq: &mut u16,
    thrift_ts_client: &mut TsSyncClient<ThriftInputProtocol, ThriftOutputProtocol>,
) -> io::Result<()> {
    let (sw_ts, hw_ts) = match ts_tracker.last_components() {
        Some(values) => values,
        None => {
            warn!("Cannot send Delay_Req: hardware timestamp unavailable");
            return Ok(());
        }
    };
    let full_ts = ts_tracker
        .last_full_timestamp()
        .unwrap_or(((sw_ts as u64) << 48) | (hw_ts & ((1u64 << 48) - 1)));
    let origin_timestamp = PtpTime::from_ns(full_ts);

    let packet =
        match build_delay_req_packet(sync_times, iface_mac, *delay_req_seq, &origin_timestamp) {
            Some(pkt) => pkt,
            None => {
                warn!("Cannot send Delay_Req: missing Sync template");
                return Ok(());
            }
        };

    debug!(
        "Preparing Delay_Req seq={} originTimestamp={} ({} ns); sw_ts=0x{:04x} hw_ts=0x{:012x}",
        *delay_req_seq, origin_timestamp, origin_timestamp.raw_ns, sw_ts, hw_ts
    );

    transmit_frame(fd, ifindex, &packet)?;
    debug!(
        "Delay_Req seq={} transmitted on ifindex={ifindex}",
        *delay_req_seq
    );

    // Wait for the hardware to transmit the packet and update the timestamp register.
    // At higher message rates (e.g., 8/s), reading immediately can return stale data.
    std::thread::sleep(std::time::Duration::from_millis(2));

    // Retry up to 5 times if timestamp is not valid yet or if we get a stale ID
    let last_id = sync_times.last_egress_ts_id;
    let mut ts_result = None;
    for attempt in 0..5 {
        match retrieve_delay_req_egress_ts(thrift_ts_client, PTP_TO_MASTER_DEV_PORT) {
            Ok(ts) => {
                if ts.valid {
                    // Check if this is a stale read (same ID as previous)
                    if let Some(prev_id) = last_id {
                        if ts.id == prev_id {
                            if attempt < 4 {
                                debug!(
                                    "Delay_Req seq={} got stale egress timestamp id={} (attempt {}), retrying...",
                                    *delay_req_seq, ts.id, attempt + 1
                                );
                                std::thread::sleep(std::time::Duration::from_millis(1));
                                continue;
                            } else {
                                warn!(
                                    "Delay_Req seq={} egress timestamp id={} still stale after {} attempts",
                                    *delay_req_seq, ts.id, attempt + 1
                                );
                            }
                        }
                    }
                    ts_result = Some(ts);
                    break;
                } else if attempt < 4 {
                    debug!(
                        "Delay_Req seq={} egress timestamp not yet valid (attempt {}), retrying...",
                        *delay_req_seq,
                        attempt + 1
                    );
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
            Err(e) => {
                warn!(
                    "Unable to retrieve egress timestamp for Delay_Req seq={} (attempt {}): {e}",
                    *delay_req_seq,
                    attempt + 1
                );
                if attempt < 4 {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }
    }

    match ts_result {
        Some(ts) => {
            let full_t3 = ts_tracker.combine(ts.raw_hw_ns);
            let t3 = PtpTime::from_ns(full_t3);
            sync_times.t3 = Some(t3);
            sync_times.delay_req_seq = Some(*delay_req_seq);
            sync_times.last_egress_ts_id = Some(ts.id);
            debug!(
                "Delay_Req seq={} captured egress timestamp id={} raw_hw=0x{:012x}; T3={} ({} ns)",
                *delay_req_seq, ts.id, ts.raw_hw_ns, t3, t3.raw_ns
            );
        }
        None => {
            warn!(
                "Delay_Req seq={} failed to get valid egress timestamp after 5 attempts",
                *delay_req_seq
            );
        }
    }

    *delay_req_seq = delay_req_seq.wrapping_add(1);
    Ok(())
}

fn build_delay_req_packet(
    sync_times: &SyncTimes,
    iface_mac: &[u8; 6],
    seq: u16,
    origin_timestamp: &PtpTime,
) -> Option<Vec<u8>> {
    let template = sync_times.last_frame.as_ref()?;
    let header = sync_times.last_header.as_ref()?;

    if template.len() < ETH_HEADER_LEN + header.message_length as usize {
        return None;
    }
    let mut packet = template.clone();

    let mut master_mac = [0u8; 6];
    master_mac.copy_from_slice(&packet[6..12]);
    packet[..6].copy_from_slice(&master_mac);
    packet[6..12].copy_from_slice(iface_mac);

    let ptp_offset = ETH_HEADER_LEN;
    let ptp_slice = &mut packet[ptp_offset..ptp_offset + header.message_length as usize];

    ptp_slice[0] = (ptp_slice[0] & 0xF0) | PTP_DELAY_REQ;
    ptp_slice[30] = (seq >> 8) as u8;
    ptp_slice[31] = (seq & 0xFF) as u8;
    ptp_slice[32] = 0x01; // control field: Delay_Req

    for byte in &mut ptp_slice[8..16] {
        *byte = 0;
    }

    let clock_identity = mac_to_clock_identity(iface_mac);
    ptp_slice[20..28].copy_from_slice(&clock_identity);
    ptp_slice[28..30].copy_from_slice(&1u16.to_be_bytes());

    let seconds_bytes = origin_timestamp.seconds.to_be_bytes();
    ptp_slice[34..40].copy_from_slice(&seconds_bytes[2..8]);
    ptp_slice[40..44].copy_from_slice(&origin_timestamp.nanoseconds.to_be_bytes());

    Some(packet)
}

fn process_servo_sample(
    sample: PtpServoSample,
    sync_counter: &mut u32,
    applied_correction: &mut i64,
    base_epoch_offset: &mut Option<i128>,
    current_increment: &mut i32,
    offset_log: &mut PtpOffsetLog,
    offset_history: &mut OffsetHistory,
    thrift_ts_client: &mut TsSyncClient<ThriftInputProtocol, ThriftOutputProtocol>,
    sync_status_tx: &watch::Sender<PtpSyncStatus>,
) {
    let t1 = sample.t1.raw_ns as i128;
    let t2 = sample.t2.raw_ns as i128;
    let t3 = sample.t3.raw_ns as i128;
    let t4 = sample.t4.raw_ns as i128;
    let cf_fwd = sample.cf_followup_ns as i128;
    let cf_rev = sample.cf_delayresp_ns as i128;

    debug!(
        "PTP timestamps: t1(master)={} ns, t2(slave_rx)={} ns, t3(slave_tx)={} ns, t4(master)={} ns, cf_followup={} ns, cf_delayresp={} ns",
        t1, t2, t3, t4, cf_fwd, cf_rev
    );

    let path_delay = ((t2 - t1 - cf_fwd) + (t4 - t3 - cf_rev)) / 2;
    let raw_offset = (t2 - t1 - cf_fwd) - path_delay;
    let ptp_time_ns = {
        let estimate = t2 - raw_offset;
        if estimate <= 0 {
            0
        } else if estimate >= u64::MAX as i128 {
            u64::MAX
        } else {
            estimate as u64
        }
    };

    // On first sample, if offset is huge (>1 second), it's likely a time domain mismatch
    // Store this as a base epoch offset and don't apply it to hardware
    if base_epoch_offset.is_none() && raw_offset.abs() > 1_000_000_000 {
        debug!(
            "Detected time domain offset: {} ns (~{} s). Storing as base epoch offset.",
            raw_offset,
            raw_offset / 1_000_000_000
        );
        *base_epoch_offset = Some(raw_offset);
        return;
    }

    // Subtract the base epoch offset to get the actual clock drift
    let offset = if let Some(base) = *base_epoch_offset {
        raw_offset - base
    } else {
        raw_offset
    };

    debug!(
        "PTP update: offsetFromMaster={} ns, pathDelay={} ns (raw_offset={} ns)",
        offset, path_delay, raw_offset
    );

    // Always record the measurement (including outliers) for analysis
    offset_log.record(offset as i64);
    offset_log.write_to_file("../data/ptp/ptp_offset.json");

    // Skip clock adjustment for outlier samples to avoid transient spikes.
    // Allow the first samples through unfiltered for initial convergence.
    const OUTLIER_THRESHOLD_NS: i128 = 150;
    const WARMUP_SAMPLES: u32 = 100;
    if *sync_counter >= WARMUP_SAMPLES && offset.abs() > OUTLIER_THRESHOLD_NS {
        // Detailed debug info to identify the anomalous timestamp
        let fwd_leg = t2 - t1 - cf_fwd; // forward path (Sync direction)
        let rev_leg = t4 - t3 - cf_rev; // reverse path (Delay_Req direction)
        warn!(
            "PTP outlier detected: offset={} ns (threshold={} ns), skipping adjustment. \
             fwd_leg={} ns, rev_leg={} ns, path_delay={} ns, \
             t1={}, t2={}, t3={}, t4={}, cf_fwd={}, cf_rev={}",
            offset,
            OUTLIER_THRESHOLD_NS,
            fwd_leg,
            rev_leg,
            path_delay,
            t1,
            t2,
            t3,
            t4,
            cf_fwd,
            cf_rev
        );
        *sync_counter += 1;
        return;
    }

    adjust_slave_clock(
        thrift_ts_client,
        sync_counter,
        applied_correction,
        current_increment,
        offset,
        ptp_time_ns,
        offset_history,
        sync_status_tx,
    );
}

fn adjust_slave_clock(
    thrift_client: &mut TsSyncClient<ThriftInputProtocol, ThriftOutputProtocol>,
    sync_counter: &mut u32,
    applied_correction: &mut i64,
    current_increment: &mut i32,
    offset_ns: i128,
    ptp_time_ns: u64,
    offset_history: &mut OffsetHistory,
    sync_status_tx: &watch::Sender<PtpSyncStatus>,
) {
    let new_correction = *applied_correction - (offset_ns as i64);
    let synced_ptp_time = PtpTime::from_ns(ptp_time_ns);

    if let Err(e) = thrift_client.ts_global_ts_offset_value_set(0, new_correction) {
        warn!("Failed to set slave clock offset: {}", e);
    } else {
        debug!(
            "Adjusted clock offset: {} ns -> {} ns (measured offset {} ns, synced_ptp_time={} / {} ns)",
            *applied_correction,
            new_correction,
            offset_ns,
            synced_ptp_time,
            ptp_time_ns
        );
        *applied_correction = new_correction;

        // Track offset history and use moving average for increment adjustment
        offset_history.push(offset_ns as i64);
        let avg_offset = offset_history.average().unwrap_or(offset_ns as i64);

        // Adjust clock increment based on moving average offset magnitude and direction:
        // negative offset = slave behind master = clock too slow → increase increment
        // positive offset = slave ahead of master = clock too fast → decrease increment
        // Scale adjustment: larger offsets get larger corrections
        let abs_avg_offset = avg_offset.abs() as i32;
        let adjustment = if abs_avg_offset > 1000 {
            50 // Large offset: aggressive correction
        } else if abs_avg_offset > 500 {
            20
        } else if abs_avg_offset > 100 {
            10
        } else if abs_avg_offset > 50 {
            5
        } else {
            1 // Small offset: fine-tuning
        };

        if avg_offset < 0 {
            *current_increment += adjustment;
        } else if avg_offset > 0 {
            *current_increment -= adjustment;
        }

        if let Err(e) = thrift_client.ts_global_ts_inc_value_set(0, *current_increment) {
            warn!("Failed to adjust clock increment: {}", e);
        } else {
            debug!(
                "Adjusted clock increment to 0x{:08x} ({}) based on avg_offset={} ns",
                *current_increment, *current_increment, avg_offset
            );
        }

        // Publish sync status
        let status = if offset_ns.abs() < 100 {
            PtpSyncStatus::Synchronized {
                offset_ns: offset_ns as i64,
                ptp_time_ns,
            }
        } else {
            PtpSyncStatus::Syncing {
                offset_ns: offset_ns as i64,
                ptp_time_ns,
            }
        };
        let _ = sync_status_tx.send(status);
    }

    *sync_counter += 1;
}

fn ptp_time_plus_2min(s: &str) -> u64 {
    let (secs, nanos) = s.split_once('.').unwrap();
    secs.parse::<u64>().unwrap() * 1_000_000_000 + nanos.parse::<u64>().unwrap() + 120_000_000_000
}

pub async fn wait_for_ptp_sync_and_enable_pktgen(
    mut sync_rx: watch::Receiver<PtpSyncStatus>,
    switch: Arc<SwitchConnection>,
    pktgen_activation_gm_time: String,
) {
    let pktgen_activation_ptp_time_ns = ptp_time_plus_2min(&pktgen_activation_gm_time);

    info!("Waiting for PTP synchronization before enabling packet generator...");
    info!(
        "Packet generator activation target in PTP domain: {} ns (from GM time {} + 120 s)",
        pktgen_activation_ptp_time_ns, pktgen_activation_gm_time
    );

    info!(
        "Activation check mode: deadline-based wait + {} ns check interval in final {} ns spin window",
        PTP_ACTIVATION_CHECK_INTERVAL_NS,
        PTP_ACTIVATION_SPIN_WINDOW_NS
    );

    let mut activation_deadline: Option<Instant> = None;

    loop {
        let status = if let Some(deadline) = activation_deadline {
            let spin_window = Duration::from_nanos(PTP_ACTIVATION_SPIN_WINDOW_NS);
            let check_interval = Duration::from_nanos(PTP_ACTIVATION_CHECK_INTERVAL_NS);
            let wake_at = deadline.checked_sub(spin_window).unwrap_or(deadline);

            tokio::select! {
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(wake_at)) => {
                    let mut now = Instant::now();
                    while now < deadline {
                        let next_check = now
                            .checked_add(check_interval)
                            .map(|candidate| if candidate < deadline { candidate } else { deadline })
                            .unwrap_or(deadline);
                        while Instant::now() < next_check {
                            std::hint::spin_loop();
                        }
                        now = Instant::now();
                    }

                    info!(
                        "Estimated PTP time reached activation target {} ns (deadline reached). Enabling packet generator...",
                        pktgen_activation_ptp_time_ns
                    );
                    match PacketGenerator::enable_pkt_gen(&switch).await {
                        Ok(_) => info!("Packet generator successfully enabled at requested PTP time."),
                        Err(e) => warn!("Failed to enable packet generator: {}", e),
                    }
                    return;
                }
                changed = sync_rx.changed() => {
                    if changed.is_err() {
                        warn!("PTP sync channel closed");
                        return;
                    }
                    sync_rx.borrow().clone()
                }
            }
        } else {
            if sync_rx.changed().await.is_err() {
                warn!("PTP sync channel closed");
                return;
            }
            sync_rx.borrow().clone()
        };

        match status {
            PtpSyncStatus::Initializing => {
                activation_deadline = None;
                info!("PTP synchronization initializing...");
            }
            PtpSyncStatus::Syncing {
                offset_ns,
                ptp_time_ns,
            } => {
                activation_deadline = None;
                debug!(
                    "PTP synchronizing: offset = {} ns, estimated PTP time = {} ns",
                    offset_ns, ptp_time_ns
                );
            }
            PtpSyncStatus::Synchronized {
                offset_ns,
                ptp_time_ns,
            } => {
                if ptp_time_ns >= pktgen_activation_ptp_time_ns {
                    info!(
                        "PTP synchronized and estimated time {} ns already passed activation target {} ns. Enabling packet generator...",
                        ptp_time_ns,
                        pktgen_activation_ptp_time_ns
                    );
                    match PacketGenerator::enable_pkt_gen(&switch).await {
                        Ok(_) => {
                            info!("Packet generator successfully enabled at requested PTP time.")
                        }
                        Err(e) => warn!("Failed to enable packet generator: {}", e),
                    }
                    return;
                }

                let remaining_ns = pktgen_activation_ptp_time_ns.saturating_sub(ptp_time_ns);
                activation_deadline = Some(Instant::now() + Duration::from_nanos(remaining_ns));

                debug!(
                    "PTP synchronized: offset = {} ns, estimated PTP time = {} ns. Waiting for activation target {} ns ({} ns remaining)...",
                    offset_ns,
                    ptp_time_ns,
                    pktgen_activation_ptp_time_ns,
                    remaining_ns
                );
            }
        }
    }
}
