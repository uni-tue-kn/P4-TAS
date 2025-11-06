/*******************************************************************************
 * BAREFOOT NETWORKS CONFIDENTIAL & PROPRIETARY
 *
 * Copyright (c) 2018-2019 Barefoot Networks, Inc.
 * All Rights Reserved.
 *
 * NOTICE: All information contained herein is, and remains the property of
 * Barefoot Networks, Inc. and its suppliers, if any. The intellectual and
 * technical concepts contained herein are proprietary to Barefoot Networks,
 * Inc.
 * and its suppliers and may be covered by U.S. and Foreign Patents, patents in
 * process, and are protected by trade secret or copyright law.
 * Dissemination of this information or reproduction of this material is
 * strictly forbidden unless prior written permission is obtained from
 * Barefoot Networks, Inc.
 *
 * No warranty, explicit or implicit is provided, unless granted under a
 * written agreement with Barefoot Networks, Inc.
 *
 *
 ******************************************************************************/


parser TofinoIngressParser(packet_in pkt,
                            out ingress_intrinsic_metadata_t ig_intr_md) {

    state start {
        pkt.extract(ig_intr_md);
        transition select(ig_intr_md.resubmit_flag) {
            1 : parse_resubmit;
            0 : parse_port_metadata;
        }
    }

    state parse_resubmit {
        // Parse resubmitted packet here. Not needed
        transition accept;
    }

    state parse_port_metadata {
        // Advance: Skip over port metadata if you do not wish to use it
        #if __TARGET_TOFINO__ == 2
                pkt.advance(192);
        #else
                pkt.advance(64);
        #endif
                transition accept;
    }
}

parser TofinoEgressParser(packet_in pkt,
                            out egress_intrinsic_metadata_t eg_intr_md) {

    state start {
        pkt.extract(eg_intr_md);
        transition accept;
    }
}

// ---------------------------------------------------------------------------
// Ingress parser
// ---------------------------------------------------------------------------
parser SwitchIngressParser(
        packet_in pkt,
        out header_t hdr,
        out ingress_metadata_t ig_md,
        out ingress_intrinsic_metadata_t ig_intr_md) {

    TofinoIngressParser() tofino_parser;

    state start {
        tofino_parser.apply(pkt, ig_intr_md);
        transition select(ig_intr_md.ingress_port){
            PACKET_GEN_PORT_PIPE0: parse_pkt_gen;
            PACKET_GEN_PORT_PIPE1: parse_pkt_gen;
            PACKET_GEN_PORT_PIPE2: parse_pkt_gen;
            PACKET_GEN_PORT_PIPE3: parse_pkt_gen;
            RECIRCULATION_PORT_PIPE0: parse_pkt_gen;
            RECIRCULATION_PORT_PIPE1: parse_pkt_gen;
            RECIRCULATION_PORT_PIPE2: parse_pkt_gen;
            RECIRCULATION_PORT_PIPE3: parse_pkt_gen;    
            default : parse_ethernet;
        }
    }

    state parse_pkt_gen {
        pkt.extract(hdr.timer);
        transition accept;
    }

    state parse_ethernet {
        pkt.extract(hdr.ethernet);
        transition select(hdr.ethernet.ether_type) {
            ether_type_t.IPV4 : parse_ipv4;
            ether_type_t.ETH_802_1Q : parse_802_1q;
            ether_type_t.MPLS : parse_mpls;
            default : accept;
        }
    }

    state parse_mpls {
        pkt.extract(hdr.mpls.next);
        // Overwrite the s_label. The s_label corresponds to the label at the bottom of stack.
        ig_md.s_label = hdr.mpls.last.label;
        transition select(hdr.mpls.last.bos){
            0x0: parse_mpls;
            0x1: accept; // No need to parse any deeper

        }
    }

    state parse_ipv4 {
        pkt.extract(hdr.ipv4);
        transition select(hdr.ipv4.protocol){
            ip_type_t.UDP : parse_transport;
            ip_type_t.TCP : parse_transport;
            default : accept;
        }
    }

    state parse_transport {
        pkt.extract(hdr.transport);
        transition accept;
    }

    state parse_802_1q {
        pkt.extract(hdr.eth_802_1q);
        transition select(hdr.eth_802_1q.ether_type) {
            ether_type_t.IPV4 : parse_ipv4;
            default: accept;
        }
    }

}

