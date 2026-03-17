//#include "PSFP/StreamFilter.p4"
#include "PSFP/StreamGate.p4"
#include "PSFP/FlowMeter.p4"
#include "StreamIdentification.p4"
#include "DeltaAdjustment.p4"

control TSN(inout header_t hdr, 
            inout ingress_metadata_t ig_md, 
            inout ingress_intrinsic_metadata_for_tm_t ig_tm_md, 
            in ingress_intrinsic_metadata_t ig_intr_md, 
            inout ingress_intrinsic_metadata_for_deparser_t ig_dprsr_md,
            in ingress_intrinsic_metadata_from_parser_t ig_prsr_md) {


    DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_counter;
    DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_counter2;
    DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_counter3;
    DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_counter4;
    DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_counter5;

    // ig_intr_md.ingress_mac_tstamp;
    Register<bit<64>, PortId_t>(256, 0) reg_ts_ingress_mac;
    RegisterAction<bit<64>, PortId_t, void>(reg_ts_ingress_mac) write_ingress_ts = {
            void apply(inout bit<64> value) {
                value = (bit<64>)ig_intr_md.ingress_mac_tstamp;
            }
     };
    // ig_intr_md.global_tstamp;
    Register<bit<64>, PortId_t>(256, 0) reg_ts_enter_ingress_parser;
    RegisterAction<bit<64>, PortId_t, void>(reg_ts_enter_ingress_parser) write_ingress_parser_ts = {
            void apply(inout bit<64> value) {
                value = (bit<64>)ig_prsr_md.global_tstamp;
            }
     };        
    

    // TSN
    DeltaAdjustment() delta_adjustment_c;

    // PSFP
    //StreamFilter() streamFilter_c;
    StreamGate() stream_gate_c;
    FlowMeter() flow_meter_c;

    StreamIdentification() stream_identification_c;

    /*
    ------
    PSFP hyperperiod mechanism
    */
    Register<bit<32>, bit<16>>(256, 0) lower_last_ts;
    Register<bit<16>, bit<16>>(256, 0) higher_last_ts;
    Register<bit<1>, bit<16>>(256, 0) hyperperiod_done;
    Register<bit<32>, bit<16>>(256, 0) period_count;
    // Read the previous value from this register. Set it to 1 afterwards.
    RegisterAction<bit<1>, bit<16>, bit<1>>(hyperperiod_done) handle_hyperperiod_done = {
            void apply(inout bit<1> value, out bit<1> read_value) {
                read_value = value;
                value = 1;
            }
    };
    // Write lower 32 bit of timestamp 
    RegisterAction<bit<32>, bit<16>, void>(lower_last_ts) set_lower_last_ts = {
            void apply(inout bit<32> value) {
                value = ig_intr_md.ingress_mac_tstamp[31:0];
            }
    };

    // Read lower 32 bit of timestamp
    RegisterAction<bit<32>, bit<16>, bit<32>>(lower_last_ts) get_lower_last_ts = {
            void apply(inout bit<32> value, out bit<32> read_value) {
                read_value = value;
                value = value;
            }
    };

    // Write higher 16 bit of timestamp 
    RegisterAction<bit<16>, bit<16>, void>(higher_last_ts) set_higher_last_ts = {
            void apply(inout bit<16> value) {
                value = ig_intr_md.ingress_mac_tstamp[47:32];
            }
    };

    // Read higher 16 bit of timestamp 
    RegisterAction<bit<16>, bit<16>, bit<16>>(higher_last_ts) get_higher_last_ts = {
            void apply(inout bit<16> value, out bit<16> read_value) {
                read_value = value;
                value = value;
            }
    };

    // Increment period
    RegisterAction<bit<32>, bit<16>, bit<32>>(period_count) increment_period_count = {
        void apply(inout bit<32> value) {
            value = value + 1;
        }
    };

    RegisterAction<bit<32>, bit<16>, bit<32>>(period_count) get_period_count = {
        void apply(inout bit<32> value, out bit<32> read_value) {
            read_value = value;
        }
    };
    /*
    ------
    */


    /*
    ------
    TAS hyperperiod mechanism
    */
    Register<bit<32>, PortId_t>(256, 0) tas_lower_last_ts;
    Register<bit<16>, PortId_t>(256, 0) tas_higher_last_ts;
    // Write lower 32 bit of timestamp 
    RegisterAction<bit<32>, PortId_t, bit<32>>(tas_lower_last_ts) tas_get_and_set_lower_last_ts = {
            void apply(inout bit<32> value, out bit<32> read_value) {
                read_value = value;
                value = ig_intr_md.ingress_mac_tstamp[31:0];
            }
    };
    // Read lower 32 bit of timestamp
    RegisterAction<bit<32>, PortId_t, bit<32>>(tas_lower_last_ts) tas_get_lower_last_ts = {
            void apply(inout bit<32> value, out bit<32> read_value) {
                read_value = value;
                value = value;
            }
    };

    // Write higher 16 bit of timestamp 
    RegisterAction<bit<16>, PortId_t, bit<16>>(tas_higher_last_ts) tas_get_and_set_higher_last_ts = {
            void apply(inout bit<16> value, out bit<16> read_value) {
                read_value = value;
                value = ig_intr_md.ingress_mac_tstamp[47:32];
            }
    };

    // Read higher 16 bit of timestamp 
    RegisterAction<bit<16>, PortId_t, bit<16>>(tas_higher_last_ts) tas_get_higher_last_ts = {
            void apply(inout bit<16> value, out bit<16> read_value) {
                read_value = value;
                value = value;
            }
    };
    /*
    ---------
    */



    /*
    ---------
    TAS clock drift measurement (delta_TG)
    */
    Register<bit<32>, bit<8>>(256, 0) next_time_series_index_tas_diff;
    RegisterAction<bit<32>, bit<8>, bit<32>>(next_time_series_index_tas_diff) get_and_increment_next_time_series_index_tas_diff = {
            void apply(inout bit<32> value, out bit<32> read_value) {
                read_value = value;
                value = value + 1;
            }
    };    
    Register<bit<32>, bit<32>>(16000, 0) tas_hyperperiod_diff;
    // Set diff in packet generation of TAS hyperperiod packet
    RegisterAction<bit<32>, bit<32>, void>(tas_hyperperiod_diff) set_tas_diff = {
            void apply(inout bit<32> value) {
                value = ig_md.hyperperiod.tas_diff;
            }
    };
    RegisterAction<bit<32>, bit<32>, bit<32>>(tas_hyperperiod_diff) get_tas_diff = {
            void apply(inout bit<32> value, out bit<32> read_value) {
                read_value = value;
                value = value;
            }
    };   
    /*
    ------
    */ 


    action assign_app_id_stream_gate(bit<16> stream_gate_id){
        ig_md.hyperperiod.stream_gate_id = stream_gate_id;
        debug_counter2.count();
    }

    action assign_app_id_tas_port(PortId_t port, bit<64> duration){
        ig_md.hyperperiod.tas_port = port;
        ig_md.hyperperiod.duration = duration;
        debug_counter4.count();
    }

    action assign_batch_id_tas_port(PortId_t port, bit<64> hyperperiod) {
        // TODO maybe use a different field
        ig_md.hyperperiod.tas_port = port;
        ig_md.hyperperiod.duration = hyperperiod;
        debug_counter5.count();
    }

    action set_recirculation_port(PortId_t recirc_port){
        ig_tm_md.ucast_egress_port = recirc_port;
        debug_counter3.count();
    }

    /*
    Each ingress port has a dedicated recirculation port.
    The recirculation port must be on the same pipe.
    */
    table mapping_tas_control_recirculation_port {
        key = {
            hdr.timer.pipe_id: exact;
        }
        actions = {
            set_recirculation_port;
        }
        size = 4;
        counters = debug_counter3;
    }
    
    action calc_diff_ts(){
        /*
        Calculates the relative position in the hyperperiod by subtracting ingress ts from hyperperiod ts
        */
        ig_md.diff_ts = ig_intr_md.ingress_mac_tstamp - ig_md.hyperperiod_ts;
    }

    /*
    This table maps the app_id of a stream gate GCL to a stream gate. It is used for hyperperiod timestamps of PSFP
    */
    table app_id_stream_gate {
        key = {
            hdr.timer.app_id: exact;
        }
        actions = {
            assign_app_id_stream_gate;
        }
        size = 16;
        counters = debug_counter2;
    }

    /*
    This table maps an app id to a TAS RX Port. It is used for hyperperiod timestamps of TAS
    */
    table app_id_tas {
        key = {
            hdr.timer.app_id: exact;
        }
        actions = {
            assign_app_id_tas_port;
        }
        size = 16;
        counters = debug_counter4;
    }    

    /*
    This table is used to map multiple batch ids of TAS control traffic (app_id = 0) to an RX port.
    */
    table batch_id_to_port {
        key = {
            hdr.timer.batch_id: exact;
        }
        actions = {
            assign_batch_id_tas_port;
        }
        size = 32;
        counters = debug_counter5;
    }

    apply {
        /* 
        Depending on the ingress port, the hyperperiod register will be updated
        with the most recent ingress timestamp (ingress port == pkt gen port, generated packet)
        or the latest value will be read into ig_md.hyperperiod.hyperperiod_ts (TSN eligible packet).
        */
        if (hdr.timer.isValid() && hdr.timer.app_id != 0){
            // Generated hyperperiod packet
                if (app_id_stream_gate.apply().hit){
                    // Increments or resets the packet count register
                    // Write new hyperperiod timestamp and reset pkt_count
                    set_lower_last_ts.execute(ig_md.hyperperiod.stream_gate_id);
                    set_higher_last_ts.execute(ig_md.hyperperiod.stream_gate_id);

                    bit<1> is_hyperperiod_done = handle_hyperperiod_done.execute(ig_md.hyperperiod.stream_gate_id);

                    if (is_hyperperiod_done == 0){
                        // Only send a digest for the first hyperperiod done.
                        // Otherwise we would flood the control plane with digests
                        ig_dprsr_md.digest_type = 6;
                    }
                    // Currently not used
                    increment_period_count.execute(ig_md.hyperperiod.stream_gate_id);
                } else if (app_id_tas.apply().hit){
                    bit<64> previous_tas_ts = 0;

                    // Write hyperperiod timestamps of completed TAS GCL period for a specific RX port
                    previous_tas_ts[31:0] = tas_get_and_set_lower_last_ts.execute(ig_md.hyperperiod.tas_port);
                    previous_tas_ts[47:32] = tas_get_and_set_higher_last_ts.execute(ig_md.hyperperiod.tas_port);

                    bit<64> tas_diff = (bit<64>)ig_intr_md.ingress_mac_tstamp - previous_tas_ts;
                    //tas_diff = tas_diff - ig_md.hyperperiod.duration;
                    ig_md.hyperperiod.tas_diff = (bit<32>)tas_diff;

                    ig_md.time_series_index_tas_diff = get_and_increment_next_time_series_index_tas_diff.execute(0);
                    set_tas_diff.execute(ig_md.time_series_index_tas_diff);

                }
                // Drop Packet, its work is done here.
                ig_md.to_be_dropped = 0x1;
        } else {
            if (hdr.timer.isValid()){
                // TAS Control Traffic
                batch_id_to_port.apply();

                // Read the last timestamp from 48-bit register of TAS control traffic and write it into metadata
                ig_md.hyperperiod_ts[31:0] = tas_get_lower_last_ts.execute(ig_md.hyperperiod.tas_port);
                ig_md.hyperperiod_ts[47:32] = tas_get_higher_last_ts.execute(ig_md.hyperperiod.tas_port);
            } else {
                // Stream Identification for either DetNet-to-TSN, TSN-to-DetNet, or pure TSN
                stream_identification_c.apply(hdr, ig_md, ig_tm_md, ig_intr_md, ig_dprsr_md);

                // Read the last timestamp from 48-bit register and write it into metadata
                ig_md.hyperperiod_ts[31:0] = get_lower_last_ts.execute((bit<16>)ig_md.stream_filter.stream_gate_id);
                ig_md.hyperperiod_ts[47:32] = get_higher_last_ts.execute((bit<16>)ig_md.stream_filter.stream_gate_id);
            }

            // Calculate relative position in Period
            calc_diff_ts();

            // Do Delta adjustment
            delta_adjustment_c.apply(hdr, ig_md, ig_tm_md, ig_intr_md, ig_dprsr_md);

            if (hdr.timer.isValid()){
                // We only get here for app_id = 0, which is a TAS control packet

                hdr.gcl_time.setValid();
                hdr.gcl_time.diff_ts = (bit<64>)ig_md.diff_ts;
                // Recirculation port is set to identify packet in egress, but packet is never recirculated! It is dropped in egress.
                // TODO do this with a custom ether type
                mapping_tas_control_recirculation_port.apply();
            } else {

                // PSFP stream gate
                stream_gate_c.apply(hdr, ig_md, ig_tm_md, ig_intr_md, ig_dprsr_md);

                // PSFP Flow meter
                flow_meter_c.apply(hdr, ig_md, ig_tm_md, ig_intr_md, ig_dprsr_md);

                if (ig_md.to_be_dropped == 1){
                    ig_dprsr_md.drop_ctl = 0x3;
                }
            }
        }   
    }
}