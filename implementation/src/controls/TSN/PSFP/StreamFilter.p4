control StreamFilter(inout header_t hdr, 
            inout ingress_metadata_t ig_md, 
            inout ingress_intrinsic_metadata_for_tm_t ig_tm_md, 
            in ingress_intrinsic_metadata_t ig_intr_md, 
            inout ingress_intrinsic_metadata_for_deparser_t ig_dprsr_md) {


    DirectCounter<bit<32>>(CounterType_t.PACKETS_AND_BYTES) stream_filter_counter;
    // Counts frames that passed SDU filter
    DirectCounter<bit<32>>(CounterType_t.PACKETS_AND_BYTES) max_sdu_filter_counter;
    // Counts frames that did not pass SDU filter
    Counter<bit<32>, bit<16>>(32, CounterType_t.PACKETS) missed_max_sdu_filter_counter;
    Counter<bit<32>, bit<16>>(512, CounterType_t.PACKETS_AND_BYTES) overall_counter;
    
    Register<bit<1>, void>(__STREAM_ID_SIZE__, 0) reg_filter_blocked;
    RegisterAction<bit<1>, bit<16>, void>(reg_filter_blocked) block_filter = {
        void apply(inout bit<1> value){
            value = 1;
        }
    };

    RegisterAction<bit<1>, bit<16>, bit<1>>(reg_filter_blocked) get_filter_state = {
        void apply(inout bit<1> value, out bit<1> read_value){
            read_value = value;
        }
    };


    action none() {
        max_sdu_filter_counter.count();
    }

    /*
    Keep SDU Filter table as separate instance, else we can not distinguish 
    if the packet does not have a stream_handle or gets rejected because of max SDU size
    */
    table max_sdu_filter {
        key = {
            ig_md.stream_filter.stream_handle: exact;
            hdr.eth_802_1q.pcp: ternary;
            hdr.recirc.pkt_len: range;
        }
        actions = {
            none;
        }
        counters = max_sdu_filter_counter;
        default_action = none;
        size = 512;
    }

    apply {
        if (max_sdu_filter.apply().miss) {
            if (ig_md.stream_filter.stream_blocked_due_to_oversize_frame_enable){
                // Permanently block out this stream from communicating if enabled!
                block_filter.execute(ig_md.stream_filter.stream_handle);
            }
            
            // Drop anyway because SDU exceeded
            ig_md.to_be_dropped = 0x1;
            missed_max_sdu_filter_counter.count(ig_md.stream_filter.stream_handle);
        } else {
            ig_md.stream_filter.stream_blocked_due_to_oversize_frame = get_filter_state.execute(ig_md.stream_filter.stream_handle);

            // 2. Match on assigned stream_handle --> assign stream_gate
            if (stream_filter_instance.apply().hit){
                // --> stream_handle mapping exists! Continue

                overall_counter.count(ig_md.stream_filter.flow_meter_instance_id);

                if (ig_md.stream_filter.stream_blocked_due_to_oversize_frame_enable && ig_md.stream_filter.stream_blocked_due_to_oversize_frame == 1){
                    // Stream is already permanently blocked
                    ig_md.to_be_dropped = 0x1;
                }
            }
        }
    }
}
