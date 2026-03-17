use std::sync::Arc;

use log::info;
use rbfrt::error::RBFRTError;
use rbfrt::table::{MatchValue, Request, ToBytes};
use rbfrt::{table, SwitchConnection};

use crate::libs::const_definitions::*;
use crate::libs::types::*;

impl TAS {
    pub fn configure_tas_control_recirculation() -> Vec<Request> {
        let mut table_requests = vec![];

        for i in 0..4 {
            let req = Request::new("ingress.tsn_c.mapping_tas_control_recirculation_port")
                .match_key("hdr.timer.pipe_id", MatchValue::exact(i as u32))
                .action("ingress.tsn_c.set_recirculation_port")
                .action_data("recirc_port", RECIRC_PIPE_PORTS_DEV_TF2[i]);
            table_requests.push(req);
        }

        table_requests
    }

    /// Enables Advanced Flow Control on all Pipes
    ///
    pub async fn configure_afc_pipes(switch: &Arc<SwitchConnection>) -> Result<(), RBFRTError> {
        let mut table_requests = vec![];

        for p in 0..4 {
            let req = Request::new("tf2.tm.pipe.sched_cfg")
                .action_data("advanced_flow_control_enable", true)
                .pipe(p)
                .default(true);
            table_requests.push(req);
        }

        switch.update_table_entries(table_requests).await?;

        Ok(())
    }

    /// Decomposes a [start, end] range into a set of ternary (value, mask) entries.
    /// Uses bitmask covering similar to prefix expansion.
    fn range_to_ternary_entries(
        start: u32,
        end: u32,
        pipe_id: u8,
        q_id: u8,
        batch_id: u8,
        action_name: &str,
        afc_value: u32,
    ) -> Vec<Request> {
        let mut requests = Vec::new();
        let mut cur = start;

        while cur <= end {
            if cur == end {
                // Handle a single value case explicitly
                let req = Request::new("egress.tas_c.gate_control_list")
                    .match_key("hdr.gcl_time.diff_ts", MatchValue::ternary(cur, 0xFFFFFFFF)) // exact match
                    .match_key("hdr.timer.app_id", MatchValue::exact(APP_ID_TAS_CONTROL)) // Must be zero
                    .match_key("hdr.timer.pipe_id", MatchValue::exact(pipe_id))
                    .match_key("hdr.timer.packet_id", MatchValue::exact(q_id as u32))
                    .match_key("hdr.timer.batch_id", MatchValue::exact(batch_id as u32))
                    .action(action_name)
                    .action_data("afc", afc_value);
                requests.push(req);
                break;
            }

            let num_remaining = end - cur + 1; // count of values in [cur, end]
            let max_block_size = 1u32 << (31 - num_remaining.leading_zeros()); // largest power of two ≤ count
            let align_size = if cur == 0 {
                1u32 << 31 // cur == 0 is maximally aligned
            } else {
                1u32 << cur.trailing_zeros()
            }; // alignment constraint
            let size = max_block_size.min(align_size);

            let mask = !(size - 1);

            let req = Request::new("egress.tas_c.gate_control_list")
                .match_key("hdr.gcl_time.diff_ts", MatchValue::ternary(cur, mask)) // exact match
                .match_key("hdr.timer.app_id", MatchValue::exact(APP_ID_TAS_CONTROL)) // Must be zero
                .match_key("hdr.timer.pipe_id", MatchValue::exact(pipe_id))
                .match_key("hdr.timer.packet_id", MatchValue::exact(q_id as u32))
                .match_key("hdr.timer.batch_id", MatchValue::exact(batch_id as u32))
                .action(action_name)
                .action_data("afc", afc_value);
            requests.push(req);
            cur += size;
        }

        requests
    }

