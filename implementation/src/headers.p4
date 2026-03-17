// Preprocessing stuff
#ifndef __STREAM_ID_SIZE__
#define __STREAM_ID_SIZE__ 2048
#endif
#ifndef __STREAM_ID__
#define __STREAM_ID__ 3
#endif
#ifndef __STREAM_GATE_SIZE__
#define __STREAM_GATE_SIZE__ 2048
#endif


#ifndef _HEADERS_
#define _HEADERS_

typedef bit<48> mac_addr_t;
typedef bit<32> ipv4_addr_t;
typedef bit<32> reg_index_t;

const PortId_t RECIRCULATION_PORT_PIPE0 = 192; // Front Panel 8
const PortId_t RECIRCULATION_PORT_PIPE1 = 264; // Front Panel 16
const PortId_t RECIRCULATION_PORT_PIPE2 = 440; // Front Panel 24    
const PortId_t RECIRCULATION_PORT_PIPE3 = 16;  // Front Panel 32

const PortId_t PACKET_GEN_PORT_PIPE0 = 6;
const PortId_t PACKET_GEN_PORT_PIPE1 = 134;
const PortId_t PACKET_GEN_PORT_PIPE2 = 262;
const PortId_t PACKET_GEN_PORT_PIPE3 = 390;  

const bit<48> MAXIMUM_48_BIT_TS = 281474976710655;

enum bit<16> ether_type_t {
    IPV4  = 0x0800,
    MPLS  = 0x8847,
    ETH_802_1Q  = 0x8100,
    PTP = 0x88f7
}

enum bit<8> ip_type_t {
    TCP = 6,
    UDP = 17
}

header ethernet_t {
    mac_addr_t dst_addr;
    mac_addr_t src_addr;
    bit<16> ether_type;
}

header mpls_h {
    bit<20> label;
    bit<3> tc; // traffic class
    bit<1> bos; // bottom of stack
    bit<8> ttl;
}

header transport_t{
    bit<16> srcPort;
    bit<16> dstPort;
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
    bit<6> diffserv;
    bit<2> ecn;
    bit<16> total_len;
    bit<16> identification;
    bit<3> flags;
    bit<13> frag_offset;
    bit<8> ttl;
    bit<8> protocol;
    bit<16> hdr_checksum;
    ipv4_addr_t srcAddr;
    ipv4_addr_t dstAddr;
}

header ptp_1_h {
    bit<4> major_sdoid;
    bit<4> msg_type;
    bit<4> minor_version;
    bit<4> version;
    bit<16> msg_length;
    bit<8> domain_number;
    bit<8> minor;
    bit<16> flags;
}

header ptp_correction_field_h {
    /*
    bit<48> cf_ns;
    bit<16> cf_sub_ns;    
    */
    bit<64> cf;
}

header ptp_2_h {
    bit<32> msg_type_specific;
    bit<64> clock_identity;
    bit<16> source_port_id;
    bit<16> sequence_id;
    bit<8> control_bits;
    bit<8> log_message_interval;
    bit<48> timestamp;
    bit<32> timestamp_nanoseconds;
}

header ptp_ingress_ts_h {
    bit<48> ingress_ts;
}

header ptp_no_cf_h {
    bit<4> major_sdoid;
    bit<4> msg_type;
    bit<4> minor_version;
    bit<4> version;
    bit<16> msg_length;
    bit<8> domain_number;
    bit<8> minor;
    bit<16> flags;
  
    bit<32> msg_type_specific;
    bit<64> clock_identity;
    bit<16> source_port_id;
    bit<16> sequence_id;
    bit<8> control_bits;
    bit<8> log_message_interval;
    bit<48> timestamp;
    bit<32> timestamp_nanoseconds;
}

/*
This header will be recirculated from egress back to ingress
*/
header recirc_t {
    bit<16> pkt_len;
    bit<32> period_count;
}

