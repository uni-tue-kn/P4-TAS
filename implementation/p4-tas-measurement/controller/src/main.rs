use log::{info, warn};
use rbfrt::error::RBFRTError;
use rbfrt::register::Register;
use rbfrt::table::MatchValue;
use rbfrt::util::{AutoNegotiation, Port, Speed, FEC};
use rbfrt::util::{PortManager, PrettyPrinter};
use rbfrt::{register, table, SwitchConnection};
use std::collections::HashMap;
use std::fs::{self};
use std::time::Duration;
use std::vec;

pub async fn sync_register(switch: &SwitchConnection, register_name: &str) {
    // Sync register
    let sync = table::Request::new(register_name).operation(table::TableOperation::SyncRegister);
    if switch.execute_operation(sync).await.is_err() {
        warn!("Error in synchronization for register {}.", register_name);
    }
}

fn build_timeseries(
    lower_ts_entries: Result<Register, RBFRTError>,
    higher_ts_entries: Result<Register, RBFRTError>,
    lower_ts_register_name: &str,
    higher_ts_register_name: &str,
    pipe: u8,
) -> Vec<u64> {
    let mut time_series = vec![];
    match (lower_ts_entries, higher_ts_entries) {
        (Ok(lower), Ok(higher)) => {
            if (lower.entries().len() == higher.entries().len()) && (!lower.entries().is_empty()) {
                for i in 0..lower.entries().len() {
                    // Combine lower and higher register entries to a single u64 timestamp

                    let lower_value = lower
                        .entries()
                        .get(&(i as u32))
                        .unwrap()
                        .get_data()
                        .get(&format!("{lower_ts_register_name}.f1"))
                        .unwrap()
                        .get(pipe as usize)
                        .unwrap();
                    let higher_value = higher
                        .entries()
                        .get(&(i as u32))
                        .unwrap()
                        .get_data()
                        .get(&format!("{higher_ts_register_name}.f1"))
                        .unwrap()
                        .get(pipe as usize)
                        .unwrap();

                    // Combine lower and higher values into a single u64 timestamp
                    let byte_array = [
                        0,
                        0,
                        higher_value[0],
                        higher_value[1],
                        lower_value[0],
                        lower_value[1],
                        lower_value[2],
                        lower_value[3],
                    ];

                    let timestamp: u64 = u64::from_be_bytes(byte_array);
                    if timestamp > 0 {
                        time_series.push(timestamp);
                    }
                }
            } else {
                warn!("Lower and higher register entries do not match in length.");
            }
        }
        (Err(e), _) | (_, Err(e)) => {
            warn!("Error reading register entries: {}", e);
        }
    }
    time_series
}

pub async fn get_register_timeseries(
    switch: &SwitchConnection,
    queue_index: u8,
    pipe: u8,
) -> (Vec<u64>, Vec<u64>) {
    let mut time_series_first = vec![];
    let mut time_series_last = vec![];

    let lower_ts_register_first =
        format!("ingress.ts_series_prio_{queue_index}_first_packet_c.lower_last_ts_series");
    let higher_ts_register_first: String =
        format!("ingress.ts_series_prio_{queue_index}_first_packet_c.higher_last_ts_series");

    //let previous_queue = if queue_index == 0 { 7 } else { queue_index - 1 };
    let lower_ts_register_last =
        format!("ingress.ts_series_prio_{queue_index}_last_packet_c.lower_last_ts_series");
    let higher_ts_register_last: String =
        format!("ingress.ts_series_prio_{queue_index}_last_packet_c.higher_last_ts_series");

    // Sync registers first
    sync_register(switch, &lower_ts_register_first).await;
    sync_register(switch, &higher_ts_register_first).await;
    sync_register(switch, &lower_ts_register_last).await;
    sync_register(switch, &higher_ts_register_last).await;

    let lower_ts_first_req = register::Request::new(&lower_ts_register_first);
    let higher_ts_first_req = register::Request::new(&higher_ts_register_first);
    let lower_ts_last_req = register::Request::new(&lower_ts_register_last);
    let higher_ts_last_req = register::Request::new(&higher_ts_register_last);

    let lower_ts_first_entries = switch.get_register_entry(lower_ts_first_req).await;
    let higher_ts_first_entries = switch.get_register_entry(higher_ts_first_req).await;
    let lower_ts_last_entries = switch.get_register_entry(lower_ts_last_req).await;
    let higher_ts_last_entries = switch.get_register_entry(higher_ts_last_req).await;

    time_series_first = build_timeseries(
        lower_ts_first_entries,
        higher_ts_first_entries,
        &lower_ts_register_first,
        &higher_ts_register_first,
        pipe,
    );
    time_series_last = build_timeseries(
        lower_ts_last_entries,
        higher_ts_last_entries,
        &lower_ts_register_last,
        &higher_ts_register_last,
        pipe,
    );

    /*
    info!(
        "Time series for queue {}: first packet timestamps: {:?}, last packet timestamps: {:?}",
        queue_index, time_series_first, time_series_last
    );
     */

    (time_series_first, time_series_last)
}

