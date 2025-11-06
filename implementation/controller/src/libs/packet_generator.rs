use crate::libs::const_definitions::*;
use crate::libs::types::*;
use rbfrt::error::RBFRTError;

use rbfrt::register::Register;
use rbfrt::table::{MatchValue, Request};
use rbfrt::{register, table, SwitchConnection};
use std::collections::HashMap;
use std::sync::Arc;

use log::{info, warn};

impl AppIDMapping {}

#[derive(Clone)]
pub struct PacketGenerator {}

impl PacketGenerator {
    pub async fn enable_pkt_gen(switch: &Arc<SwitchConnection>) -> Result<(), RBFRTError> {
        // TODO only active for configured pipes

        let req: Vec<Request> = TG_PIPE_PORTS_TF2
            .iter()
            .copied()
            .map(|x| {
                Request::new(PORT_CFG_TF2)
                    .match_key("dev_port", MatchValue::exact(x))
                    .action_data("pktgen_enable", true)
            })
            .collect();

        switch.update_table_entries(req).await?;

        info!("Activated traffic gen capabilities.");

        Ok(())
    }

    pub async fn activate_traffic_gen_applications(
        config: &Configuration,
        switch: &Arc<SwitchConnection>,
    ) -> Result<(), RBFRTError> {
        let mut update_requests: Vec<Request> = vec![];

        let number_of_tas_gcls = config.tas.batch_mappings.clone().unwrap().iter().len();

        // Traffic Gen for TAS Control traffic
        let req = table::Request::new(APP_CFG_TF2)
            .match_key("app_id", MatchValue::exact(APP_ID_TAS_CONTROL))
            .action("trigger_timer_periodic")
            .action_data("app_enable", true)
            .action_data("pkt_len", 64)
            .action_data("timer_nanosec", 1)
            .action_data("batch_count_cfg", number_of_tas_gcls as u16 - 1)
            .action_data("packets_per_batch_cfg", 7) // Actually means 8 packets per batch
            .action_data("pipe_local_source_port", TG_PIPE_PORTS_TF2[0]) // Generating on all pipes, local pipe port
            .action_data("pkt_buffer_offset", 0);
        update_requests.push(req);

        // Traffic Gen for Hyperperiod Packets of Stream Gates
        for mapping in config.psfp.app_id_mappings.clone().unwrap() {
            let req = table::Request::new(APP_CFG_TF2)
                .match_key("app_id", MatchValue::exact(mapping.app_id))
                .action("trigger_timer_periodic")
                .action_data("app_enable", true)
                .action_data("pkt_len", 64)
                .action_data("timer_nanosec", mapping.hyperperiod_duration)
                .action_data("packets_per_batch_cfg", 0)
                .action_data("pipe_local_source_port", TG_PIPE_PORTS_TF2[0]) // Generating on all pipes
                .action_data("pkt_buffer_offset", 0);
            update_requests.push(req);
        }

        // Traffic Gen for Hyperperiod Packets of TAS GCLs
        for mapping in config.tas.batch_mappings.clone().unwrap() {
            let req = table::Request::new(APP_CFG_TF2)
                .match_key("app_id", MatchValue::exact(mapping.app_id))
                .action("trigger_timer_periodic")
                .action_data("app_enable", true)
                .action_data("pkt_len", 64)
                .action_data("timer_nanosec", mapping.hyperperiod_duration)
                .action_data("packets_per_batch_cfg", 0)
                .action_data("pipe_local_source_port", TG_PIPE_PORTS_TF2[0]) // Generating on all pipes
                .action_data("pkt_buffer_offset", 0);
            update_requests.push(req);
        }

        update_requests = update_requests
            .into_iter()
            .map(|req| req.action_data("assigned_chnl_id", TG_PIPE_PORTS_TF2[0]))
            .collect();
        switch.update_table_entries(update_requests).await?;

        Ok(())
    }

    pub async fn reset_packet_generator(switch: &Arc<SwitchConnection>) -> Result<(), RBFRTError> {
        let app_ids: Vec<u8> = (0..16).collect();

        let update_requests: Vec<Request> = app_ids
            .iter()
            .map(|x| {
                table::Request::new(APP_CFG_TF2)
                    .match_key("app_id", MatchValue::exact(*x))
                    .action("trigger_timer_periodic")
                    .action_data("app_enable", false)
            })
            .collect();

        switch.update_table_entries(update_requests).await?;
        let hyperperiod_registers = vec![
            "ingress.tsn_c.lower_last_ts",
            "ingress.tsn_c.higher_last_ts",
            "ingress.tsn_c.hyperperiod_done",
            "ingress.tsn_c.period_count",
        ];
        switch.clear_tables(hyperperiod_registers).await?;
        Ok(())
    }