/*
This header will be bridged only from ingress to egress before recirculation
*/
header bridge_t {
    bit<64> diff_ts;                // Relative position in hyperperiod
    bit<64> ingress_timestamp;      
    bit<64> hyperperiod_ts;         // Register value of last hyperperiod
    bit<16> ingress_port;
    bit<64> offset;
    bit<32> period_count;           // Used for OctectsExceeded param of stream gate
}
/*
This header will be recirculated from egress back to ingress. 
It is a separate header to contain the 20-bit field inside its own container
*/
header gcl_time_t {
    bit<64> diff_ts;
}

struct header_t {
    pktgen_timer_header_t timer;
    recirc_t recirc;
    bridge_t bridge;
    gcl_time_t gcl_time;
    ptp_metadata_t ptp_metadata; // Tofino-internal type
    ethernet_t ethernet;
    eth_802_1q_t eth_802_1q;
    ptp_1_h ptp_1;
    ptp_correction_field_h ptp_correction_field;
    ptp_2_h ptp_2;
    ptp_ingress_ts_h ptp_ingress_ts;
    mpls_h[16]       mpls;
    ipv4_t ipv4;
    transport_t transport;
}

struct hyperperiod_t {
    bit<48> hyperperiod_ts;                 // Value from hyperperiod register loaded in here
    bit<16> pkt_count_hyperperiod;          // Amount of packets that need to be captured until the hyperperiod TS is updated
    bit<16> pkt_count_register;
    PortId_t tas_port;
    bit<16> stream_gate_id;
    bit<64> duration;
    bit<32> tas_diff;
}

struct stream_filter_t {
    bit<16> stream_handle;
    bit<1> stream_blocked_due_to_oversize_frame;  // Max SDU exceeded, stream blocked permanently
    bool stream_blocked_due_to_oversize_frame_enable;
    bool active_stream_identification;          // Flag if header values will be overwritten on stream identification
    bit<12> stream_gate_id;
    bit<16> flow_meter_instance_id;
}

struct stream_gate_t {
    bit<4> ipv;
    bit<1> PSFPGateEnabled;
    bit<32> max_octets_interval;
    bit<32> initial_sdu;
    bool reset_octets;
    bit<32> remaining_octets;
    bit<12> interval_identifier;
    bool gate_closed_due_to_invalid_rx_enable;
    bool gate_closed_due_to_octets_exceeded_enable;
    bit<1> gate_closed;
}

struct flow_meter_t {
    bit<2> color;
    bool drop_on_yellow;
    bit<1> meter_blocked;
    bool mark_all_frames_red_enable;
    bool color_aware;                       // true means packets labeled yellow from previous bridges will not be able to be labeled back to green
    MeterColor_t pre_color;
}

struct ingress_metadata_t {
    stream_filter_t stream_filter;
    stream_gate_t stream_gate;
    flow_meter_t flow_meter;
    bit<20> match_ts; 
    hyperperiod_t hyperperiod;
    bit<48> diff_ts;
    bit<1> to_be_dropped;
    bit<3> block_reason;

    bit<48> difference_max_to_hyperperiod;
    bit<48> rel_ts_plus_offset;
    bit<48> hyperperiod_duration;
    bit<48> new_rel_pos_with_offset;
    bit<48> offset;
    bit<48> hyperperiod_minus_offset;

    bit<16> previous_ts; // Used for TAS control packets to measure time between two TAS control packets
    bit<16> diff_tas_control_ts; // Difference between two TAS control packets
    bit<32> time_series_index_tas_diff;

    bit<48> hyperperiod_ts;

    bit<20> s_label;
    bit<48> ingress_ts;

    bit<64> ingress_timestamp; // For PTP residence time correction
}

struct bytes_in_period_t {
    bit<32> period_id;
    bit<32> octects_in_this_period;
}

struct egress_metadata_t {
    bool queue_state_changed;
    bit<8> new_queue_state;
    bit<32> deq_tstamp;
    bit<32> time_series_index;
}

struct digest_finished_hyperperiod_t {
    bit<16> stream_gate_id;
    bit<4> app_id;
    bit<2> pipe_id;
    bit<48> ingress_ts;
}

struct digest_missed_time_slice_t {
    bit<48> diff_ts;
    bit<48> register_hyperperiod_ts;
    bit<48> ingress_timestamp;
}

#endif /* _HEADERS_ */