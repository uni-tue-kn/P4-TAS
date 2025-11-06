use libs::const_definitions::RECIRC_PIPE_PORTS_TF2;
use libs::packet_generator::PacketGenerator;
use libs::types::*;
use macaddr::MacAddr;
use rbfrt::error::RBFRTError;
use rbfrt::table::{MatchValue, Request, ToBytes};
use rbfrt::util::{AutoNegotiation, Loopback, Port, Speed, FEC};
use rbfrt::util::{PortManager, PrettyPrinter};
use rbfrt::{register, table, SwitchConnection};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;

use log::{info, warn};

mod libs;

pub async fn sync_register(switch: &SwitchConnection, register_name: &str) {
    // Sync register
    let sync = table::Request::new(register_name).operation(table::TableOperation::SyncRegister);
    if switch.execute_operation(sync).await.is_err() {
        warn!("Error in synchronization for register {}.", register_name);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Create switch class and set attributes
    // Assumes you forwarded the port via ssh -L ${port}:127.0.0.1:${port} -N ${tofino_name}
    let switch = SwitchConnection::builder("localhost", 50052) // ip and port to reach tofino
        .device_id(0)
        .client_id(1)
        .p4_name("p4_tas") // Name of P4 program without "".p4"
        .connect() // Establish connection
        .await
        .expect("Could not create switch connection!"); // Crash if cannot connect to switch

    // Reset previous port coonfiguration
    switch.clear_table("$PORT").await?;

    // Keep a list of all tables here and clear them once at the beginning.
    // else write_table_entry returns an error if the entry already exists
    let tables: Vec<&str> = vec![
        "ingress.layer_2_forwarding",
        "ingress.tsn_c.app_id_tas",
        "ingress.tsn_c.batch_id_to_port",
        "ingress.tsn_c.stream_identification_c.stream_id",
        "ingress.tsn_c.stream_gate_c.stream_gate_instance",
        /*
        "ingress.tas_control_measurement_c.reg_ts_ingress_mac_tas_control",
        "ingress.tas_control_measurement_c.reg_ts_tas_inter_batch_delay_queue0",
        "ingress.tas_control_measurement_c.reg_ts_tas_intra_batch_delay_queue1",
        "ingress.tas_control_measurement_c.next_time_series_index_inter_batch_delay",
         */
        "ingress.tsn_c.delta_adjustment_c.underflow_detection",
        "ingress.tsn_c.delta_adjustment_c.hyperperiod_exceeded_detection",
        "ingress.tsn_c.app_id_stream_gate",
        "ingress.tsn_c.flow_meter_c.flow_meter_instance",
        "ingress.tsn_c.mapping_tas_control_recirculation_port",
        "ingress.tsn_c.tas_hyperperiod_diff",
        "ingress.tsn_c.next_time_series_index_tas_diff",
        "egress.tas_c.gate_control_list",
        "egress.count_priority",
        "egress.tas_c.queue_state",
        "egress.tas_c.lower_last_ts",
        "egress.tas_c.reg_ts_dequeue_diff",
        "egress.tas_c.reg_dequeue_ts_measurement",
        "egress.tas_c.next_time_series_index",
    ];
    switch.clear_tables(tables).await?;

    // Get object to interact with ports
    let pm = PortManager::new(&switch).await;

    // Collect port requests before sending them to the switch
    let mut port_requests: Vec<Port> = vec![];
    let mut table_requests: Vec<Request> = vec![];

    // TODO move into config file
    let hosts = vec![
        Host {
            name: "p4tg-pazuzu-1".to_string(),
            auto_neg_in: AutoNegotiation::PM_AN_DEFAULT,
            port: 5,
            eth_dst: MacAddr::from([0x81, 0xE7, 0x9D, 0xE3, 0xAD, 0x40]),
            egress_port: 5,
            auto_neg_eg: AutoNegotiation::PM_AN_DEFAULT,
            speed: Speed::BF_SPEED_100G,
        },
        Host {
            name: "p4tg-pazuzu-2".to_string(),
            auto_neg_in: AutoNegotiation::PM_AN_DEFAULT,
            port: 6,
            eth_dst: MacAddr::from([0x81, 0xE7, 0x9D, 0xE3, 0xAD, 0x41]),
            egress_port: 6,
            auto_neg_eg: AutoNegotiation::PM_AN_DEFAULT,
            speed: Speed::BF_SPEED_100G,
        },
        Host {
            name: "p4tg-pazuzu-3".to_string(),
            auto_neg_in: AutoNegotiation::PM_AN_DEFAULT,
            port: 7,
            eth_dst: MacAddr::from([0x81, 0xE7, 0x9D, 0xE3, 0xAD, 0x42]),
            egress_port: 7,
            auto_neg_eg: AutoNegotiation::PM_AN_DEFAULT,
            speed: Speed::BF_SPEED_100G,
        },
        Host {
            name: "p4tg-pazuzu-4".to_string(),
            auto_neg_in: AutoNegotiation::PM_AN_DEFAULT,
            port: 4,
            eth_dst: MacAddr::from([0x81, 0xE7, 0x9D, 0xE3, 0xAD, 0x43]),
            egress_port: 4,
            auto_neg_eg: AutoNegotiation::PM_AN_DEFAULT,
            speed: Speed::BF_SPEED_100G,
        },
        Host {
            name: "measurement-donnie".to_string(),
            auto_neg_in: AutoNegotiation::PM_AN_DEFAULT,
            port: 3,
            eth_dst: MacAddr::from([0x81, 0xE7, 0x9D, 0xE3, 0xAD, 0x48]),
            egress_port: 3,
            auto_neg_eg: AutoNegotiation::PM_AN_DEFAULT,
            speed: Speed::BF_SPEED_400G,
        },
    ];

    for host in &hosts {
        // Configure ingress port
        let mut port = Port::new(host.port, 0)
            .speed(host.speed.clone())
            .auto_negotiation(host.auto_neg_in.clone());
        if host.speed.clone() == Speed::BF_SPEED_400G {
            port = port.fec(FEC::BF_FEC_TYP_REED_SOLOMON)
        }
        port_requests.push(port);

        // Configure egress port
        let egress_port = Port::new(host.egress_port, 0)
            .speed(host.speed.clone())
            .auto_negotiation(host.auto_neg_eg.clone());
        port_requests.push(egress_port);

        // Configure Layer 2 Forwarding
        let req = table::Request::new("ingress.layer_2_forwarding")
            .match_key(
                "hdr.ethernet.dst_addr",
                MatchValue::exact(host.eth_dst.as_bytes().to_vec()),
            )
            .action("ingress.forward")
            .action_data("port", pm.dev_port(host.egress_port, 0).unwrap());
        table_requests.push(req);
    }

    let recirc_requests: Vec<Port> = RECIRC_PIPE_PORTS_TF2
        .into_iter()
        .map(|p| {
            Port::new(p as u32, 0)
                .speed(Speed::BF_SPEED_400G)
                .auto_negotiation(AutoNegotiation::PM_AN_DEFAULT)
                .loopback(Loopback::BF_LPBK_MAC_NEAR)
                .fec(FEC::BF_FEC_TYP_REED_SOLOMON)
        })
        .collect();
    port_requests.extend(recirc_requests);

    // Write all port requests
    pm.add_ports(&switch, &port_requests).await?;

    for i in 0..8 {
        let req = table::Request::new("egress.count_priority")
            .match_key("hdr.eth_802_1q.pcp", MatchValue::exact(i))
            .action(&format!("egress.prio_{i}_count"));
        table_requests.push(req);
    }

    let mut app_state: AppState = AppState::new();
    let mut configuration = Configuration::new("configuration.json".to_string()).unwrap();
    configuration.insert_tas_gsi();
    configuration.configure_app_ids_stream_gate_hyperperiod(&mut app_state);
    configuration.configure_app_ids_tas_hyperperiod(&mut app_state);

    let switch_arc = Arc::new(switch);
    let config_arc = Arc::new(configuration);
    let app_state_arc = Arc::new(Mutex::new(app_state));

    let switch_clone = Arc::clone(&switch_arc);
    let config_clone = Arc::clone(&config_arc);
    let app_state_clone = Arc::clone(&app_state_arc);
    tokio::spawn(async move {
        info!("Started Digest Monitor");
        loop {
            StreamGateControlList::monitor_digests(&switch_clone, &config_clone, &app_state_clone)
                .await;
            tokio::task::yield_now().await;
        }
    });

    PacketGenerator::reset_packet_generator(&switch_arc).await?;

    PacketGenerator::enable_pkt_gen(&switch_arc).await?;
    PacketGenerator::activate_traffic_gen_applications(&config_arc, &switch_arc).await?;

    let tp: PrettyPrinter = PrettyPrinter::new();

    TAS::configure_afc_pipes(&switch_arc).await?;
    TAS::configure_afc_ports(&switch_arc, &config_arc).await?;
    table_requests.extend(
        TAS::configure_tas_queue_id(&switch_arc, &config_arc)
            .await
            .unwrap(),
    );
    table_requests.extend(TAS::configure_tas_control_recirculation());
    table_requests.extend(Configuration::configure_stream_identification(&config_arc));
    table_requests.extend(Configuration::configure_flow_meter(&config_arc));

    table_requests.extend(DeltaAdjustment::init_underflow_detection_table());

    /*
    table_requests.extend(DeltaAdjustment::init_clock_drift_offset_detection_table(
        &config_arc,
    ));
     */

    table_requests.extend(PacketGenerator::configure_app_ids(&config_arc));
    //table_requests.extend(StreamGateControlList::configure_tas_queue_id());

    switch_arc.write_table_entries(table_requests).await?;

    // Keep Controller alive
    loop {
        //debug_prints(&switch_arc, &tp).await?;

        //read_diff_tas_clock_drift(&switch_arc, 1, &config_arc).await;
        //read_diff_queue_change(&switch_arc).await;
        //read_q_depth(&switch_arc).await;
        //read_diff_tas_control(&switch_arc, 1).await;

        /*
        println!(
            "Hyperperiod Stream gate 0 (TAS) {:?}",
            PacketGenerator::read_hyperperiod_register(&switch_arc, 0).await
        );
        println!(
            "Hyperperiod Stream gate 1 {:?}",
            PacketGenerator::read_hyperperiod_register(&switch_arc, 1).await
        );
        println!(
            "Hyperperiod Stream gate 2 {:?}",
            PacketGenerator::read_hyperperiod_register(&switch_arc, 2).await
        );
         */

        // Sleep for 1s
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn read_diff_tas_control(switch: &Arc<SwitchConnection>, pipe: u8) {
    let reg_intra_batch =
        "ingress.tas_control_measurement_c.reg_ts_tas_intra_batch_delay_queue1".to_string();
    let reg_inter_batch =
        "ingress.tas_control_measurement_c.reg_ts_tas_inter_batch_delay_queue0".to_string();

    let mut time_series_intra_batch = vec![];
    let mut time_series_inter_batch = vec![];

    // Sync registers first
    sync_register(switch, &reg_intra_batch).await;
    sync_register(switch, &reg_inter_batch).await;

    let request_intra = register::Request::new(&reg_intra_batch);
    let request_inter = register::Request::new(&reg_inter_batch);

    let entries_intra = switch.get_register_entry(request_intra).await;
    let entries_inter = switch.get_register_entry(request_inter).await;

    if let Ok(entries) = entries_intra {
        let len = entries.entries().len();
        for i in 0..len {
            let value = entries
                .entries()
                .get(&(i as u32))
                .unwrap()
                .get_data()
                .get(&format!("{reg_intra_batch}.f1"))
                .unwrap()
                .get(pipe as usize)
                .unwrap();
            let byte_array = [value[0], value[1]];
            let intra_batch_delay = u16::from_be_bytes(byte_array);

            time_series_intra_batch.push(intra_batch_delay);
        }
    }

    if let Ok(entries) = entries_inter {
        let len = entries.entries().len();
        for i in 0..len {
            let value = entries
                .entries()
                .get(&(i as u32))
                .unwrap()
                .get_data()
                .get(&format!("{reg_inter_batch}.f1"))
                .unwrap()
                .get(pipe as usize)
                .unwrap();
            let byte_array = [value[0], value[1]];
            let inter_batch_delay = u16::from_be_bytes(byte_array);

            time_series_inter_batch.push(inter_batch_delay);
        }
    }

    let file_path = "../data/tas_control_traffic_delay/data.json";
    let json_data = json!({
        "intra_batch_delay": time_series_intra_batch,
        "inter_batch_delay": time_series_inter_batch
    });

    // Write the JSON string to the file.
    match fs::write(file_path, serde_json::to_string(&json_data).unwrap()) {
        Ok(_) => println!("Data successfully written to {}", file_path),
        Err(e) => eprintln!("Failed to write to file: {}", e),
    }

    //info!("Difference between consecutive TAS control batches: {time_series_inter_batch:?}");
    //info!("Difference between consecutive TAS control frames: {time_series_intra_batch:?}");
}

/// Reads the difference between queue opening and frame being dequeued
async fn read_diff_queue_change(switch: &Arc<SwitchConnection>) {
    sync_register(switch, "egress.tas_c.reg_ts_dequeue_diff").await;
    let mut time_series = vec![];

    let diff_req = register::Request::new("egress.tas_c.reg_ts_dequeue_diff");
    let diff_entries = switch.get_register_entry(diff_req).await;

    if let Ok(entries) = diff_entries {
        for i in 0..entries.entries().len() {
            let diff = entries
                .entries()
                .get(&(i as u32))
                .unwrap()
                .get_data()
                .get("egress.tas_c.reg_ts_dequeue_diff.f1")
                .unwrap()
                .get(1) // pipe
                .unwrap();
            let byte_array = [diff[0], diff[1], diff[2], diff[3]];
            let diff = u32::from_be_bytes(byte_array);

            if diff < 4294900000 && diff > 0 {
                // Measurement error due to truncated time stamps in data plane
                time_series.push(diff);
            }
        }
    }
    if !time_series.is_empty() {
        let file_path = "../data/queue_opening_delay/measurement.json";

        // Write the JSON string to the file.
        match fs::write(file_path, serde_json::to_string(&time_series).unwrap()) {
            Ok(_) => println!("Data successfully written to {}", file_path),
            Err(e) => eprintln!("Failed to write to file: {}", e),
        }

        /*
        let sum: u64 = time_series.iter().map(|&x| x as u64).sum();
        let mean = sum as f64 / time_series.len() as f64;
        let variance = time_series
            .iter()
            .map(|&x| {
                let diff = x as f64 - mean;
                diff * diff
            })
            .sum::<f64>()
            / time_series.len() as f64;
        let std_dev = variance.sqrt();
        info!("Mean difference queue state change and dequeuing: {mean}, Std Dev: {std_dev}");
        */
    }
}

/// Reads the inacurracy of TAS hyperperiod packets (clock drift)
pub async fn read_diff_tas_clock_drift(
    switch: &SwitchConnection,
    pipe: u8,
    config: &Configuration,
) -> Vec<i64> {
    let mut time_series = vec![];

    let reg_time_series_clock_drift = "ingress.tsn_c.tas_hyperperiod_diff".to_string();
    let reg_time_series_index = "ingress.tsn_c.next_time_series_index_tas_diff".to_string();

    // Sync registers first
    sync_register(switch, &reg_time_series_index).await;
    sync_register(switch, &reg_time_series_clock_drift).await;

    let request_time_series_clock_drift = register::Request::new(&reg_time_series_clock_drift);

    let entries_time_series_clock_drift = switch
        .get_register_entry(request_time_series_clock_drift)
        .await;

    let hyperperiod_duration = config
        .tas
        .batch_mappings
        .clone()
        .unwrap()
        .first()
        .unwrap()
        .hyperperiod_duration;

    if let Ok(entries) = entries_time_series_clock_drift {
        let len = entries.entries().len();
        info! {"Len: {len:}"};
        for i in 0..len {
            let value = entries
                .entries()
                .get(&(i as u32))
                .unwrap()
                .get_data()
                .get(&format!("{reg_time_series_clock_drift}.f1"))
                .unwrap()
                .get(pipe as usize)
                .unwrap();
            let byte_array = [value[0], value[1], value[2], value[3]];
            let measured_duration = u32::from_be_bytes(byte_array);

            let deviation: i64 = if hyperperiod_duration as i64 > measured_duration as i64 {
                -(hyperperiod_duration as i64 - measured_duration as i64)
            } else {
                measured_duration as i64 - hyperperiod_duration as i64
            };

            if deviation < 1000 {
                time_series.push(deviation);
            }
        }
    }

    let file_path = "../data/clock_drift_measurement/period_400us.json";

    // Write the JSON string to the file.
    match fs::write(file_path, serde_json::to_string(&time_series).unwrap()) {
        Ok(_) => println!("Data successfully written to {}", file_path),
        Err(e) => eprintln!("Failed to write to file: {}", e),
    }

    time_series
}

async fn debug_prints(
    switch: &Arc<SwitchConnection>,
    tp: &PrettyPrinter,
) -> Result<(), RBFRTError> {
    //let table_to_check = "egress.identify_queue";
    //let table_to_check = "ingress.tsn_c.stream_gate_c.stream_gate_instance";
    //let table_to_check = "ingress.tsn_c.stream_identification_c.stream_id";
    //let table_to_check = "ingress.tsn_c.flow_meter_c.flow_meter_instance";
    let table_to_check = "egress.tas_c.gate_control_list";
    //let table_to_check = "ingress.tsn_c.detnet_tsn_relay_c.detnet_active_stream_id";
    //let table_to_check = "ingress.layer_2_forwarding";
    //let table_to_check = "egress.count_priority";
    //let table_to_check = "ingress.tsn_c.delta_adjustment_c.hyperperiod_exceeded_detection";
    //let table_to_check = "ingress.tsn_c.delta_adjustment_c.underflow_detection";
    //let table_to_check = "ingress.tsn_c.app_id_stream_gate";
    //let table_to_check = "ingress.tsn_c.app_id_tas";
    //let table_to_check = "ingress.tsn_c.batch_id_to_port";

    let sync = table::Request::new(table_to_check).operation(table::TableOperation::SyncCounters);

    if switch.execute_operation(sync).await.is_err() {
        warn! {"Encountered error while synchronizing {}.", table_to_check};
    }

    let req: table::Request = table::Request::new(table_to_check);
    let res = switch.get_table_entries(req).await;
    tp.print_table(res.unwrap())?;

    Ok(())
}

#[tokio::main]
async fn main() {
    env_logger::init();

    //run();
    match run().await {
        Ok(_) => {}
        Err(e) => {
            warn!("Error: {}", e);
        }
    }
}
