control StreamIdentification(inout header_t hdr, 
            inout ingress_metadata_t ig_md, 
            inout ingress_intrinsic_metadata_for_tm_t ig_tm_md, 
            in ingress_intrinsic_metadata_t ig_intr_md, 
            inout ingress_intrinsic_metadata_for_deparser_t ig_dprsr_md) {


    DirectCounter<bit<32>>(CounterType_t.PACKETS_AND_BYTES) stream_id_counter;

    action assign_stream_handle_overwrite_mac(bit<16> stream_handle, 
                                bool stream_blocked_due_to_oversize_frame_enable, mac_addr_t eth_dst_addr,
                                bit<12> stream_gate_id, 
                                bit<16> flow_meter_instance_id,
                                bool gate_closed_due_to_invalid_rx_enable,
                                bool gate_closed_due_to_octets_exceeded_enable,
                                bool dropOnYellow, bool markAllFramesRedEnable, bool colorAware,
                                bit<64> hyperperiod) {

        // Active ID
        hdr.ethernet.dst_addr = eth_dst_addr;

        // Write config
        ig_md.stream_filter.stream_handle = stream_handle;
        ig_md.stream_filter.stream_blocked_due_to_oversize_frame_enable = stream_blocked_due_to_oversize_frame_enable;
        ig_md.stream_filter.stream_gate_id = stream_gate_id;
        ig_md.stream_filter.flow_meter_instance_id = flow_meter_instance_id;
        ig_md.stream_gate.gate_closed_due_to_invalid_rx_enable = gate_closed_due_to_invalid_rx_enable;
        ig_md.stream_gate.gate_closed_due_to_octets_exceeded_enable = gate_closed_due_to_octets_exceeded_enable;
        ig_md.hyperperiod.duration = hyperperiod;
        ig_md.flow_meter.drop_on_yellow = dropOnYellow;
        ig_md.flow_meter.mark_all_frames_red_enable = markAllFramesRedEnable;
        ig_md.flow_meter.color_aware = colorAware;

        stream_id_counter.count();
    }

    action assign_stream_handle_overwrite_mac_and_pcp(bit<16> stream_handle, 
                                bool stream_blocked_due_to_oversize_frame_enable, mac_addr_t eth_dst_addr, bit<3> pcp,
                                bit<12> stream_gate_id, 
                                bit<16> flow_meter_instance_id,
                                bool gate_closed_due_to_invalid_rx_enable,
                                bool gate_closed_due_to_octets_exceeded_enable,
                                bool dropOnYellow, bool markAllFramesRedEnable, bool colorAware,
                                bit<64> hyperperiod) {

        // Active ID
        hdr.ethernet.dst_addr = eth_dst_addr;
        hdr.eth_802_1q.pcp = pcp;

        // Write config
        ig_md.stream_filter.stream_handle = stream_handle;
        ig_md.stream_filter.stream_blocked_due_to_oversize_frame_enable = stream_blocked_due_to_oversize_frame_enable;
        ig_md.stream_filter.stream_gate_id = stream_gate_id;
        ig_md.stream_filter.flow_meter_instance_id = flow_meter_instance_id;
        ig_md.stream_gate.gate_closed_due_to_invalid_rx_enable = gate_closed_due_to_invalid_rx_enable;
        ig_md.stream_gate.gate_closed_due_to_octets_exceeded_enable = gate_closed_due_to_octets_exceeded_enable;
        ig_md.hyperperiod.duration = hyperperiod;
        ig_md.flow_meter.drop_on_yellow = dropOnYellow;
        ig_md.flow_meter.mark_all_frames_red_enable = markAllFramesRedEnable;
        ig_md.flow_meter.color_aware = colorAware;

        stream_id_counter.count();
    }

    action assign_stream_handle_overwrite_mac_push_vlan(bit<16> stream_handle, 
                                bool stream_blocked_due_to_oversize_frame_enable, mac_addr_t eth_dst_addr, bit<3> pcp, bit<12> vid,
                                bit<12> stream_gate_id, 
                                bit<16> flow_meter_instance_id,
                                bool gate_closed_due_to_invalid_rx_enable,
                                bool gate_closed_due_to_octets_exceeded_enable,
                                bool dropOnYellow, bool markAllFramesRedEnable, bool colorAware,
                                bit<64> hyperperiod) {

        // Active ID
        hdr.ethernet.dst_addr = eth_dst_addr;
        hdr.ethernet.ether_type = ether_type_t.ETH_802_1Q;
        hdr.eth_802_1q.setValid();
        hdr.eth_802_1q.vid = vid;
        hdr.eth_802_1q.ether_type = hdr.ethernet.ether_type;

        // Write config
        ig_md.hyperperiod.duration = hyperperiod;
        ig_md.stream_filter.stream_handle = stream_handle;
        ig_md.stream_filter.stream_blocked_due_to_oversize_frame_enable = stream_blocked_due_to_oversize_frame_enable;
        ig_md.stream_filter.stream_gate_id = stream_gate_id;
        ig_md.stream_filter.flow_meter_instance_id = flow_meter_instance_id;
        ig_md.stream_gate.gate_closed_due_to_invalid_rx_enable = gate_closed_due_to_invalid_rx_enable;
        ig_md.stream_gate.gate_closed_due_to_octets_exceeded_enable = gate_closed_due_to_octets_exceeded_enable;
        ig_md.flow_meter.drop_on_yellow = dropOnYellow;
        ig_md.flow_meter.mark_all_frames_red_enable = markAllFramesRedEnable;
        ig_md.flow_meter.color_aware = colorAware;


        stream_id_counter.count();
    } 

    action assign_stream_handle_overwrite_mac_remove_vlan(bit<16> stream_handle, 
                                bool stream_blocked_due_to_oversize_frame_enable, mac_addr_t eth_dst_addr, bit<3> pcp, bit<12> vid,
                                bit<12> stream_gate_id, 
                                bit<16> flow_meter_instance_id,
                                bool gate_closed_due_to_invalid_rx_enable,
                                bool gate_closed_due_to_octets_exceeded_enable,
                                bool dropOnYellow, bool markAllFramesRedEnable, bool colorAware,
                                bit<64> hyperperiod) {

        // Active ID, remove VLAN tag
        hdr.ethernet.dst_addr = eth_dst_addr;
        hdr.ethernet.ether_type = hdr.eth_802_1q.ether_type;;
        hdr.eth_802_1q.setInvalid();

        // Write config
        ig_md.hyperperiod.duration = hyperperiod;
        ig_md.stream_filter.stream_handle = stream_handle;
        ig_md.stream_filter.stream_blocked_due_to_oversize_frame_enable = stream_blocked_due_to_oversize_frame_enable;
        ig_md.stream_filter.stream_gate_id = stream_gate_id;
        ig_md.stream_filter.flow_meter_instance_id = flow_meter_instance_id;
        ig_md.stream_gate.gate_closed_due_to_invalid_rx_enable = gate_closed_due_to_invalid_rx_enable;
        ig_md.stream_gate.gate_closed_due_to_octets_exceeded_enable = gate_closed_due_to_octets_exceeded_enable;
        ig_md.flow_meter.drop_on_yellow = dropOnYellow;
        ig_md.flow_meter.mark_all_frames_red_enable = markAllFramesRedEnable;
        ig_md.flow_meter.color_aware = colorAware;

        stream_id_counter.count();
    }         

    action assign_stream_handle(bit<16> stream_handle, 
                                bool stream_blocked_due_to_oversize_frame_enable,
                                bit<12> stream_gate_id, 
                                bit<16> flow_meter_instance_id,
                                bool gate_closed_due_to_invalid_rx_enable,
                                bool gate_closed_due_to_octets_exceeded_enable,
                                bool dropOnYellow, bool markAllFramesRedEnable, bool colorAware,
                                bit<64> hyperperiod) {

        // Write config
        ig_md.stream_filter.stream_handle = stream_handle;
        ig_md.stream_filter.stream_blocked_due_to_oversize_frame_enable = stream_blocked_due_to_oversize_frame_enable;
        ig_md.stream_filter.stream_gate_id = stream_gate_id;
        ig_md.stream_filter.flow_meter_instance_id = flow_meter_instance_id;
        ig_md.stream_gate.gate_closed_due_to_invalid_rx_enable = gate_closed_due_to_invalid_rx_enable;
        ig_md.stream_gate.gate_closed_due_to_octets_exceeded_enable = gate_closed_due_to_octets_exceeded_enable;
        ig_md.hyperperiod.duration = hyperperiod;
        ig_md.flow_meter.drop_on_yellow = dropOnYellow;
        ig_md.flow_meter.mark_all_frames_red_enable = markAllFramesRedEnable;
        ig_md.flow_meter.color_aware = colorAware;

        stream_id_counter.count();
    }        

        table stream_id {
        key = {
            hdr.ethernet.dst_addr: exact;     // Null stream + active identification
            hdr.eth_802_1q.vid: exact;

            hdr.ipv4.srcAddr: ternary;          // IP stream identification
            hdr.ipv4.dstAddr: ternary;

            ig_md.s_label: ternary;        // DetNet translation
        }
        actions = {
            assign_stream_handle;
            assign_stream_handle_overwrite_mac;
            assign_stream_handle_overwrite_mac_and_pcp;
            assign_stream_handle_overwrite_mac_push_vlan;  //DetNet->TSN
            assign_stream_handle_overwrite_mac_remove_vlan; // TSN->DetNet
        }
        size = 8196;
        counters = stream_id_counter;
    }

    apply {
        // Apply TSN stream identification table
        stream_id.apply();
    }
}