    ///
    /// Creates a table entry in the assign_app_id_stream_gate table matching for this instance app_id and stream gate id.
    ///
    /// # Returns
    ///
    /// * The table requests to write
    pub fn configure_app_ids(config: &Configuration) -> Vec<Request> {
        let mut table_requests: Vec<Request> = vec![];

        for mapping in config.psfp.app_id_mappings.clone().unwrap() {
            let req = Request::new("ingress.tsn_c.app_id_stream_gate")
                .match_key("hdr.timer.app_id", MatchValue::exact(mapping.app_id))
                .action("ingress.tsn_c.assign_app_id_stream_gate")
                .action_data("stream_gate_id", mapping.stream_gate_id);
            table_requests.push(req);
            info!(
                "app_id {:} to stream gate ID {:} configured.",
                mapping.app_id, mapping.stream_gate_id
            );
            let period_exceeded_reqs = DeltaAdjustment::init_hyperperiod_exceeded_detection_table(
                mapping.hyperperiod_duration,
            );
            table_requests.extend(period_exceeded_reqs);
        }

        // Generated TAS GCL hyperperiod packet with different app ids
        for mapping in config.tas.batch_mappings.clone().unwrap() {
            let req = Request::new("ingress.tsn_c.app_id_tas")
                .match_key("hdr.timer.app_id", MatchValue::exact(mapping.app_id))
                .action("ingress.tsn_c.assign_app_id_tas_port")
                .action_data("port", mapping.egress_dev_port)
                .action_data("duration", mapping.hyperperiod_duration);
            table_requests.push(req);
            info!(
                "app_id {:} to TAS GCL port{:} configured.",
                mapping.app_id, mapping.egress_dev_port
            );

            // Generated TAS Control traffic frame with app_id = 0 and different batch IDs
            let req = Request::new("ingress.tsn_c.batch_id_to_port")
                .match_key("hdr.timer.batch_id", MatchValue::exact(mapping.batch_id))
                .action("ingress.tsn_c.assign_batch_id_tas_port")
                .action_data("port", mapping.egress_dev_port)
                .action_data("hyperperiod", mapping.hyperperiod_duration);
            table_requests.push(req);
            info!(
                "Batch ID {:} to TAS GCL Port {:} configured.",
                mapping.batch_id, mapping.egress_dev_port
            );

            let period_exceeded_reqs = DeltaAdjustment::init_hyperperiod_exceeded_detection_table(
                mapping.hyperperiod_duration,
            );
            table_requests.extend(period_exceeded_reqs);
        }

        table_requests
    }

    pub async fn read_register(
        register_name: String,
        index: u32,
        switch: &SwitchConnection,
    ) -> Register {
        let requests = vec![register::Request::new(&register_name).index(index)];

        let sync =
            table::Request::new(&register_name).operation(table::TableOperation::SyncRegister);
        // sync register
        if switch.execute_operation(sync).await.is_err() {
            warn!("Error in synchronization for register {}.", register_name);
        }

        let fut = switch.get_register_entries(requests.clone()).await;

        match fut {
            Ok(f) => f,
            Err(err) => {
                warn!("Error in monitor_iat. Error: {}", format!("{:#?}", err));
                Register::new("default", HashMap::new())
            }
        }
    }

    pub async fn read_hyperperiod_register(switch: &Arc<SwitchConnection>, index: u8) -> u64 {
        // The index to the register is the ingress port. We determine the pipe_id to use the correct register value
        let pipe: u8 = index >> 7;

        let lower_register = PacketGenerator::read_register(
            "ingress.tsn_c.lower_last_ts".to_string(),
            index as u32,
            switch,
        )
        .await;
        let higher_register = PacketGenerator::read_register(
            "ingress.tsn_c.higher_last_ts".to_string(),
            index as u32,
            switch,
        )
        .await;

        let lower_entries = lower_register
            .get(index as u32)
            .unwrap()
            .get_data()
            .get("ingress.tsn_c.lower_last_ts.f1")
            .unwrap()
            .get(pipe as usize)
            .unwrap();
        let higher_entries = higher_register
            .get(index as u32)
            .unwrap()
            .get_data()
            .get("ingress.tsn_c.higher_last_ts.f1")
            .unwrap()
            .get(pipe as usize)
            .unwrap();

        let byte_array = [
            0,
            0,
            higher_entries[0],
            higher_entries[1],
            lower_entries[0],
            lower_entries[1],
            lower_entries[2],
            lower_entries[3],
        ];

        let timestamp: u64 = u64::from_be_bytes(byte_array);

        timestamp
    }
}