// ---------------------------------------------------------------------------
// Ingress Deparser
// ---------------------------------------------------------------------------
control SwitchIngressDeparser(
        packet_out pkt,
        inout header_t hdr,
        in ingress_metadata_t ig_md,
        in ingress_intrinsic_metadata_for_deparser_t ig_dprsr_md,
        in ingress_intrinsic_metadata_t ig_intr_md) {

    Digest<digest_finished_hyperperiod_t>() digest_hyperperiod;
    Digest<digest_missed_time_slice_t>() digest_missed_slice;
    
    apply {

        if (ig_dprsr_md.digest_type == 6){
            digest_hyperperiod.pack({ig_md.hyperperiod.stream_gate_id, 
                                     hdr.timer.app_id,
                                     hdr.timer.pipe_id,
                                     ig_intr_md.ingress_mac_tstamp});
        } else if (ig_dprsr_md.digest_type == 1){
            digest_missed_slice.pack({
                ig_md.diff_ts,
                ig_md.hyperperiod_ts,
                ig_intr_md.ingress_mac_tstamp
                });
        }

        // Headers for a TAS control packet
        pkt.emit(hdr.timer);
        pkt.emit(hdr.gcl_time);

        // Headers for a normal packet
        pkt.emit(hdr.ethernet);
        pkt.emit(hdr.eth_802_1q);
        pkt.emit(hdr.mpls);
        pkt.emit(hdr.ipv4);
        pkt.emit(hdr.transport);
    }
}


// ---------------------------------------------------------------------------
// Egress parser
// ---------------------------------------------------------------------------
parser SwitchEgressParser(
        packet_in pkt,
        out header_t hdr,
        out egress_metadata_t eg_md,
        out egress_intrinsic_metadata_t eg_intr_md) {

    TofinoEgressParser() tofino_parser;

    state start {
        tofino_parser.apply(pkt, eg_intr_md);
        transition select(eg_intr_md.egress_port){
            RECIRCULATION_PORT_PIPE0: parse_pkt_gen;
            RECIRCULATION_PORT_PIPE1: parse_pkt_gen;
            RECIRCULATION_PORT_PIPE2: parse_pkt_gen;
            RECIRCULATION_PORT_PIPE3: parse_pkt_gen;            
            default : parse_ethernet;
        }
    }

    state parse_pkt_gen {
        pkt.extract(hdr.timer);
        transition parse_gcl_time;
    }

    state parse_gcl_time {
        pkt.extract(hdr.gcl_time);
        transition accept;
    }

    state parse_ethernet {
        pkt.extract(hdr.ethernet);
        transition select(hdr.ethernet.ether_type) {
            ether_type_t.ETH_802_1Q : parse_802_1q;
            ether_type_t.IPV4 : parse_ipv4;
            default : accept;
        }
    }

    state parse_802_1q {
        pkt.extract(hdr.eth_802_1q);
        transition select(hdr.eth_802_1q.ether_type) {
            ether_type_t.IPV4 : parse_ipv4;
            default: accept;
        }
    }

    state parse_ipv4 {
        pkt.extract(hdr.ipv4);
        transition accept;
    }

}

// ---------------------------------------------------------------------------
// Egress Deparser
// ---------------------------------------------------------------------------
control SwitchEgressDeparser(
        packet_out pkt,
        inout header_t hdr,
        in egress_metadata_t eg_md,
        in egress_intrinsic_metadata_for_deparser_t eg_dprsr_md) {
    Checksum() ipv4_checksum;
    apply {
        if (hdr.ipv4.isValid()){
            hdr.ipv4.hdr_checksum = ipv4_checksum.update(
                    {hdr.ipv4.version,
                    hdr.ipv4.ihl,
                    hdr.ipv4.diffserv,
                    hdr.ipv4.ecn,
                    hdr.ipv4.total_len,
                    hdr.ipv4.identification,
                    hdr.ipv4.flags,
                    hdr.ipv4.frag_offset,
                    hdr.ipv4.ttl,
                    hdr.ipv4.protocol,
                    hdr.ipv4.srcAddr,
                    hdr.ipv4.dstAddr});
        }
        pkt.emit(hdr.timer);
        pkt.emit(hdr.ethernet);
        pkt.emit(hdr.eth_802_1q);
        pkt.emit(hdr.mpls);
        pkt.emit(hdr.ipv4);
        pkt.emit(hdr.transport);
    }
}
