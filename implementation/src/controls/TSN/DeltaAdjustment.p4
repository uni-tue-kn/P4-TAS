control DeltaAdjustment(inout header_t hdr, 
            inout ingress_metadata_t ig_md, 
            inout ingress_intrinsic_metadata_for_tm_t ig_tm_md, 
            in ingress_intrinsic_metadata_t ig_intr_md, 
            inout ingress_intrinsic_metadata_for_deparser_t ig_dprsr_md) {        
        
        DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_counter1;
        DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_counter2;
        DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_counter3;



        // Underflow handling
        action calculate_underflow_timestamp(){
            ig_md.diff_ts = ig_md.difference_max_to_hyperperiod + ig_intr_md.ingress_mac_tstamp;
        }

        // Underflow handling
        action reset_diff_ts(){
            ig_md.diff_ts = 0;
            debug_counter1.count();
        }

        action nop(){
            debug_counter1.count();
        }

        /*
        This table detects an underflow of the relative position in hyperperiod.
        It has a very large value if an underflow happened.
        This table performs a maximum comparison operation by applying a ternary mask
        e.g. mask:1111111100000000000000 result:0
        if it has a result > 0 (i.e. a table miss), it means that the value was very large
        */
        table underflow_detection {
            key = {
                ig_md.diff_ts: ternary;
            }
            actions = {
                nop;
                reset_diff_ts;
            }
            size = 8;
            counters = debug_counter1;
        }

        action reset_diff_ts2(bit<48> hyperperiod_max){
            ig_md.diff_ts = hyperperiod_max;
            debug_counter2.count();
        }

        table hyperperiod_exceeded_detection {
            key = {
                ig_md.diff_ts: ternary;
                ig_md.hyperperiod.duration: exact;
            }
            actions = {
                reset_diff_ts2;
            }
            size = 1024;
            counters = debug_counter2;
        }        

        apply {
            if (underflow_detection.apply().miss){
                // We have an underflow by subtracting ingress ts from hyperperiod ts
                ig_md.difference_max_to_hyperperiod = MAXIMUM_48_BIT_TS - ig_md.hyperperiod_ts;
                calculate_underflow_timestamp();
            }
            hyperperiod_exceeded_detection.apply();
    }
}