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

    DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_counter;
    action enable_tstamp_capture() {
            // Enable TS7 for delay_req message. Required to synchronize this tofino
            eg_intr_md_for_oport.capture_tstamp_on_tx = 1;
            debug_counter.count();
    }

    action enable_ts6(bit<8> cf_byte_offset) {
        // Enable ts6 (correctionField) calculation. Required for transparent clock
        eg_intr_md_for_oport.update_delay_on_tx = 1;
        hdr.ptp_metadata.setValid();
        hdr.ptp_metadata.cf_byte_offset = cf_byte_offset;
        // We do not have UDP. On Tofino2, this value must therefore be set to a value larger than the packet length. On Tofino1, it can be set to zero.
        hdr.ptp_metadata.udp_cksum_byte_offset = 128;
        hdr.ptp_metadata.updated_cf = hdr.ptp_correction_field.cf[63:16];
        hdr.ptp_correction_field.setInvalid();
        debug_counter.count();
    }

    action reset_cf(){
        // This packet will be sent to the control plane and is not part of the transparent clock.
        // Reset the correctionField which was pre-filled with - iTS
        hdr.ptp_correction_field.cf = 0;
        debug_counter.count();
    }

    action nop(){
        // For Delay_Resp messages which are sent to the downstream slaves, do not reset the correction field.
        debug_counter.count();
    }    

    table ptp {
        key = {
            hdr.ptp_1.msg_type: exact;
            hdr.ptp_2.clock_identity: exact;
            eg_intr_md.egress_port: exact;
        }
        actions = {
            enable_tstamp_capture;
            enable_ts6;
            reset_cf;
            nop;
        }
        size = 16;
        counters = debug_counter;
        default_action = reset_cf;
    }

    apply {
        tas_c.apply(hdr, eg_md, eg_intr_md, eg_intr_md_for_dprsr, eg_intr_from_prsr);

        if (hdr.eth_802_1q.isValid()) {
            count_priority.apply();
        }
        else if (hdr.ptp_1.isValid()) {
            ptp.apply();
        }
        
    }
}
