use macaddr::MacAddr;
use rbfrt::table;
use rbfrt::table::{MatchValue, Request};
use serde_json::Error;
use std::fs;
use std::str::FromStr;

use crate::libs::types::*;

impl AppState {
    pub fn new() -> AppState {
        AppState {
            // App_ID 0 is configured for TAS control traffic
            last_configured_app_id: 1,
            unique_interval_identifier: 0,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl Configuration {
    pub fn new(config_file: String) -> Result<Configuration, Error> {
        let data = fs::read_to_string(config_file).expect("Unable to read file");
        match serde_json::from_str::<Configuration>(&data) {
            Ok(data) => Ok(data),
            Err(e) => {
                eprintln!("Error deserializing JSON: {}", e);
                Err(e)
            }
        }
    }

    pub fn configure_stream_identification(&self) -> Vec<Request> {
        let mut table_requests: Vec<Request> = vec![];

        for stream_id in &self.psfp.streams {
            let stream_handle_entry = self
                .psfp
                .stream_handles
                .iter()
                .find(|h| h.stream_handle == stream_id.stream_handle)
                .cloned()
                .unwrap_or_default();

            let stream_gate_entry = self
                .psfp
                .stream_gates
                .iter()
                .find(|g| g.stream_gate_id == stream_handle_entry.stream_gate_instance)
                .cloned()
                .unwrap_or_default();

            let flow_meter_entry = self
                .psfp
                .flow_meters
                .iter()
                .find(|f| f.flow_meter_id == stream_handle_entry.flow_meter_instance)
                .cloned()
                .unwrap_or_default();

            let stream_filter_entry = self
                .psfp
                .stream_filters
                .iter()
                .find(|s| s.stream_handle == stream_id.stream_handle)
                .cloned()
                .unwrap_or_default();

            let hyperperiod = self
                .psfp
                .stream_gate_schedules
                .iter()
                .find(|s| s.name == stream_gate_entry.schedule)
                .unwrap()
                .period;

            let mut req = table::Request::new("ingress.tsn_c.stream_identification_c.stream_id")
                .match_key(
                    "hdr.ethernet.dst_addr",
                    MatchValue::exact(
                        MacAddr::from_str(&stream_id.eth_dst.clone())
                            .unwrap()
                            .as_bytes()
                            .to_vec(),
                    ), // TODO unwrap_or
                )
                .match_key("hdr.eth_802_1q.vid", MatchValue::exact(stream_id.vid));

            if let Some(new_mac) = stream_id.overwrite_mac.clone() {
                let new_pcp = stream_id.overwrite_pcp.unwrap(); // TODO error handling

                req = req.action("ingress.tsn_c.stream_identification_c.assign_stream_handle_overwrite_mac_and_pcp")
                .action_data("pcp", new_pcp)
                .action_data("eth_dst_addr", MacAddr::from_str(&new_mac)
                            .unwrap()
                            .as_bytes()
                            .to_vec());
            } else {
                req = req.action("ingress.tsn_c.stream_identification_c.assign_stream_handle");
            }

            let mut req = req
                .action_data("stream_handle", stream_handle_entry.stream_handle)
                .action_data(
                    "stream_blocked_due_to_oversize_frame_enable",
                    stream_filter_entry.stream_block_enable, // TODO not implemented for TAS
                )
                .action_data("stream_gate_id", stream_gate_entry.stream_gate_id)
                .action_data(
                    "gate_closed_due_to_invalid_rx_enable",
                    stream_gate_entry.gate_closed_due_to_invalid_rx_enable,
                )
                .action_data(
                    "gate_closed_due_to_octets_exceeded_enable",
                    stream_gate_entry.gate_closed_due_to_octets_exceeded_emable,
                )
                .action_data("flow_meter_instance_id", flow_meter_entry.flow_meter_id)
                .action_data("dropOnYellow", flow_meter_entry.drop_yellow)
                .action_data("markAllFramesRedEnable", flow_meter_entry.mark_red)
                .action_data("colorAware", flow_meter_entry.color_aware)
                .action_data("hyperperiod", hyperperiod);

            if let Some(ipv4_src) = stream_id.ipv4_src {
                req = req.match_key("hdr.ipv4.srcAddr", MatchValue::ternary(ipv4_src, ipv4_src));
            }
            if let Some(ipv4_dst) = stream_id.ipv4_dst {
                req = req.match_key("hdr.ipv4.dstAddr", MatchValue::ternary(ipv4_dst, ipv4_dst));
            }

            table_requests.push(req);
        }

        table_requests
    }

    pub fn configure_app_ids_stream_gate_hyperperiod(&mut self, app_state: &mut AppState) {
        let mut app_mappings: Vec<AppIDMapping> = vec![];

        for stream_gate in &self.psfp.stream_gates {
            let s_gcl_entry = &self
                .psfp
                .stream_gate_schedules
                .iter()
                .find(|gcl| gcl.name == stream_gate.schedule)
                .unwrap(); // TODO handle unwrap if schedule doesnt exist

            let app_id_map = AppIDMapping {
                app_id: app_state.last_configured_app_id,
                hyperperiod_done: false,
                stream_gate_id: stream_gate.stream_gate_id,
                delta: DeltaAdjustment {
                    epsilon_1: 0,
                    epsilon_2: 0,
                    delta: 0,
                    sum: 0,
                },
                hyperperiod_register_value: 0,
                hyperperiod_duration: s_gcl_entry.period,
            };
            app_state.last_configured_app_id += 1;

            app_mappings.push(app_id_map)
        }

        self.psfp.app_id_mappings = Some(app_mappings)
    }

    pub fn configure_app_ids_tas_hyperperiod(&mut self, app_state: &mut AppState) {
        let mut batch_mappings: Vec<BatchIDMapping> = vec![];

        for (batch_cfg, t_gcl_mapping) in self.tas.gcl_to_port_mapping.iter().enumerate() {
            let t_gcl = &self
                .tas
                .gcls
                .iter()
                .find(|t| t.name == t_gcl_mapping.gcl)
                .unwrap();

            let batch_id_map = BatchIDMapping {
                app_id: app_state.last_configured_app_id,
                batch_id: batch_cfg as u8, // This is not used in this case
                egress_dev_port: t_gcl_mapping.port,
                hyperperiod_duration: t_gcl.period as u64,
            };
            app_state.last_configured_app_id += 1;

            batch_mappings.push(batch_id_map)
        }
        self.tas.batch_mappings = Some(batch_mappings)
    }

    pub fn configure_flow_meter(&self) -> Vec<Request> {
        let mut table_requests: Vec<Request> = vec![];

        for f in &self.psfp.flow_meters {
            let req = table::Request::new("ingress.tsn_c.flow_meter_c.flow_meter_instance")
                .match_key(
                    "ig_md.stream_filter.flow_meter_instance_id",
                    MatchValue::exact(f.flow_meter_id),
                )
                .action("ingress.tsn_c.flow_meter_c.set_color_direct")
                .action_data("$METER_SPEC_CIR_KBPS", f.cir_kbps)
                .action_data("$METER_SPEC_PIR_KBPS", f.pir_kbps)
                .action_data("$METER_SPEC_CBS_KBITS", f.cbs)
                .action_data("$METER_SPEC_PBS_KBITS", f.pbs);
            table_requests.push(req);
        }

        table_requests
    }

    pub fn insert_tas_gsi(&mut self) {
        // Inserts a Time slice with a width configured in the config file between every time slice of the GCL.
        // All queues are closed in this time slice.
        // Also increments the period by the duration of the guard bands
        let mut guarded_gcls = vec![];

        for gcl in &self.tas.gcls {
            let mut new_time_slices = Vec::new();
            let mut last_high = 0;

            if gcl.guard_band_width == 0 {
                guarded_gcls.push(gcl.clone());
                continue;
            }

            let mut new_gcl = GateControlList {
                period: gcl.period,
                name: gcl.name.clone(),
                time_slices: vec![],
                guard_band_width: gcl.guard_band_width,
            };

            for ts in &gcl.time_slices {
                // Original timeslice
                let mut orig_ts = ts.clone();
                // Adjust low to be contiguous if needed
                orig_ts.low = last_high;
                orig_ts.high = orig_ts.low + (ts.high - ts.low);
                last_high = orig_ts.high;
                new_time_slices.push(orig_ts);

                // Guard band timeslice
                let mut guard_ts = ts.clone();
                guard_ts.low = last_high;
                guard_ts.high = guard_ts.low + gcl.guard_band_width;
                // Set all queues to 1
                for v in guard_ts.queue_states.values_mut() {
                    *v = 1;
                }
                last_high = guard_ts.high;
                new_time_slices.push(guard_ts);
            }

            new_gcl.period += gcl.guard_band_width * gcl.time_slices.len() as u32;
            new_gcl.time_slices = new_time_slices;
            guarded_gcls.push(new_gcl);
        }
        self.tas.gcls = guarded_gcls;
    }
}