async fn collect_data(switch: &SwitchConnection) -> HashMap<u8, (Vec<u64>, Vec<u64>)> {
    let mut map: HashMap<u8, (Vec<u64>, Vec<u64>)> = HashMap::new();
    for q in 0..8 {
        let (time_series_first, time_series_last) = get_register_timeseries(switch, q, 1).await;
        map.insert(q, (time_series_first, time_series_last));
    }

    map
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // IMPORTANT
    // For examples on how to use the rbfrt, check the docs
    // https://uni-tue-kn.github.io/rbfrt/rbfrt/

    // Create switch class and set attributes
    // Assumes you forwarded the port via ssh -L ${port}:127.0.0.1:${port} -N ${tofino_name}
    let switch = SwitchConnection::builder("localhost", 50052) // ip and port to reach tofino
        .device_id(0) // Device ID, should be 0 for simple projects
        .client_id(1) // Client ID, should be 1 for simple projects
        .p4_name("p4_tas_measurement") // Name of P4 program without "".p4"
        .connect() // Establish connection
        .await
        .expect("Could not create switch connection!"); // Crash if cannot connect to switch

    // Reset previous port configuration
    switch.clear_table("$PORT").await?;

    // Keep a list of all tables here and clear them once at the beginning.
    // else write_table_entry returns an error if the entry already exists
    let tables: Vec<&str> = vec![
        "ingress.count_priority",
        "ingress.ts_series_prio_0_last_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_0_last_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_1_last_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_1_last_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_2_last_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_2_last_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_3_last_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_3_last_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_4_last_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_4_last_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_5_last_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_5_last_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_6_last_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_6_last_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_7_last_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_7_last_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_0_first_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_0_first_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_1_first_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_1_first_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_2_first_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_2_first_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_3_first_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_3_first_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_4_first_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_4_first_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_5_first_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_5_first_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_6_first_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_6_first_packet_c.higher_last_ts_series",
        "ingress.ts_series_prio_7_first_packet_c.lower_last_ts_series",
        "ingress.ts_series_prio_7_first_packet_c.higher_last_ts_series",
        "ingress.ts_last_packet_c.lower_last_ts_series",
        "ingress.ts_last_packet_c.higher_last_ts_series",
        "ingress.last_queue_active",
        "ingress.next_time_series_index",
    ];
    switch.clear_tables(tables).await?;

    let mut table_requests = vec![];
    for i in 0..8 {
        let req = table::Request::new("ingress.count_priority")
            .match_key("hdr.eth_802_1q.pcp", MatchValue::exact(i))
            .action(&format!("ingress.prio_{i}_count"));
        table_requests.push(req);
    }

    switch.write_table_entries(table_requests).await?;

    // Get object to interact with ports
    let pm = PortManager::new(&switch).await;

    // Collect port requests before sending them to the switch
    let mut port_requests: Vec<Port> = vec![];

    // Configure front panel ports 1-10 with 100 Gbit/s speed and disable auto-negotiation
    let port = Port::new(4, 0)
        .speed(Speed::BF_SPEED_400G) // 100 Gbit/s
        .auto_negotiation(AutoNegotiation::PM_AN_DEFAULT) // AutoNeg should be disabled if not a recirculation port
        .fec(FEC::BF_FEC_TYP_REED_SOLOMON);
    port_requests.push(port);

    // Add all port requests in a single call to the switch
    pm.add_ports(&switch, &port_requests).await?;

    let tp = PrettyPrinter::new();

    // Keep Controller alive
    loop {
        let data = collect_data(&switch).await;

        //info!("Time series for priority 0: {:?}", data.get(&0_u8));
        //info!("Time series for priority 1: {:?}", data.get(&1_u8));

        let table_to_check = "ingress.count_priority";

        let file_path = "../../data/equal-intervals-800us_one-gcl_30ns-gb_1518-B.json";

        // Write the JSON string to the file.
        match fs::write(file_path, serde_json::to_string(&data).unwrap()) {
            Ok(_) => println!("Data successfully written to {}", file_path),
            Err(e) => eprintln!("Failed to write to file: {}", e),
        }
        let sync =
            table::Request::new(table_to_check).operation(table::TableOperation::SyncCounters);

        if switch.execute_operation(sync).await.is_err() {
            warn! {"Encountered error while synchronizing {}.", table_to_check};
        }

        let req: table::Request = table::Request::new(table_to_check);
        let res = switch.get_table_entries(req).await;
        tp.print_table(res.unwrap())?;

        // Sleep for 3s
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    match run().await {
        Ok(_) => {}
        Err(e) => {
            warn!("Error: {e}");
        }
    }
}