    pub async fn configure_afc_ports(
        switch: &Arc<SwitchConnection>,
        config: &Arc<Configuration>,
    ) -> Result<(), RBFRTError> {
        let mut table_requests = vec![];

        for mapping in config.tas.clone().gcl_to_port_mapping {
            // Find port group ID of the port group
            let req: table::Request = table::Request::new("tf2.tm.port.cfg")
                .match_key("dev_port", MatchValue::exact(mapping.port));
            let res = switch.get_table_entries(req).await?;
            let port_cfg = res.first().unwrap();
            let pg_id = port_cfg.get_action_data("pg_id")?.as_u32();

            // In TSN, we have 8 queues according to the 3-bit PCP/IPV. We enable AFC for those 8 queues
            for qid in 0..8 {
                // Find out mapping for Port Groups and Queues
                let ingress_qid_map = port_cfg
                    .get_action_data("ingress_qid_map")?
                    .get_data()
                    .to_int_arr();
                let egress_qid_queues = port_cfg
                    .get_action_data("egress_qid_queues")?
                    .get_data()
                    .to_int_arr();
                let pg_queue = egress_qid_queues[ingress_qid_map[qid] as usize];

                let req = Request::new("tf2.tm.queue.sched_cfg")
                    .match_key("pg_id", MatchValue::exact(pg_id))
                    .match_key("pg_queue", MatchValue::exact(pg_queue))
                    .pipe(mapping.port as u32 >> 7)
                    .action_data("advanced_flow_control", "XOFF");
                table_requests.push(req);
            }
        }

        switch.update_table_entries(table_requests).await?;

        Ok(())
    }

    pub async fn get_afc_value(
        switch: &Arc<SwitchConnection>,
        q_id: u8,
        pipe_id: u8,
        port: u8,
        state: u16,
    ) -> Result<u32, RBFRTError> {
        /* 32-bit adv_flow_ctl format */
        // bit<1> qfc;
        // bit<2> tm_pipe_id;
        // bit<4> tm_mac_id;
        // bit<3> _pad;
        // bit<7> tm_mac_qid;
        // bit<15> credit;

        // Find port group ID of the port group
        // TODO move this into a global mapping struct
        let req: table::Request =
            table::Request::new("tf2.tm.port.cfg").match_key("dev_port", MatchValue::exact(port));
        let res = switch.get_table_entries(req).await?;
        let port_cfg = res.first().unwrap();
        let pg_id = port_cfg.get_action_data("pg_id")?.as_u32();

        // Find out mapping for Port Groups and Queues
        let ingress_qid_map = port_cfg
            .get_action_data("ingress_qid_map")?
            .get_data()
            .to_int_arr();
        let egress_qid_queues = port_cfg
            .get_action_data("egress_qid_queues")?
            .get_data()
            .to_int_arr();
        let pg_queue = egress_qid_queues[ingress_qid_map[q_id as usize] as usize];

        //let credit = if state == 1 {0} else {1};

        let afc = AdvancedFlowControl::new(pipe_id, pg_id as u8, pg_queue as u8, state);

        Ok(afc.value)
    }

    pub async fn configure_tas_queue_id(
        switch: &Arc<SwitchConnection>,
        config: &Arc<Configuration>,
    ) -> Result<Vec<Request>, RBFRTError> {
        let mut table_requests = vec![];

        for mapping in config.tas.gcl_to_port_mapping.clone() {
            // Find the GCL object by name
            // TODO error handling
            let gcl = config
                .tas
                .gcls
                .clone()
                .into_iter()
                .find(|x| x.name == mapping.gcl)
                .unwrap();

            let batch_mappings = config.tas.batch_mappings.clone().unwrap();
            let batch_mapping = batch_mappings
                .iter()
                .find(|b| b.egress_dev_port == mapping.port)
                .unwrap();
            let batch_id = batch_mapping.batch_id;

            let entries_before = table_requests.len();

            for t in gcl.time_slices {
                for (q_id, q_state) in t.queue_states {
                    let pipe_id = mapping.port >> 7;
                    let action_name = if q_state == 0 {
                        "egress.tas_c.open_queue"
                    } else {
                        "egress.tas_c.close_queue"
                    };

                    let afc_value =
                        TAS::get_afc_value(switch, q_id, pipe_id, mapping.port, q_state as u16)
                            .await?;

                    let ternary_entries = TAS::range_to_ternary_entries(
                        t.low,
                        t.high,
                        pipe_id,
                        q_id,
                        batch_id,
                        action_name,
                        afc_value,
                    );
                    table_requests.extend(ternary_entries);
                }
            }

            let gcl_entries = table_requests.len() - entries_before;
            info!(
                "tGCL '{}' (port {}, batch_id {}): {} ternary entries",
                mapping.gcl, mapping.port, batch_id, gcl_entries
            );
        }

        let temp = table_requests.len();
        info!("Number of TAS GCL entries: {temp:?}");

        Ok(table_requests)
    }
}
