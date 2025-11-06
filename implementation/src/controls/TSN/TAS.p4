control TAS(inout header_t hdr, 
            inout egress_metadata_t eg_md, 
            in egress_intrinsic_metadata_t eg_intr_md,
            inout egress_intrinsic_metadata_for_deparser_t eg_intr_md_for_dprsr,
            in egress_intrinsic_metadata_from_parser_t eg_intr_from_prsr) {
    DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_counter1;

    bit<32> previous_queue_state_ts = 0;
    bit<32> diff_dequeue_ts = 0;

    /** 32-bit adv_flow_ctl format */
    // bit<1> qfc;
    // bit<2> tm_pipe_id;
    // bit<4> tm_mac_id;
    // bit<3> _pad;
    // bit<7> tm_mac_qid;
    // bit<15> credit; 
    action open_queue(bit<32> afc){
        /*
        eg_intr_md_for_dprsr.adv_flow_ctl.qfc = 1;
        eg_intr_md_for_dprsr.adv_flow_ctl.tm_pipe_id = hdr.timer.pipe_id;
        eg_intr_md_for_dprsr.adv_flow_ctl.tm_mac_id = port_group_id;
        eg_intr_md_for_dprsr.adv_flow_ctl.tm_mac_qid = hdr.timer.packet_id;
        eg_intr_md_for_dprsr.adv_flow_ctl.credit = 0;
        */
        debug_counter1.count();
        eg_intr_md_for_dprsr.adv_flow_ctl = afc;
        eg_md.new_queue_state = 1; // Set queue state to open
    }

    action close_queue(bit<32> afc){
        //eg_intr_md_for_dprsr.adv_flow_ctl.qfc = 1;
        //eg_intr_md_for_dprsr.adv_flow_ctl.tm_pipe_id = hdr.timer.pipe_id;
        //eg_intr_md_for_dprsr.adv_flow_ctl.tm_mac_id = port_group_id;
        //eg_intr_md_for_dprsr.adv_flow_ctl.tm_mac_qid = hdr.timer.packet_id;
        //eg_intr_md_for_dprsr.adv_flow_ctl.credit = 1;
        eg_intr_md_for_dprsr.adv_flow_ctl = afc;       
        debug_counter1.count();
        eg_md.new_queue_state = 0; // Set queue state to closed
    }

    action missed_time_slot(){
        debug_counter1.count();
    }

    table gate_control_list {
        key = {
            hdr.gcl_time.diff_ts: ternary;
            hdr.timer.app_id: exact;  // Must be 0 for TAS control traffic
            hdr.timer.pipe_id: exact;
            hdr.timer.batch_id: exact; // One batch is used per tGCL. This indicates the RX port.
            hdr.timer.packet_id: exact; // This is configured to correspond to the Queue ID 0-7
        }
        actions = {
            open_queue;
            close_queue;
            missed_time_slot;
        }
        counters = debug_counter1;
        default_action = missed_time_slot;
        size = 39000;
    }    


    Register<bit<8>, bit<8>>(256, 0) queue_state;
    // Action to set the queue state and return true if the queue state changed
    RegisterAction<bit<8>, bit<8>, bool>(queue_state) read_and_set_queue_state = {
            void apply(inout bit<8> value, out bool read_value) {
                if (value != eg_md.new_queue_state) {
                    // The queue state changed -> return 1
                    read_value = true;
                } else {
                    read_value = false;
                }
                value = eg_md.new_queue_state;
            }
    };

    // Register to store the last timestamp of a queue change
    Register<bit<32>, bit<8>>(256, 0) lower_last_ts;
    // Write lower 32 bit of timestamp 
    RegisterAction<bit<32>, bit<8>, void>(lower_last_ts) set_lower_last_ts = {
            void apply(inout bit<32> value) {
                value = eg_intr_from_prsr.global_tstamp[31:0];
            }
    };
    // Read last 32 bit of queue state change 
    RegisterAction<bit<32>, bit<8>, bit<32>>(lower_last_ts) get_lower_last_ts = {
            void apply(inout bit<32> value, out bit<32> read_value) {
                read_value = value;
            }
    };

    // Register to store the difference between dequeue of frame and queue opening
    Register<bit<32>, bit<32>>(90000, 0) reg_ts_dequeue_diff;
    RegisterAction<bit<32>, bit<32>, void>(reg_ts_dequeue_diff) write_ts_diff = {
            void apply(inout bit<32> value) {
                value = diff_dequeue_ts;
            }
     };

    Register<bit<8>, bit<8>>(256, 0) reg_dequeue_ts_measurement;
    RegisterAction<bit<8>, bit<8>, bit<8>>(reg_dequeue_ts_measurement) read_and_deactivate_dequeue_measurement = {
            void apply(inout bit<8> value, out bit<8> read_value) {
                read_value = value;
                value = 0;
            }
     };
    RegisterAction<bit<8>, bit<8>, void>(reg_dequeue_ts_measurement) activate_dequeue_measurement = {
            void apply(inout bit<8> value) {
                value = 1;
            }
     };     

    // Register to count the index in the time series. Only done for prio 0 right now
    Register<bit<32>, bit<32>>(256, 0) next_time_series_index;
    RegisterAction<bit<32>, bit<32>, bit<32>>(next_time_series_index) get_and_increment_next_time_series_index = {
            void apply(inout bit<32> value, out bit<32> read_value) {
                read_value = value;
                value = value + 1;
            }
    };   

     action calculate_dequeue_timestamp() {
        // This action is used to calculate the dequeue timestamp
        // It is called when a packet is dequeued from the TAS queue
        // The dequeue timestamp is calculated as the sum of the enqueue timestamp and the dequeue timedelta
        eg_md.deq_tstamp = eg_intr_md.enq_tstamp + eg_intr_md.deq_timedelta;
     }

    apply {
        // TODO configure packet generator to not generate on all pipes but only the required ones
        if (hdr.timer.isValid() && hdr.timer.app_id == 0) {
            // Change queue state according to GCL
            gate_control_list.apply();

            if (read_and_set_queue_state.execute(0)) {
                // Queue state has changed.
                if (eg_md.new_queue_state == 1) {
                    // Only write timestamp if queue is now open
                    set_lower_last_ts.execute(0);

                    // This sets the register to 1, so that the next TSN packet will trigger a dequeue measurement
                    activate_dequeue_measurement.execute(0);
                }
            }

            // Drop TAS control traffic
            eg_intr_md_for_dprsr.drop_ctl = 3;
        } else {
            diff_dequeue_ts = 0;

            calculate_dequeue_timestamp();

            // Calculate time it took from queue state change until packet was dequeued
            // Retrieve timestamp of last queue state change
            previous_queue_state_ts = get_lower_last_ts.execute((bit<8>)hdr.eth_802_1q.pcp);

            // Calculate difference between current timestamp and previous queue state change
            //diff_dequeue_ts = eg_intr_from_prsr.global_tstamp[31:0] - previous_queue_state_ts;
            diff_dequeue_ts = eg_md.deq_tstamp - previous_queue_state_ts;

            // Write the difference to the register, only for the first packet after queue opened
            bit<8> do_dequeue_ts_measurement = read_and_deactivate_dequeue_measurement.execute((bit<8>)hdr.eth_802_1q.pcp);
            if (do_dequeue_ts_measurement == 1 && hdr.eth_802_1q.pcp == 0) {
                // Only store the value for queue number 0, later maybe do this based on priority in PCP
                eg_md.time_series_index = get_and_increment_next_time_series_index.execute(0);
                write_ts_diff.execute(eg_md.time_series_index);
            }
        }
    }
}
