use std::sync::Arc;

use crate::libs::const_definitions::*;
use crate::libs::types::*;
use rbfrt::table::{MatchValue, Request, ToBytes};
use rbfrt::SwitchConnection;
use tokio::sync::Mutex;

use super::const_definitions::HYPERPEROPD_FINISHED_DIGEST_NAME;

impl StreamGateControlList {
    /// Decomposes a [start, end] range into a set of ternary (value, mask) entries.
    /// Uses bitmask covering similar to prefix expansion.
    fn range_to_ternary_entries(
        stream_gate_id: u32,
        start: u32,
        end: u32,
        gate_state: u8,
        ipv: u8,
        interval_identifier: u32,
        max_octets_interval: u32,
    ) -> Vec<Request> {
        let mut requests = Vec::new();
        let mut cur = start;

        while cur <= end {
            let remaining = end - cur;
            if remaining == 0 {
                // Handle a single value case explicitly
                let req = Request::new("ingress.tsn_c.stream_gate_c.stream_gate_instance")
                    .match_key(
                        "ig_md.stream_filter.stream_gate_id",
                        MatchValue::exact(stream_gate_id),
                    )
                    .match_key("ig_md.diff_ts", MatchValue::ternary(cur, 0xFFFFFFFF)) // exact match
                    .action("ingress.tsn_c.stream_gate_c.set_gate_and_ipv")
                    .action_data("gate_state", gate_state)
                    .action_data("ipv", ipv)
                    .action_data("interval_identifier", interval_identifier)
                    .action_data("max_octets_interval", max_octets_interval);
                requests.push(req);
                break;
            }

            let max_block_size = 1 << (31 - remaining.leading_zeros()); // largest power of two ≤ remaining
            let align_size = if cur == 0 {
                1
            } else {
                1 << cur.trailing_zeros()
            }; // alignment constraint
            let size = max_block_size.min(align_size);

            let mask = !(size - 1);

            let req = Request::new("ingress.tsn_c.stream_gate_c.stream_gate_instance")
                .match_key(
                    "ig_md.stream_filter.stream_gate_id",
                    MatchValue::exact(stream_gate_id),
                )
                .match_key("ig_md.diff_ts", MatchValue::ternary(cur, mask)) // exact match
                .action("ingress.tsn_c.stream_gate_c.set_gate_and_ipv")
                .action_data("gate_state", gate_state)
                .action_data("ipv", ipv)
                .action_data("interval_identifier", interval_identifier)
                .action_data("max_octets_interval", max_octets_interval);
            requests.push(req);
            cur += size;
        }

        requests
    }

    pub async fn write_schedule(
        config: &Configuration,
        app_state: &Arc<Mutex<AppState>>,
        app_id: u8,
    ) -> Vec<Request> {
        let app_id_mapping = config
            .psfp
            .app_id_mappings
            .as_ref()
            .and_then(|mappings| mappings.iter().find(|a| a.app_id == app_id))
            .expect("AppIDMapping not found");

        let schedule_name = config
            .psfp
            .stream_gates
            .iter()
            .find(|m| m.stream_gate_id == app_id_mapping.stream_gate_id)
            .expect("Schedule to stream gate mapping not found")
            .schedule
            .clone();

        let stream_gate_schedule = config
            .psfp
            .stream_gate_schedules
            .iter()
            .find(|s| s.name == schedule_name)
            .expect("Stream gate schedule not found");

        let stream_gate = config
            .psfp
            .stream_gates
            .iter()
            .find(|g| g.schedule == schedule_name)
            .unwrap();

        let mut table_requests = Vec::new();
        let mut app_state = app_state.lock().await;
        for t in stream_gate_schedule.intervals.clone() {
            let ternary_entries = StreamGateControlList::range_to_ternary_entries(
                stream_gate.stream_gate_id,
                t.low,
                t.high,
                t.state,
                t.ipv,
                app_state.unique_interval_identifier,
                t.octets,
            );
            app_state.unique_interval_identifier += 1;
            table_requests.extend(ternary_entries);
        }

        table_requests
    }

    pub async fn monitor_digests(
        switch: &Arc<SwitchConnection>,
        config: &Arc<Configuration>,
        app_state: &Arc<Mutex<AppState>>,
    ) {
        if let Ok(digest) = &mut switch.digest_queue.try_recv() {
            if digest.name == HYPERPEROPD_FINISHED_DIGEST_NAME {
                let data = &digest.data;

                // we know how the digest is build
                // unwrap without error handling
                let port = data.get("stream_gate_id").unwrap().to_u32();
                let app_id = data.get("app_id").unwrap().to_u32();
                let pipe_id = data.get("pipe_id").unwrap().to_u32();

                let table_requests =
                    StreamGateControlList::write_schedule(config, app_state, app_id as u8).await;

                let _ = switch.write_table_entries(table_requests).await;

                println!(
                    "Got a digest with stream_gate_id: {:?}, app_id {:?}:, pipe_id {:?}",
                    port, app_id, pipe_id
                );
            } else if digest.name == MISSED_SLICE_DIGEST_NAME {
                let data = &digest.data;

                let reg_hyperperiod = data.get("register_hyperperiod_ts").unwrap().to_u64();
                let diff_ts = data.get("diff_ts").unwrap().to_u64();
                let ingress_timestamp = data.get("ingress_timestamp").unwrap().to_u64();

                //let truncated = (diff_ts & TRUNCATION_MASK as u64) >> 12;

                let calculated: i64 = ingress_timestamp as i64 - reg_hyperperiod as i64;

                println!("Missed Slice Digest, Hyperperiod_Reg {reg_hyperperiod:?}, ingress_ts {ingress_timestamp:?} diff_ts {diff_ts:?}, calculated: {calculated:?}");
            }
        }
    }
}
