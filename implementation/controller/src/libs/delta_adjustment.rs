use std::vec;

use rbfrt::table;
use rbfrt::table::{MatchValue, Request};

use crate::libs::const_definitions::*;
use crate::libs::types::*;

impl DeltaAdjustment {
    pub fn init_underflow_detection_table() -> Vec<Request> {
        let mut table_requests = vec![];

        let req = table::Request::new("ingress.tsn_c.delta_adjustment_c.underflow_detection")
            .match_key("ig_md.diff_ts", MatchValue::ternary(0, MASK_MAX_UNDERFLOW))
            .match_key("$MATCH_PRIORITY", MatchValue::exact(0))
            .action("ingress.tsn_c.delta_adjustment_c.nop");
        table_requests.push(req);

        let req = table::Request::new("ingress.tsn_c.delta_adjustment_c.underflow_detection")
            .match_key(
                "ig_md.diff_ts",
                MatchValue::ternary(
                    MASK_INTERVAL_SWITCH_UNDERFLOW,
                    MASK_INTERVAL_SWITCH_UNDERFLOW,
                ),
            )
            .match_key("$MATCH_PRIORITY", MatchValue::exact(1))
            .action("ingress.tsn_c.delta_adjustment_c.reset_diff_ts");
        table_requests.push(req);

        table_requests
    }

    pub fn init_hyperperiod_exceeded_detection_table(hyperperiod: u64) -> Vec<table::Request> {
        const N: u32 = 48; // bit width
                           // All-ones mask for N bits
        let all: u64 = (1u64 << N) - 1;

        // Work with the lower N bits of hyperperiod only
        let t: u64 = hyperperiod & all;

        let mut table_requests: Vec<table::Request> = Vec::new();

        // Iterate i = N-1 .. 0
        for i in (0..N).rev() {
            let bit_i: u64 = 1u64 << i;

            // Only create an entry when T's bit i is 0
            if (t & bit_i) == 0 {
                // mask_high: ones above i (zero out lower i+1 bits)
                let mask_high: u64 = all & !((bit_i << 1) - 1);

                // value: T's prefix above i, but force bit i to 1
                let value: u64 = (t & mask_high) | bit_i;

                // mask: must match T's prefix above i and force bit i
                let mask: u64 = mask_high | bit_i;

                let req = table::Request::new(
                    "ingress.tsn_c.delta_adjustment_c.hyperperiod_exceeded_detection",
                )
                .match_key("ig_md.diff_ts", MatchValue::ternary(value, mask))
                .match_key("ig_md.hyperperiod.duration", MatchValue::exact(hyperperiod))
                // Use i as a deterministic priority (adjust if your target expects inverse)
                .match_key("$MATCH_PRIORITY", MatchValue::exact(i))
                .action("ingress.tsn_c.delta_adjustment_c.reset_diff_ts2")
                .action_data("hyperperiod_max", hyperperiod);

                table_requests.push(req);
            }
        }

        table_requests
    }
}
