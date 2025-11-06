control IP(inout header_t hdr, inout ingress_metadata_t ig_md, inout ingress_intrinsic_metadata_for_tm_t ig_tm_md, in ingress_intrinsic_metadata_t ig_intr_md) {

    DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_counter;
    action ipv4_forward(PortId_t port) {
        // Set output port from control plane
        ig_tm_md.ucast_egress_port = port;

        // Decrement TTL
        hdr.ipv4.ttl = hdr.ipv4.ttl - 1;
        debug_counter.count();
    }

    table ip_forward {
        key = {
            hdr.ipv4.dst_addr: lpm;
        }
        actions = {
            ipv4_forward;
        }
        size = 1024;
        counters = debug_counter;        
    }


    apply {
        // Apply IPv4 Forwarding
        if (hdr.ipv4.isValid()) {
            ip_forward.apply();
        }
    }
}
