control PTP(inout header_t hdr, 
            inout ingress_metadata_t ig_md, 
            inout ingress_intrinsic_metadata_for_tm_t ig_tm_md, 
            in ingress_intrinsic_metadata_t ig_intr_md, 
            inout ingress_intrinsic_metadata_for_deparser_t ig_dprsr_md) {



    DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_counter;
    action add_ingress_ts() {
        // This needs to be done only for SYNC messages with a destined for this tofino (control plane)
        // we repurpose the GCL Time header to make the PHV allocation happy
        hdr.gcl_time.setValid();
        // Will be read by control plane later.
        // Since we shifted ig_md.ingress_timestamp left by 16 bits in ingress,
        // we need to shift it back here.
        hdr.gcl_time.diff_ts = ig_md.ingress_timestamp >> 16;
        debug_counter.count();
    }

    action write_ingress_ts_to_correction_field(){
        // This needs to be done for:
        // - Delay_Req Message with clock identity not equal to this tofino (donnie)
        // - Follow Up Messages destined for downstream (donnie)
        hdr.ptp_correction_field.cf = hdr.ptp_correction_field.cf - ig_md.ingress_timestamp;
        debug_counter.count();
    }

    table ptp {
        key = {
            hdr.ptp_1.msg_type: exact;
            hdr.ptp_2.clock_identity: exact;
        }
        actions = {
            add_ingress_ts; // only for message type 0 (Sync)
            write_ingress_ts_to_correction_field; // only for message type 8 (Follow_Up)
        }
        size = 16;
        counters = debug_counter;
    }

    apply {
        ptp.apply();
    }
}
