
#include "controls/TSN/TSN.p4"

#include "controls/TSN/ControlTrafficMeasurement.p4"


control ingress(
        inout header_t hdr,
        inout ingress_metadata_t ig_md,
        in ingress_intrinsic_metadata_t ig_intr_md,
        in ingress_intrinsic_metadata_from_parser_t ig_prsr_md,
        inout ingress_intrinsic_metadata_for_deparser_t ig_dprsr_md,
        inout ingress_intrinsic_metadata_for_tm_t ig_tm_md) {

    TSN() tsn_c;
    TASControlMeasurement() tas_control_measurement_c;

    DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_counter;


    action forward(PortId_t port) {
        ig_tm_md.ucast_egress_port = port;
        debug_counter.count();
    }

    table layer_2_forwarding {
        key = {
            hdr.ethernet.dst_addr: exact;
        }
        actions = {
            forward;
        }
        size = 1024;
        counters = debug_counter;
    }

    apply {
        //tas_control_measurement_c.apply(hdr, ig_md, ig_intr_md);

        if (hdr.mpls[0].isValid() || hdr.eth_802_1q.isValid() || hdr.timer.isValid()){
            tsn_c.apply(hdr, ig_md, ig_tm_md, ig_intr_md, ig_dprsr_md, ig_prsr_md);
        }

        layer_2_forwarding.apply();
    }
}
