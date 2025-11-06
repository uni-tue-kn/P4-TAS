/// Table for internal traffic generation
pub const APP_CFG_TF2: &str = "tf2.pktgen.app_cfg";

/// Table for internal packet buffer
pub const _APP_BUFFER_CFG_TF2: &str = "tf2.pktgen.pkt_buffer";

/// Table to activate internal traffic generation on ports
pub const PORT_CFG_TF2: &str = "tf2.pktgen.port_cfg";

/// We use traffic generation on the four internal tg ports on tofino2
pub const TG_PIPE_PORTS_TF2: [u16; 4] = [6, 134, 262, 390];

/// Those ports are used as recirculation ports for TAS control on each pipe
pub const RECIRC_PIPE_PORTS_DEV_TF2: [u16; 4] = [16, 192, 264, 440];
pub const RECIRC_PIPE_PORTS_TF2: [u16; 4] = [8, 16, 24, 32];

// Mask to filter for large values (2^38, which is around 4.5 minutes)
pub const MASK_MAX_UNDERFLOW: u64 = 0b111111111111111000000000000000000000000000000000;

// Underflow Mask needed for underflow handling due to inaccuracy in packet generation
pub const MASK_INTERVAL_SWITCH_UNDERFLOW: u64 = 0b111111111111111111111111111111111111111110000000;

/// Digest names
pub const HYPERPEROPD_FINISHED_DIGEST_NAME: &str = "pipe.SwitchIngressDeparser.digest_hyperperiod";
pub const MISSED_SLICE_DIGEST_NAME: &str = "pipe.SwitchIngressDeparser.digest_missed_slice";

pub const APP_ID_TAS_CONTROL: u8 = 0;
