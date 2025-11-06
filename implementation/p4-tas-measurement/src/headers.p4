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

#ifndef _HEADERS_
#define _HEADERS_

typedef bit<48> mac_addr_t;
typedef bit<32> ipv4_addr_t;

enum bit<16> ether_type_t {
    IPV4  = 0x0800,
    MPLS  = 0x8847,
    ETH_802_1Q  = 0x8100
}


header ethernet_h {
    mac_addr_t dst_addr;
    mac_addr_t src_addr;
    bit<16> ether_type;
}

header eth_802_1q_t {
    // ether_type from upper ethernet header is 0x8100 and first two bytes (already parsed in ethernet)
    bit<3> pcp; // Priority Code Point
    bit<1> dei; // Drop Eligible Indicator
    bit<12> vid; // VLAN indicator
    bit<16> ether_type;
}

header ipv4_t {
    bit<4> version;
    bit<4> ihl;
    bit<8> diffserv;
    bit<16> total_len;
    bit<16> identification;
    bit<3> flags;
    bit<13> frag_offset;
    bit<8> ttl;
    bit<8> protocol;
    bit<16> hdr_checksum;
    ipv4_addr_t src_addr;
    ipv4_addr_t dst_addr;
}

struct header_t {
    ethernet_h ethernet;
    eth_802_1q_t eth_802_1q;
    ipv4_t ipv4;
}


struct ingress_metadata_t {
    bit<8> new_active_queue;  // Value from 0 to 7
    bit<32> time_series_index;
    bool active_queue_has_changed;
    bit<48> previous_timestamp;
}

struct egress_metadata_t {
}


#endif /* _HEADERS_ */
