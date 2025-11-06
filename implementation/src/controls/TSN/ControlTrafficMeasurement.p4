control TASControlMeasurement(inout header_t hdr, 
            inout ingress_metadata_t ig_md,
            in ingress_intrinsic_metadata_t ig_intr_md
) {

    bit<16> previous_ts = 0;
    bit<16> inter_batch_delay = 0;
    bit<16> intra_batch_delay = 0;
    Register<bit<16>, bit<8>>(8, 0) reg_ts_ingress_mac_tas_control;
    RegisterAction<bit<16>, bit<8>, bit<16>>(reg_ts_ingress_mac_tas_control) read_and_write_current_ingress_ts_lower = {
            void apply(inout bit<16> value, out bit<16> read_value) {
                // Return the previous value and write the new value
                read_value = value;
                value = ig_intr_md.ingress_mac_tstamp[15:0];
            }
    };
    RegisterAction<bit<16>, bit<8>, bit<16>>(reg_ts_ingress_mac_tas_control) read_current_ingress_ts_lower = {
            void apply(inout bit<16> value, out bit<16> read_value) {
                // Return the previous value
                read_value = value;
                value = value;
            }
    };    
    Register<bit<16>, bit<32>>(100000, 0) reg_ts_tas_inter_batch_delay_queue0;
    RegisterAction<bit<16>, bit<32>, void>(reg_ts_tas_inter_batch_delay_queue0) write_tas_inter_batch_delay_queue0 = {
            void apply(inout bit<16> value) {
                value = inter_batch_delay;
            }
    };
    Register<bit<16>, bit<32>>(100000, 0) reg_ts_tas_intra_batch_delay_queue1;
    RegisterAction<bit<16>, bit<32>, void>(reg_ts_tas_intra_batch_delay_queue1) write_tas_intra_batch_delay_queue1 = {
            void apply(inout bit<16> value) {
                value = intra_batch_delay;
            }
    };    

    Register<bit<32>, bit<8>>(256, 0) next_time_series_index_inter_batch_delay;
    RegisterAction<bit<32>, bit<8>, bit<32>>(next_time_series_index_inter_batch_delay) get_and_increment_next_time_series_inter_batch_delay = {
            void apply(inout bit<32> value, out bit<32> read_value) {
                read_value = value;
                value = value + 1;
            }
    }; 


    apply {
        if (hdr.timer.isValid() && hdr.timer.app_id == 0) {

            bit<8> prio = (bit<8>)hdr.timer.packet_id;

            bit<32> ts_index = get_and_increment_next_time_series_inter_batch_delay.execute(prio);

            if (hdr.timer.batch_id == 0) {
                if (hdr.timer.packet_id == 0) {
                    // INTER-BATCH DELAY CALCULATION
                    // MEASUREMENT: Time between two TAS control packets
                    previous_ts = read_and_write_current_ingress_ts_lower.execute(prio);

                    // Calculate the difference between the current and previous ingress timestamp
                    // This is the time between two TAS control packets
                    inter_batch_delay = ig_intr_md.ingress_mac_tstamp[15:0] - previous_ts;

                    write_tas_inter_batch_delay_queue0.execute(ts_index);
                } else if (hdr.timer.packet_id == 1) {
                    // INTRA-BATCH DELAY CALCULATION
                    previous_ts = read_current_ingress_ts_lower.execute(0);

                    intra_batch_delay = ig_intr_md.ingress_mac_tstamp[15:0] - previous_ts;

                    write_tas_intra_batch_delay_queue1.execute(ts_index);
                }
            }
        }
    }
}