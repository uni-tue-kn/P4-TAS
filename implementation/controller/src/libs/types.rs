use macaddr::MacAddr;
use rbfrt::util::AutoNegotiation;
use rbfrt::util::Speed;
use rbfrt::util::FEC;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::Ipv4Addr};

pub struct PortConfig {
    pub port: u32,
    pub channel: u8,
    pub speed: Speed,
    pub auto_neg: AutoNegotiation,
    pub fec: Option<FEC>,
}

pub struct L2ForwardingEntry {
    pub eth_dst: MacAddr,
    pub egress_port: u32,
    pub egress_channel: u8,
}

pub struct AppState {
    pub last_configured_app_id: u8,
    pub unique_interval_identifier: u32,
}

fn default_pktgen_activation_gm_time() -> String {
    "1772647368.495519698".to_string()
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Configuration {
    #[serde(default = "default_pktgen_activation_gm_time")]
    pub pktgen_activation_gm_time: String,
    pub psfp: PSFP,
    pub tas: TAS,
}

#[derive(Serialize, Deserialize, Clone)]

pub struct PSFP {
    pub streams: Vec<Stream>,
    pub stream_handles: Vec<StreamHandle>,
    pub stream_filters: Vec<StreamFilter>,
    pub stream_gates: Vec<StreamGate>,
    pub flow_meters: Vec<FlowMeter>,
    pub stream_gate_schedules: Vec<StreamGateControlList>,
    pub app_id_mappings: Option<Vec<AppIDMapping>>,
}

#[derive(Serialize, Deserialize, Clone)]

pub struct Stream {
    pub vid: u16,
    pub stream_handle: u8,
    pub eth_src: Option<String>,    // TODO
    pub eth_dst: String,            // TODO
    pub overwrite_vid: Option<u16>, // Used to overwrite an existing VID with active ID
    pub overwrite_pcp: Option<u8>,
    pub overwrite_mac: Option<String>,
    pub dst_port: Option<u32>,
    pub ipv4_src: Option<Ipv4Addr>,
    pub ipv4_dst: Option<Ipv4Addr>,
    pub ipv4_diffserv: Option<u8>,
    pub ipv4_protocol: Option<u8>,
    pub src_port: Option<u32>,
}
#[derive(Serialize, Deserialize, Clone, Copy, Default)]
pub struct StreamHandle {
    pub stream_handle: u8,
    pub stream_gate_instance: u32,
    pub flow_meter_instance: u32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Default)]
pub struct StreamFilter {
    pub stream_handle: u8,
    pub stream_block_enable: bool,
    pub max_sdu: u32,
}

#[derive(Serialize, Deserialize, Clone, Default)]

pub struct StreamGate {
    pub stream_gate_id: u32,
    pub schedule: String,
    #[serde(default)]
    pub gate_closed_due_to_invalid_rx_enable: bool,
    #[serde(default)]
    pub gate_closed_due_to_octets_exceeded_emable: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Default)]
pub struct FlowMeter {
    pub flow_meter_id: u32,
    pub cir_kbps: u64,
    pub pir_kbps: u64,
    pub cbs: u64,
    pub pbs: u64,
    #[serde(default)]
    pub drop_yellow: bool,
    #[serde(default)]
    pub mark_red: bool,
    #[serde(default)]
    pub color_aware: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy)]

pub struct PSFPTimeSlice {
    pub low: u32,
    pub high: u32,
    pub state: u8,
    pub ipv: u8,
    pub octets: u32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct StreamGateControlList {
    pub name: String,
    pub period: u64,
    pub intervals: Vec<PSFPTimeSlice>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AppIDMapping {
    pub app_id: u8,
    pub stream_gate_id: u32,
    pub hyperperiod_done: bool,
    pub delta: DeltaAdjustment,
    pub hyperperiod_register_value: u64,
    pub hyperperiod_duration: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BatchIDMapping {
    pub app_id: u8,
    pub batch_id: u8,
    pub egress_dev_port: u8,
    pub hyperperiod_duration: u64,
    //pub delta: DeltaAdjustment,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeltaAdjustment {
    pub epsilon_1: i64,
    pub epsilon_2: i64,
    pub delta: i64,
    pub sum: i64,
}

#[derive(Serialize, Deserialize, Clone)]

pub struct TAS {
    pub gcl_to_port_mapping: Vec<GCLToPortMapping>,
    pub gcls: Vec<GateControlList>,
    pub batch_mappings: Option<Vec<BatchIDMapping>>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GCLToPortMapping {
    pub port: u8,
    pub gcl: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GateControlList {
    pub period: u32,
    pub name: String,
    pub time_slices: Vec<TASTimeSlice>,
    pub guard_band_width: u32,
}
#[derive(Serialize, Deserialize, Clone, Debug)]

pub struct TASTimeSlice {
    pub low: u32,
    pub high: u32,
    pub queue_states: HashMap<u8, u8>, // Mapping from Queue ID to Queue State
}

pub struct AdvancedFlowControl {
    // bit<1> qfc;
    // bit<2> tm_pipe_id;
    // bit<4> tm_mac_id;
    // bit<3> _pad;
    // bit<7> tm_mac_qid;
    // bit<15> credit;
    pub value: u32,
}
