#include "controls/TSN/TAS.p4"

control egress(
        inout header_t hdr,
        inout egress_metadata_t eg_md,
        in egress_intrinsic_metadata_t eg_intr_md,
        in egress_intrinsic_metadata_from_parser_t eg_intr_from_prsr,
        inout egress_intrinsic_metadata_for_deparser_t eg_intr_md_for_dprsr,
        inout egress_intrinsic_metadata_for_output_port_t eg_intr_md_for_oport) {

    DirectCounter<bit<32>>(CounterType_t.PACKETS) prio_counter;

    // TAS
    TAS() tas_c;

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
        tas_c.apply(hdr, eg_md, eg_intr_md, eg_intr_md_for_dprsr, eg_intr_from_prsr);

        if (hdr.eth_802_1q.isValid()) {
            count_priority.apply();
        }
        
    }
}
