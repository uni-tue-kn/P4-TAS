#include "controls/IP.p4"
#include "controls/TimestampRegister.p4"
#include "controls/TimestampSeriesRegister.p4"

control ingress(
    inout header_t hdr,
    inout ingress_metadata_t ig_md, in ingress_intrinsic_metadata_t ig_intr_md, in ingress_intrinsic_metadata_from_parser_t ig_prsr_md,
    inout ingress_intrinsic_metadata_for_deparser_t ig_dprsr_md,
    inout ingress_intrinsic_metadata_for_tm_t ig_tm_md) {

    IP() ip_c;
    // Stores the timestamp of the first packet received after a queue state change
    TimestampSeriesRegister() ts_series_prio_0_first_packet_c;
    TimestampSeriesRegister() ts_series_prio_1_first_packet_c;
    TimestampSeriesRegister() ts_series_prio_2_first_packet_c;
    TimestampSeriesRegister() ts_series_prio_3_first_packet_c;
    TimestampSeriesRegister() ts_series_prio_4_first_packet_c;
    TimestampSeriesRegister() ts_series_prio_5_first_packet_c;
    TimestampSeriesRegister() ts_series_prio_6_first_packet_c;
    TimestampSeriesRegister() ts_series_prio_7_first_packet_c;

    // Stores the timestamp of the last packet received in a queue
    TimestampSeriesRegister() ts_series_prio_0_last_packet_c;
    TimestampSeriesRegister() ts_series_prio_1_last_packet_c;
    TimestampSeriesRegister() ts_series_prio_2_last_packet_c;
    TimestampSeriesRegister() ts_series_prio_3_last_packet_c;
    TimestampSeriesRegister() ts_series_prio_4_last_packet_c;
    TimestampSeriesRegister() ts_series_prio_5_last_packet_c;
    TimestampSeriesRegister() ts_series_prio_6_last_packet_c;
    TimestampSeriesRegister() ts_series_prio_7_last_packet_c;    

    // Stores the timestamp of the previous packet, independent of queue
    TimestampRegister() ts_last_packet_c;


    DirectCounter<bit<32>>(CounterType_t.PACKETS) prio_counter;

    Register<bit<8>, bit<8>>(256, 0) last_queue_active;
    // Action to set the last active queue and return true if the active queue changed
    RegisterAction<bit<8>, bit<8>, bool>(last_queue_active) read_and_set_queue_state = {
            void apply(inout bit<8> value, out bool read_value) {
                if (value != ig_md.new_active_queue) {
                    // The queue state changed -> return 1
                    read_value = true;
                } else {
                    read_value = false;
                }
                value = ig_md.new_active_queue;
            }
        };    


    Register<bit<32>, bit<8>>(256, 0) next_time_series_index;
    RegisterAction<bit<32>, bit<8>, bit<32>>(next_time_series_index) get_and_increment_next_time_series_index = {
            void apply(inout bit<32> value, out bit<32> read_value) {
                read_value = value;
                value = value + 1;
            }
    };                                      


    action prio_0_count() {
        prio_counter.count();
    }
    action prio_1_count() {
        prio_counter.count();
    }
    action prio_2_count() {
        prio_counter.count();
    }
    action prio_3_count() {
        prio_counter.count();
    }
    action prio_4_count() {
        prio_counter.count();
    }
    action prio_5_count() {
        prio_counter.count();
    }
    action prio_6_count() {
        prio_counter.count();
    }
    action prio_7_count() {
        prio_counter.count();
    }


    table count_priority {
        key = {
            hdr.eth_802_1q.pcp: exact;
        }
        actions = {
            prio_0_count;
            prio_1_count;
            prio_2_count;
            prio_3_count;
            prio_4_count;
            prio_5_count;
            prio_6_count;
            prio_7_count;
        }
        size = 8; // Number of entries in the table
        counters = prio_counter;
    }

    apply {

        count_priority.apply();

        ig_md.new_active_queue = (bit<8>)hdr.eth_802_1q.pcp;
        ig_md.active_queue_has_changed = read_and_set_queue_state.execute(0);

        // Read the timestamp of the previous packet and write the current timestamp into register
        ts_last_packet_c.apply(ig_intr_md.ingress_mac_tstamp, ig_md.previous_timestamp);

        if (ig_md.active_queue_has_changed){
            ig_md.time_series_index = get_and_increment_next_time_series_index.execute((bit<8>)hdr.eth_802_1q.pcp);

            // The queue has changed, so 
            // - write the current timestamp into the first ts register of the new queue
            // - write the timestamp of the previous packet into the last packet register of the previous queue
            // ! This measurement only works if queues in the GCL are cyclic, i.e., 0-1-2-3-4-5-6-7-0-1-2...

            if (hdr.eth_802_1q.pcp == 0) {
                ts_series_prio_0_first_packet_c.apply(ig_intr_md.ingress_mac_tstamp, ig_md.time_series_index);
                ts_series_prio_7_last_packet_c.apply(ig_md.previous_timestamp, ig_md.time_series_index);
            } else if (hdr.eth_802_1q.pcp == 1) {
                ts_series_prio_1_first_packet_c.apply(ig_intr_md.ingress_mac_tstamp, ig_md.time_series_index);
                ts_series_prio_0_last_packet_c.apply(ig_md.previous_timestamp, ig_md.time_series_index);
            }  else if (hdr.eth_802_1q.pcp == 2) {
                ts_series_prio_2_first_packet_c.apply(ig_intr_md.ingress_mac_tstamp, ig_md.time_series_index);
                ts_series_prio_1_last_packet_c.apply(ig_md.previous_timestamp, ig_md.time_series_index);
            }  else if (hdr.eth_802_1q.pcp == 3) {
                ts_series_prio_3_first_packet_c.apply(ig_intr_md.ingress_mac_tstamp, ig_md.time_series_index);
                ts_series_prio_2_last_packet_c.apply(ig_md.previous_timestamp, ig_md.time_series_index);
            }  else if (hdr.eth_802_1q.pcp == 4) {
                ts_series_prio_4_first_packet_c.apply(ig_intr_md.ingress_mac_tstamp, ig_md.time_series_index);
                ts_series_prio_3_last_packet_c.apply(ig_md.previous_timestamp, ig_md.time_series_index);
            }  else if (hdr.eth_802_1q.pcp == 5) {
                ts_series_prio_5_first_packet_c.apply(ig_intr_md.ingress_mac_tstamp, ig_md.time_series_index);
                ts_series_prio_4_last_packet_c.apply(ig_md.previous_timestamp, ig_md.time_series_index);
            }  else if (hdr.eth_802_1q.pcp == 6) {
                ts_series_prio_6_first_packet_c.apply(ig_intr_md.ingress_mac_tstamp, ig_md.time_series_index);
                ts_series_prio_5_last_packet_c.apply(ig_md.previous_timestamp, ig_md.time_series_index);
            }  else if (hdr.eth_802_1q.pcp == 7) {
                ts_series_prio_7_first_packet_c.apply(ig_intr_md.ingress_mac_tstamp, ig_md.time_series_index);
                ts_series_prio_6_last_packet_c.apply(ig_md.previous_timestamp, ig_md.time_series_index);
            }
        }

        //if (hdr.ipv4.isValid()) {
        //    ip_c.apply(hdr, ig_md, ig_tm_md, ig_intr_md);
        //}
    }

}
