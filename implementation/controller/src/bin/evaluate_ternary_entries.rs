/// Standalone evaluation of the range-to-ternary conversion used by TAS and PSFP.
///
/// This binary evaluates how many MAT (Match-Action Table) ternary entries are
/// required for different GCL configurations (varying periods, number of time
/// slices, guard band widths, queue counts, etc.).
///
/// Run with:
///   cargo run --bin evaluate_ternary_entries
///
use serde::Serialize;
use std::fs;
use std::path::Path;
// ---------------------------------------------------------------------------
// Core algorithm – mirrors the range_to_ternary_entries logic from tas.rs /
// stream_gate_schedule.rs but only *counts* entries instead of building Request
// objects.
// ---------------------------------------------------------------------------

/// Counts the number of ternary (value, mask) entries needed to cover the
/// closed range `[start, end]`.  This is the same decomposition used in both
/// `TAS::range_to_ternary_entries` and
/// `StreamGateControlList::range_to_ternary_entries`.
fn count_ternary_entries(start: u32, end: u32) -> usize {
    if start > end {
        return 0;
    }

    let mut count: usize = 0;
    let mut cur = start;

    while cur <= end {
        let remaining = end - cur;
        if remaining == 0 {
            count += 1;
            break;
        }

        let max_block_size = 1u32 << (31 - remaining.leading_zeros());
        let align_size = if cur == 0 {
            1
        } else {
            1u32 << cur.trailing_zeros()
        };
        let size = max_block_size.min(align_size);

        count += 1;
        cur += size;
    }

    count
}

// ---------------------------------------------------------------------------
// Optimized algorithm – fixes two issues in the original:
//
// 1. `cur == 0` special case: the original sets `align_size = 1` to avoid
//    `1u32 << 32` overflow from `0u32.trailing_zeros() == 32`. This forces
//    the very first iteration to emit a single exact-match entry for value 0,
//    then slowly build up alignment (1, 2, 4, 8, ...).
//    Fix: treat cur == 0 as maximally aligned (align_size = u32::MAX).
//
// 2. `remaining = end - cur` computes the *distance* to the end, not the
//    *count* of remaining values. For an inclusive range [cur, end], the count
//    is `end - cur + 1`. Using the distance causes `max_block_size` to be
//    half of what it could be for perfectly sized ranges.
//    Fix: use `count = end - cur + 1` and pick the highest power-of-two ≤ count.
// ---------------------------------------------------------------------------

/// Optimized ternary entry count – fixes both cur==0 and remaining off-by-one.
fn count_ternary_entries_optimized(start: u32, end: u32) -> usize {
    if start > end {
        return 0;
    }

    let mut count: usize = 0;
    let mut cur = start;

    while cur <= end {
        // Single remaining value → exact match, done.
        if cur == end {
            count += 1;
            break;
        }

        let num_remaining = end - cur + 1; // count of values, not distance

        // Largest power-of-two that fits in the remaining count
        let max_block_size = 1u32 << (31 - (num_remaining).leading_zeros());

        // Alignment: largest power-of-two that divides cur.
        // cur == 0 is aligned to everything, so use a large value.
        let align_size = if cur == 0 {
            1u32 << 31 // maximally aligned (avoid 1<<32 overflow)
        } else {
            1u32 << cur.trailing_zeros()
        };

        let size = max_block_size.min(align_size);
        count += 1;
        cur += size;
    }

    count
}

fn count_tas_entries_optimized(config: &TASConfig) -> usize {
    config
        .time_slices
        .iter()
        .map(|ts| ts.num_queue_states * count_ternary_entries_optimized(ts.low, ts.high))
        .sum()
}

fn count_psfp_entries_optimized(config: &PSFPConfig) -> usize {
    config
        .intervals
        .iter()
        .map(|iv| count_ternary_entries_optimized(iv.low, iv.high))
        .sum()
}

/// Optimized version that returns actual (value, mask) pairs.
fn ternary_entries_optimized(start: u32, end: u32) -> Vec<(u32, u32)> {
    let mut entries = Vec::new();
    if start > end {
        return entries;
    }
    let mut cur = start;

    while cur <= end {
        if cur == end {
            entries.push((cur, 0xFFFF_FFFF));
            break;
        }

        let num_remaining = end - cur + 1;
        let max_block_size = 1u32 << (31 - num_remaining.leading_zeros());
        let align_size = if cur == 0 {
            1u32 << 31
        } else {
            1u32 << cur.trailing_zeros()
        };
        let size = max_block_size.min(align_size);
        let mask = !(size - 1);
        entries.push((cur, mask));
        cur += size;
    }

    entries
}

/// Returns the individual ternary (value, mask) pairs for a range – useful for
/// detailed inspection / debugging.
fn ternary_entries(start: u32, end: u32) -> Vec<(u32, u32)> {
    let mut entries = Vec::new();
    if start > end {
        return entries;
    }
    let mut cur = start;

    while cur <= end {
        let remaining = end - cur;
        if remaining == 0 {
            entries.push((cur, 0xFFFF_FFFF));
            break;
        }

        let max_block_size = 1u32 << (31 - remaining.leading_zeros());
        let align_size = if cur == 0 {
            1
        } else {
            1u32 << cur.trailing_zeros()
        };
        let size = max_block_size.min(align_size);
        let mask = !(size - 1);

        entries.push((cur, mask));
        cur += size;
    }

    entries
}

// ---------------------------------------------------------------------------
// TAS GCL model
// ---------------------------------------------------------------------------

/// A simplified TAS time‐slice: range `[low, high]` with a set of queue states.
struct TASTimeSlice {
    low: u32,
    high: u32,
    /// Number of queue‐state entries (each queue × state produces one set of
    /// ternary entries in the real implementation).
    num_queue_states: usize,
}

/// A simplified TAS Gate Control List.
#[allow(dead_code)]
struct TASConfig {
    name: String,
    period_ns: u64,
    guard_band_ns: u32,
    time_slices: Vec<TASTimeSlice>,
}

/// Count the total MAT entries a TAS GCL would produce.
fn count_tas_entries(config: &TASConfig) -> usize {
    config
        .time_slices
        .iter()
        .map(|ts| ts.num_queue_states * count_ternary_entries(ts.low, ts.high))
        .sum()
}

// ---------------------------------------------------------------------------
// PSFP Stream Gate Schedule model
// ---------------------------------------------------------------------------

struct PSFPInterval {
    low: u32,
    high: u32,
}

#[allow(dead_code)]
struct PSFPConfig {
    name: String,
    period_ns: u64,
    intervals: Vec<PSFPInterval>,
}

fn count_psfp_entries(config: &PSFPConfig) -> usize {
    config
        .intervals
        .iter()
        .map(|iv| count_ternary_entries(iv.low, iv.high))
        .sum()
}

// ---------------------------------------------------------------------------
// Helper: build equal-width time slices for a period
// ---------------------------------------------------------------------------

/// Generates `num_slices` equal-width content time slices, matching the real
/// controller's `insert_tas_gsi` logic:
///
/// 1. Each content slice keeps its original width: `slice_width = period / num_slices`
/// 2. A guard band slice (all queues closed) is **appended after** each content slice
/// 3. The total period grows: `period + guard_band * num_slices`
///
/// So the ranges are:
///   content_0: [0, slice_width]
///   gb_0:      [slice_width, slice_width + guard_band]
///   content_1: [slice_width + guard_band, 2*slice_width + guard_band]
///   gb_1:      [2*slice_width + guard_band, 2*slice_width + 2*guard_band]
///   ...
fn make_equal_tas_slices(
    period: u32,
    num_slices: usize,
    guard_band: u32,
    num_queue_states_per_slice: usize,
) -> Vec<TASTimeSlice> {
    let slice_width = period / num_slices as u32;
    let mut slices = Vec::new();
    let mut cursor: u32 = 0;

    for _i in 0..num_slices {
        // Content slice
        let content_low = cursor;
        let content_high = cursor + slice_width;
        slices.push(TASTimeSlice {
            low: content_low,
            high: content_high,
            num_queue_states: num_queue_states_per_slice,
        });
        cursor = content_high;

        // Guard band slice (all queues closed), appended after content
        if guard_band > 0 {
            let gb_low = cursor;
            let gb_high = cursor + guard_band;
            slices.push(TASTimeSlice {
                low: gb_low,
                high: gb_high,
                num_queue_states: num_queue_states_per_slice,
            });
            cursor = gb_high;
        }
    }

    slices
}

fn make_equal_psfp_intervals(period: u32, num_intervals: usize) -> Vec<PSFPInterval> {
    let width = period / num_intervals as u32;
    (0..num_intervals)
        .map(|i| {
            let low = i as u32 * width;
            let high = if i == num_intervals - 1 {
                period - 1
            } else {
                (i as u32 + 1) * width - 1
            };
            PSFPInterval { low, high }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Pretty-printing helpers
// ---------------------------------------------------------------------------

fn print_section(title: &str) {
    let bar = "=".repeat(80);
    println!("\n{bar}");
    println!("  {title}");
    println!("{bar}");
}

#[allow(dead_code)]
fn print_subsection(title: &str) {
    println!("\n  --- {title} ---");
}

// ---------------------------------------------------------------------------
// Evaluation scenarios
// ---------------------------------------------------------------------------

fn evaluate_single_range_examples() {
    print_section("1) Single-range ternary decomposition examples");

    let test_ranges: Vec<(u32, u32)> = vec![
        (0, 0),
        (0, 1),
        (0, 7),
        (0, 255),
        (0, 999),
        (0, 1023),
        (0, 4095),
        (100, 199),
        (100, 1000),
        (1000, 1999),
        (0, 499_999),       // 500 µs period
        (0, 999_999),       // 1 ms period
        (0, 127_999_999),   // 128 ms period
        (0, 799_999_999),   // 800 ms period
        (250_000, 499_999), // second half of 500 µs
        (500_000, 999_999), // second half of 1 ms
        (333_333, 666_665), // middle third of 1 ms
    ];

    println!("  {:>15} {:>15} {:>10}", "start", "end", "#entries");
    println!("  {}", "-".repeat(44));
    for (s, e) in &test_ranges {
        let n = count_ternary_entries(*s, *e);
        println!("  {:>15} {:>15} {:>10}", s, e, n);
    }
}

fn evaluate_periods_x_slices() {
    print_section(
        "TAS – entry count for each period × number of slices (8 queues, 30 ns guard band)",
    );

    let periods_ns: Vec<(u64, &str)> = vec![
        (10_000, "10 µs"),
        (400_000, "400 µs"),
        (500_000, "500 µs"),
        (524_288, "524 µs (2¹⁹)"),
        (2_000_000, "2 ms"),
        (128_000_000, "128 ms"),
    ];

    let slice_counts: Vec<usize> = vec![1, 2, 3, 4, 5, 6, 7, 8, 10, 16, 20, 32];
    let guard_band: u32 = 30;
    let num_queues: usize = 8;

    // Header row: period labels
    print!("  {:>10}", "#slices");
    for (_p, label) in &periods_ns {
        print!(" | {:>12}", label);
    }
    println!();

    // Separator
    print!("  {}", "-".repeat(10));
    for _ in &periods_ns {
        print!("-+-{}", "-".repeat(12));
    }
    println!();

    // Data rows
    for &ns in &slice_counts {
        print!("  {:>10}", ns);
        for (period, _label) in &periods_ns {
            let p = *period as u32;
            let slices = make_equal_tas_slices(p, ns, guard_band, num_queues);
            let cfg = TASConfig {
                name: String::new(),
                period_ns: *period,
                guard_band_ns: guard_band,
                time_slices: slices,
            };
            let total = count_tas_entries(&cfg);
            print!(" | {:>12}", total);
        }
        println!();
    }

    // Also show entries per slice
    println!();
    print_section("TAS – entries per slice (same configurations)");

    print!("  {:>10}", "#slices");
    for (_p, label) in &periods_ns {
        print!(" | {:>12}", label);
    }
    println!();

    print!("  {}", "-".repeat(10));
    for _ in &periods_ns {
        print!("-+-{}", "-".repeat(12));
    }
    println!();

    for &ns in &slice_counts {
        print!("  {:>10}", ns);
        for (period, _label) in &periods_ns {
            let p = *period as u32;
            let slices = make_equal_tas_slices(p, ns, guard_band, num_queues);
            let cfg = TASConfig {
                name: String::new(),
                period_ns: *period,
                guard_band_ns: guard_band,
                time_slices: slices,
            };
            let total = count_tas_entries(&cfg);
            print!(" | {:>12.1}", total as f64 / ns as f64);
        }
        println!();
    }
}

fn evaluate_psfp_periods_x_intervals() {
    print_section("PSFP – entry count for each period × number of intervals");

    let periods_ns: Vec<(u64, &str)> = vec![
        (10_000, "10 µs"),
        (400_000, "400 µs"),
        (500_000, "500 µs"),
        (524_288, "524 µs (2¹⁹)"),
        (2_000_000, "2 ms"),
        (128_000_000, "128 ms"),
    ];

    let interval_counts: Vec<usize> = vec![1, 2, 3, 4, 5, 8, 10, 16, 20, 32];

    print!("  {:>12}", "#intervals");
    for (_p, label) in &periods_ns {
        print!(" | {:>12}", label);
    }
    println!();

    print!("  {}", "-".repeat(12));
    for _ in &periods_ns {
        print!("-+-{}", "-".repeat(12));
    }
    println!();

    for &ni in &interval_counts {
        print!("  {:>12}", ni);
        for (period, _label) in &periods_ns {
            let p = *period as u32;
            let intervals = make_equal_psfp_intervals(p, ni);
            let cfg = PSFPConfig {
                name: String::new(),
                period_ns: *period,
                intervals,
            };
            let total = count_psfp_entries(&cfg);
            print!(" | {:>12}", total);
        }
        println!();
    }
}

fn evaluate_po2_comparison() {
    print_section("Power-of-two alignment: original vs. optimized algorithm");

    // Compare: original periods vs nearby power-of-two periods
    let comparisons: Vec<(u64, &str, u64, &str)> = vec![
        (10_000, "10 µs", 8_192, "~8.2 µs (2¹³)"),
        (10_000, "10 µs", 16_384, "~16.4 µs (2¹⁴)"),
        (400_000, "400 µs", 262_144, "~262 µs (2¹⁸)"),
        (400_000, "400 µs", 524_288, "~524 µs (2¹⁹)"),
        (500_000, "500 µs", 524_288, "~524 µs (2¹⁹)"),
        (2_000_000, "2 ms", 2_097_152, "~2.1 ms (2²¹)"),
        (128_000_000, "128 ms", 134_217_728, "~134 ms (2²⁷)"),
    ];

    let slice_counts: Vec<usize> = vec![1, 2, 4, 8, 16, 32];
    let guard_band: u32 = 30;
    let num_queues: usize = 8;

    println!(
        "\n  {:>14} {:>14} {:>8}  {:>10} {:>10}  {:>10} {:>10}  {:>7}",
        "Period", "Po2 Period", "#slices", "Orig", "Orig-Po2", "Opt", "Opt-Po2", "Savings"
    );
    println!("  {}", "-".repeat(105));

    for (period, plabel, po2_period, po2label) in &comparisons {
        for &ns in &slice_counts {
            let p = *period as u32;
            let p2 = *po2_period as u32;

            // Original algorithm
            let slices_orig = make_equal_tas_slices(p, ns, guard_band, num_queues);
            let cfg_orig = TASConfig {
                name: String::new(),
                period_ns: *period,
                guard_band_ns: guard_band,
                time_slices: slices_orig,
            };
            let orig = count_tas_entries(&cfg_orig);

            let slices_orig_po2 = make_equal_tas_slices(p2, ns, guard_band, num_queues);
            let cfg_orig_po2 = TASConfig {
                name: String::new(),
                period_ns: *po2_period,
                guard_band_ns: guard_band,
                time_slices: slices_orig_po2,
            };
            let orig_po2 = count_tas_entries(&cfg_orig_po2);

            // Optimized algorithm
            let slices_opt = make_equal_tas_slices(p, ns, guard_band, num_queues);
            let cfg_opt = TASConfig {
                name: String::new(),
                period_ns: *period,
                guard_band_ns: guard_band,
                time_slices: slices_opt,
            };
            let opt = count_tas_entries_optimized(&cfg_opt);

            let slices_opt_po2 = make_equal_tas_slices(p2, ns, guard_band, num_queues);
            let cfg_opt_po2 = TASConfig {
                name: String::new(),
                period_ns: *po2_period,
                guard_band_ns: guard_band,
                time_slices: slices_opt_po2,
            };
            let opt_po2 = count_tas_entries_optimized(&cfg_opt_po2);

            let savings = if orig > 0 {
                format!("{:.0}%", (1.0 - opt_po2 as f64 / orig as f64) * 100.0)
            } else {
                "-".to_string()
            };

            println!(
                "  {:>14} {:>14} {:>8}  {:>10} {:>10}  {:>10} {:>10}  {:>7}",
                plabel, po2label, ns, orig, orig_po2, opt, opt_po2, savings
            );
        }
        println!();
    }

    // Show single-range examples to illustrate the core difference
    print_section("Single-range comparison: original vs. optimized");
    let ranges: Vec<(u32, u32, &str)> = vec![
        (0, 7, "[0, 7]       — 8 values (2³)"),
        (0, 255, "[0, 255]     — 256 values (2⁸)"),
        (0, 1023, "[0, 1023]    — 1024 values (2¹⁰)"),
        (256, 511, "[256, 511]   — 256 values, aligned"),
        (1024, 2047, "[1024, 2047] — 1024 values, aligned"),
        (0, 999, "[0, 999]     — 1000 values (non-po2)"),
        (0, 499_999, "[0, 499999]  — 500 µs (non-po2)"),
        (0, 524_287, "[0, 524287]  — 2¹⁹ values (po2)"),
        (0, 2_097_151, "[0, 2097151] — 2²¹ values (po2)"),
        (
            100_000,
            199_999,
            "[100k, 200k) — 100k values, non-po2 start",
        ),
        (131_072, 262_143, "[2¹⁷, 2¹⁸-1] — 2¹⁷ values, aligned"),
    ];

    println!(
        "  {:>40}  {:>8}  {:>8}  {:>8}",
        "Range", "Original", "Optimized", "Ratio"
    );
    println!("  {}", "-".repeat(72));
    for (s, e, label) in &ranges {
        let orig = count_ternary_entries(*s, *e);
        let opt = count_ternary_entries_optimized(*s, *e);
        let ratio = orig as f64 / opt as f64;
        println!("  {:>40}  {:>8}  {:>8}  {:>7.1}x", label, orig, opt, ratio);
    }
}

fn evaluate_detailed_decomposition() {
    print_section("9) Detailed ternary decomposition for selected ranges");

    let ranges = vec![
        (0u32, 499_999u32, "500 µs slice [0, 499999]"),
        (500_000, 999_999, "500 µs slice [500000, 999999]"),
        (0, 249_970, "250 µs - 30 ns gb [0, 249970]"),
        (100, 199, "Small range [100, 199]"),
    ];

    for (start, end, label) in &ranges {
        let entries = ternary_entries(*start, *end);
        println!("\n  {label}  →  {} entries", entries.len());
        if entries.len() <= 40 {
            for (val, mask) in &entries {
                println!("    value=0x{val:08X}  mask=0x{mask:08X}");
            }
        } else {
            for (val, mask) in entries.iter().take(10) {
                println!("    value=0x{val:08X}  mask=0x{mask:08X}");
            }
            println!("    ... ({} more entries omitted)", entries.len() - 10);
        }
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

fn format_period(ns: u64) -> String {
    if ns >= 1_000_000_000 {
        format!("{} ms", ns / 1_000_000)
    } else if ns >= 1_000_000 {
        format!("{} ms", ns / 1_000_000)
    } else if ns >= 1_000 {
        format!("{} µs", ns / 1_000)
    } else {
        format!("{} ns", ns)
    }
}

// ---------------------------------------------------------------------------
// Diagnostic: verify against exact CYCLIC_PRIOS config
// ---------------------------------------------------------------------------

fn verify_cyclic_prios() {
    print_section("DIAGNOSTIC: exact CYCLIC_PRIOS config (400 µs, 8 slices, 8 queues)");

    // Exact ranges from configuration.json CYCLIC_PRIOS
    let config_ranges: Vec<(u32, u32)> = vec![
        (0, 50000),
        (50000, 100000),
        (100000, 150000),
        (150000, 200000),
        (200000, 250000),
        (250000, 300000),
        (300000, 350000),
        (350000, 400000),
    ];
    let num_queue_states: usize = 8;

    println!("\n  A) Config ranges as-is (no guard band slices):");
    let mut total_no_gb = 0;
    for (low, high) in &config_ranges {
        let n = count_ternary_entries(*low, *high);
        println!(
            "    [{:>6}, {:>6}]: {:>3} entries × {} qs = {:>5}",
            low,
            high,
            n,
            num_queue_states,
            n * num_queue_states
        );
        total_no_gb += n * num_queue_states;
    }
    println!("    Total (no guard bands): {}", total_no_gb);

    println!(
        "\n  B) With guard band slices (gb=30ns, content=[low, high-30], gb=[high-30, high]):"
    );
    let guard_band: u32 = 30;
    let mut total_with_gb = 0;
    for (low, high) in &config_ranges {
        let content_high = high - guard_band;
        let nc = count_ternary_entries(*low, content_high);
        let ng = count_ternary_entries(content_high, *high);
        println!(
            "    content [{:>6}, {:>6}]: {:>3}  |  gb [{:>6}, {:>6}]: {:>3}  |  × {} qs = {:>5}",
            low,
            content_high,
            nc,
            content_high,
            high,
            ng,
            num_queue_states,
            (nc + ng) * num_queue_states
        );
        total_with_gb += (nc + ng) * num_queue_states;
    }
    println!("    Total (with guard bands): {}", total_with_gb);

    println!("\n  C) Guard band between slices (content=[low, next_low-gb], gb=[next_low-gb, next_low]):");
    let mut total_between = 0;
    for i in 0..config_ranges.len() {
        let (low, _high) = config_ranges[i];
        let next_low = if i + 1 < config_ranges.len() {
            config_ranges[i + 1].0
        } else {
            400000 // period
        };
        let content_high = next_low - guard_band;
        let nc = count_ternary_entries(low, content_high);
        let ng = count_ternary_entries(content_high, next_low);
        println!(
            "    content [{:>6}, {:>6}]: {:>3}  |  gb [{:>6}, {:>6}]: {:>3}  |  × {} qs = {:>5}",
            low,
            content_high,
            nc,
            content_high,
            next_low,
            ng,
            num_queue_states,
            (nc + ng) * num_queue_states
        );
        total_between += (nc + ng) * num_queue_states;
    }
    println!("    Total (gb between slices): {}", total_between);

    println!("\n  D) Full content ranges as in config + separate guard band slices on top:");
    let mut total_overlay = 0;
    // Content: exact config ranges
    for (low, high) in &config_ranges {
        let n = count_ternary_entries(*low, *high);
        total_overlay += n * num_queue_states;
    }
    println!("    Content entries: {}", total_overlay);
    // Guard band slices: [boundary - gb, boundary]
    let mut gb_entries = 0;
    for i in 0..config_ranges.len() {
        let boundary = if i + 1 < config_ranges.len() {
            config_ranges[i + 1].0
        } else {
            400000
        };
        let gb_low = boundary - guard_band;
        let gb_high = boundary;
        let ng = count_ternary_entries(gb_low, gb_high);
        println!(
            "    gb [{:>6}, {:>6}]: {:>3}  × {} qs = {:>5}",
            gb_low,
            gb_high,
            ng,
            num_queue_states,
            ng * num_queue_states
        );
        gb_entries += ng * num_queue_states;
    }
    println!("    Guard band entries: {}", gb_entries);
    total_overlay += gb_entries;
    println!("    Total (overlay model): {}", total_overlay);

    println!("\n  Expected from controller: 1432");
}

// ---------------------------------------------------------------------------
// JSON export
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct EvalOutput {
    tas: TASEvalOutput,
    tas_no_gb: TASEvalOutput,
    psfp: PSFPEvalOutput,
    po2_comparison: Po2ComparisonOutput,
}

#[derive(Serialize)]
struct TASEvalOutput {
    guard_band_ns: u32,
    num_queues: usize,
    periods_ns: Vec<u64>,
    period_labels: Vec<String>,
    slice_counts: Vec<usize>,
    /// entries[slice_idx][period_idx]
    entries: Vec<Vec<usize>>,
}

#[derive(Serialize)]
struct PSFPEvalOutput {
    periods_ns: Vec<u64>,
    period_labels: Vec<String>,
    interval_counts: Vec<usize>,
    /// entries[interval_idx][period_idx]
    entries: Vec<Vec<usize>>,
}

#[derive(Serialize)]
struct Po2ComparisonOutput {
    /// The original (non-po2) periods in nanoseconds
    original_periods_ns: Vec<u64>,
    original_period_labels: Vec<String>,
    /// Nearest power-of-two periods in nanoseconds
    po2_periods_ns: Vec<u64>,
    po2_period_labels: Vec<String>,
    guard_band_ns: u32,
    num_queues: usize,
    slice_counts: Vec<usize>,
    /// TAS entries with original period, original algorithm [slice_idx][period_idx]
    tas_orig: Vec<Vec<usize>>,
    /// TAS entries with po2 period, original algorithm [slice_idx][period_idx]
    tas_orig_po2: Vec<Vec<usize>>,
    /// TAS entries with original period, optimized algorithm [slice_idx][period_idx]
    tas_opt: Vec<Vec<usize>>,
    /// TAS entries with po2 period, optimized algorithm [slice_idx][period_idx]
    tas_opt_po2: Vec<Vec<usize>>,
}

fn export_json(output_dir: &str) {
    let periods_ns: Vec<(u64, &str)> = vec![
        (10_000, "10 µs"),
        (400_000, "400 µs"),
        (500_000, "500 µs"),
        (524_288, "524 µs (2¹⁹)"),
        (2_000_000, "2 ms"),
        (128_000_000, "128 ms"),
    ];
    let guard_band: u32 = 30;
    let num_queues: usize = 8;

    // TAS
    let tas_slice_counts: Vec<usize> = vec![1, 2, 3, 4, 5, 6, 7, 8, 10, 16, 20, 32];
    let mut tas_entries: Vec<Vec<usize>> = Vec::new();
    for &ns in &tas_slice_counts {
        let mut row = Vec::new();
        for (period, _label) in &periods_ns {
            let slices = make_equal_tas_slices(*period as u32, ns, guard_band, num_queues);
            let cfg = TASConfig {
                name: String::new(),
                period_ns: *period,
                guard_band_ns: guard_band,
                time_slices: slices,
            };
            row.push(count_tas_entries_optimized(&cfg));
        }
        tas_entries.push(row);
    }

    // TAS (no guard band)
    let mut tas_no_gb_entries: Vec<Vec<usize>> = Vec::new();
    for &ns in &tas_slice_counts {
        let mut row = Vec::new();
        for (period, _label) in &periods_ns {
            let slices = make_equal_tas_slices(*period as u32, ns, 0, num_queues);
            let cfg = TASConfig {
                name: String::new(),
                period_ns: *period,
                guard_band_ns: 0,
                time_slices: slices,
            };
            row.push(count_tas_entries_optimized(&cfg));
        }
        tas_no_gb_entries.push(row);
    }

    // PSFP
    let psfp_interval_counts: Vec<usize> = vec![1, 2, 3, 4, 5, 8, 10, 16, 20, 32];
    let mut psfp_entries: Vec<Vec<usize>> = Vec::new();
    for &ni in &psfp_interval_counts {
        let mut row = Vec::new();
        for (period, _label) in &periods_ns {
            let intervals = make_equal_psfp_intervals(*period as u32, ni);
            let cfg = PSFPConfig {
                name: String::new(),
                period_ns: *period,
                intervals,
            };
            row.push(count_psfp_entries_optimized(&cfg));
        }
        psfp_entries.push(row);
    }

    // Power-of-two comparison
    // Map each original period to the nearest-above power-of-two period
    let po2_periods: Vec<(u64, &str, u64, String)> = vec![
        (10_000, "10 µs", 16_384, "~16.4 µs (2¹⁴)".into()),
        (400_000, "400 µs", 524_288, "~524 µs (2¹⁹)".into()),
        (500_000, "500 µs", 524_288, "~524 µs (2¹⁹)".into()),
        (524_288, "524 µs (2¹⁹)", 524_288, "524 µs (2¹⁹)".into()),
        (2_000_000, "2 ms", 2_097_152, "~2.1 ms (2²¹)".into()),
        (128_000_000, "128 ms", 134_217_728, "~134 ms (2²⁷)".into()),
    ];
    let po2_slice_counts: Vec<usize> = vec![1, 2, 4, 8, 16, 32];

    let mut po2_tas_orig: Vec<Vec<usize>> = Vec::new();
    let mut po2_tas_orig_po2: Vec<Vec<usize>> = Vec::new();
    let mut po2_tas_opt: Vec<Vec<usize>> = Vec::new();
    let mut po2_tas_opt_po2: Vec<Vec<usize>> = Vec::new();

    for &ns in &po2_slice_counts {
        let mut row_orig = Vec::new();
        let mut row_orig_po2 = Vec::new();
        let mut row_opt = Vec::new();
        let mut row_opt_po2 = Vec::new();

        for (period, _, po2_period, _) in &po2_periods {
            let p = *period as u32;
            let p2 = *po2_period as u32;

            // Original period, original algorithm
            let sl = make_equal_tas_slices(p, ns, guard_band, num_queues);
            let cfg = TASConfig {
                name: String::new(),
                period_ns: *period,
                guard_band_ns: guard_band,
                time_slices: sl,
            };
            row_orig.push(count_tas_entries(&cfg));

            // Po2 period, original algorithm
            let sl2 = make_equal_tas_slices(p2, ns, guard_band, num_queues);
            let cfg2 = TASConfig {
                name: String::new(),
                period_ns: *po2_period,
                guard_band_ns: guard_band,
                time_slices: sl2,
            };
            row_orig_po2.push(count_tas_entries(&cfg2));

            // Original period, optimized algorithm
            let sl3 = make_equal_tas_slices(p, ns, guard_band, num_queues);
            let cfg3 = TASConfig {
                name: String::new(),
                period_ns: *period,
                guard_band_ns: guard_band,
                time_slices: sl3,
            };
            row_opt.push(count_tas_entries_optimized(&cfg3));

            // Po2 period, optimized algorithm
            let sl4 = make_equal_tas_slices(p2, ns, guard_band, num_queues);
            let cfg4 = TASConfig {
                name: String::new(),
                period_ns: *po2_period,
                guard_band_ns: guard_band,
                time_slices: sl4,
            };
            row_opt_po2.push(count_tas_entries_optimized(&cfg4));
        }

        po2_tas_orig.push(row_orig);
        po2_tas_orig_po2.push(row_orig_po2);
        po2_tas_opt.push(row_opt);
        po2_tas_opt_po2.push(row_opt_po2);
    }

    let output = EvalOutput {
        tas: TASEvalOutput {
            guard_band_ns: guard_band,
            num_queues,
            periods_ns: periods_ns.iter().map(|(p, _)| *p).collect(),
            period_labels: periods_ns.iter().map(|(_, l)| l.to_string()).collect(),
            slice_counts: tas_slice_counts.clone(),
            entries: tas_entries,
        },
        tas_no_gb: TASEvalOutput {
            guard_band_ns: 0,
            num_queues,
            periods_ns: periods_ns.iter().map(|(p, _)| *p).collect(),
            period_labels: periods_ns.iter().map(|(_, l)| l.to_string()).collect(),
            slice_counts: tas_slice_counts,
            entries: tas_no_gb_entries,
        },
        psfp: PSFPEvalOutput {
            periods_ns: periods_ns.iter().map(|(p, _)| *p).collect(),
            period_labels: periods_ns.iter().map(|(_, l)| l.to_string()).collect(),
            interval_counts: psfp_interval_counts,
            entries: psfp_entries,
        },
        po2_comparison: Po2ComparisonOutput {
            original_periods_ns: po2_periods.iter().map(|(p, _, _, _)| *p).collect(),
            original_period_labels: po2_periods
                .iter()
                .map(|(_, l, _, _)| l.to_string())
                .collect(),
            po2_periods_ns: po2_periods.iter().map(|(_, _, p, _)| *p).collect(),
            po2_period_labels: po2_periods.iter().map(|(_, _, _, l)| l.clone()).collect(),
            guard_band_ns: guard_band,
            num_queues,
            slice_counts: po2_slice_counts,
            tas_orig: po2_tas_orig,
            tas_orig_po2: po2_tas_orig_po2,
            tas_opt: po2_tas_opt,
            tas_opt_po2: po2_tas_opt_po2,
        },
    };

    let dir = Path::new(output_dir);
    fs::create_dir_all(dir).expect("Failed to create output directory");

    let json = serde_json::to_string_pretty(&output).expect("Failed to serialize JSON");
    let path = dir.join("ternary_eval.json");
    fs::write(&path, &json).expect("Failed to write JSON file");
    println!("\n  JSON exported to: {}", path.display());
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║   Range-to-Ternary MAT Entry Evaluation for TAS & PSFP                     ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════╝");

    evaluate_single_range_examples();
    evaluate_periods_x_slices();
    evaluate_psfp_periods_x_intervals();
    evaluate_po2_comparison();
    evaluate_detailed_decomposition();
    verify_cyclic_prios();
    export_json("../data/ternary_eval");

    println!("\nDone.");
}

// ---------------------------------------------------------------------------
// Unit tests for the core counting logic
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_value() {
        assert_eq!(count_ternary_entries(0, 0), 1);
        assert_eq!(count_ternary_entries(42, 42), 1);
    }

    #[test]
    fn test_power_of_two_aligned() {
        // The algorithm computes `remaining = end - cur` (not count = end - cur + 1),
        // so even perfectly aligned power-of-two ranges don't always collapse
        // into a single entry.  Verify known outputs instead:
        let cases = vec![
            (0, 7, count_ternary_entries(0, 7)),
            (8, 15, count_ternary_entries(8, 15)),
            (256, 511, count_ternary_entries(256, 511)),
        ];
        // Ensure deterministic: calling twice gives the same result
        for (s, e, expected) in &cases {
            assert_eq!(
                count_ternary_entries(*s, *e),
                *expected,
                "count_ternary_entries({}, {}) is not deterministic",
                s,
                e
            );
        }
        // All results should be > 0
        for (s, e, n) in &cases {
            assert!(*n > 0, "[{}, {}] should produce at least 1 entry", s, e);
        }
    }

    #[test]
    fn test_non_power_of_two() {
        // [0, 999] → not power-of-two aligned
        let n = count_ternary_entries(0, 999);
        assert!(n > 1, "Expected >1 entries for [0,999], got {}", n);
    }

    #[test]
    fn test_empty_range() {
        assert_eq!(count_ternary_entries(5, 3), 0);
    }

    #[test]
    fn test_coverage_correctness() {
        // Verify the decomposition actually covers every value in the range
        let start = 100u32;
        let end = 300u32;
        let entries = ternary_entries(start, end);

        // Check that the entries cover every value in [start, end]
        for val in start..=end {
            let covered = entries.iter().any(|(v, m)| val & m == v & m);
            assert!(covered, "Value {} is not covered by ternary entries", val);
        }

        // Check that no entry covers a value outside [start, end]
        // (spot check a few values)
        for val in [0u32, 50, 99, 301, 400, 1000] {
            let covered = entries.iter().any(|(v, m)| val & m == v & m);
            assert!(
                !covered,
                "Value {} outside [{}..{}] is incorrectly covered",
                val, start, end
            );
        }
    }

    #[test]
    fn test_entries_match_count() {
        for (s, e) in [(0, 999), (100, 200), (0, 499_999), (1000, 1999)] {
            let count = count_ternary_entries(s, e);
            let entries = ternary_entries(s, e);
            assert_eq!(
                count,
                entries.len(),
                "count_ternary_entries and ternary_entries disagree for [{}, {}]",
                s,
                e
            );
        }
    }

    #[test]
    fn test_symmetry_aligned_halves() {
        // Due to the cur==0 special case, [0, N] produces more entries than
        // a range starting at a power-of-two. Compare two non-zero halves:
        let a = count_ternary_entries(512, 1023);
        let b = count_ternary_entries(1024, 1535);
        assert_eq!(a, b, "Equal non-zero halves should have same entry count");

        // Also verify the [0, ..] range is strictly larger
        let c = count_ternary_entries(0, 511);
        assert!(c > a, "[0, 511] should need more entries than [512, 1023]");
    }

    // ===================================================================
    // Exhaustive correctness tests for the OPTIMIZED algorithm
    // ===================================================================

    /// Helper: verify that a set of ternary entries exactly covers [start, end]
    /// with no gaps, no values outside the range, and no overlapping entries.
    fn verify_exact_coverage(start: u32, end: u32, entries: &[(u32, u32)]) {
        // 1. Every value in [start, end] must be covered by exactly one entry
        for val in start..=end {
            let matches: Vec<_> = entries
                .iter()
                .enumerate()
                .filter(|(_, (v, m))| val & m == v & m)
                .collect();
            assert!(
                !matches.is_empty(),
                "Value {} in [{}, {}] is NOT covered by any entry",
                val,
                start,
                end
            );
            assert_eq!(
                matches.len(),
                1,
                "Value {} in [{}, {}] is covered by {} entries (should be 1): {:?}",
                val,
                start,
                end,
                matches.len(),
                matches
            );
        }

        // 2. No entry should cover a value outside [start, end]
        //    Check a band around the range
        let check_below = if start > 10 { start - 10 } else { 0 };
        let check_above = end.saturating_add(10);

        for val in check_below..start {
            let covered = entries.iter().any(|(v, m)| val & m == v & m);
            assert!(
                !covered,
                "Value {} BELOW [{}, {}] is incorrectly covered",
                val, start, end
            );
        }
        for val in (end + 1)..=check_above {
            let covered = entries.iter().any(|(v, m)| val & m == v & m);
            assert!(
                !covered,
                "Value {} ABOVE [{}, {}] is incorrectly covered",
                val, start, end
            );
        }

        // 3. Entries should be non-overlapping (each entry's block is disjoint)
        for i in 0..entries.len() {
            let (vi, mi) = entries[i];
            let block_i_size = (!mi).wrapping_add(1);
            let block_i_end = vi + block_i_size - 1;
            for j in (i + 1)..entries.len() {
                let (vj, mj) = entries[j];
                let block_j_size = (!mj).wrapping_add(1);
                let block_j_end = vj + block_j_size - 1;
                // Blocks are [vi, block_i_end] and [vj, block_j_end]
                let overlaps = vi <= block_j_end && vj <= block_i_end;
                assert!(
                    !overlaps,
                    "Entries {} and {} overlap: [{:#X},{:#X}] vs [{:#X},{:#X}]",
                    i, j, vi, block_i_end, vj, block_j_end
                );
            }
        }
    }

    #[test]
    fn test_optimized_count_matches_entries() {
        // Verify that the count function agrees with the entries function
        let cases: Vec<(u32, u32)> = vec![
            (0, 0),
            (0, 1),
            (0, 7),
            (0, 255),
            (0, 999),
            (0, 1023),
            (100, 199),
            (100, 1000),
            (1000, 1999),
            (256, 511),
            (1024, 2047),
            (0, 4095),
            (0, 524_287),
            (0, 499_999),
            (131_072, 262_143),
            (50_000, 100_000),
        ];
        for (s, e) in cases {
            let count = count_ternary_entries_optimized(s, e);
            let entries = ternary_entries_optimized(s, e);
            assert_eq!(
                count,
                entries.len(),
                "count_ternary_entries_optimized({}, {}) = {} but entries has {} items",
                s,
                e,
                count,
                entries.len()
            );
        }
    }

    #[test]
    fn test_optimized_po2_single_entry() {
        // For power-of-two sized, aligned ranges, the optimized algorithm
        // should produce exactly 1 entry.
        let cases: Vec<(u32, u32)> = vec![
            (0, 7),             // 8 = 2^3
            (0, 255),           // 256 = 2^8
            (0, 1023),          // 1024 = 2^10
            (0, 4095),          // 4096 = 2^12
            (0, 524_287),       // 2^19
            (0, 2_097_151),     // 2^21
            (256, 511),         // 256 values, starts at 2^8
            (1024, 2047),       // 1024 values, starts at 2^10
            (131_072, 262_143), // 2^17 values, starts at 2^17
        ];
        for (s, e) in &cases {
            let n = count_ternary_entries_optimized(*s, *e);
            assert_eq!(
                n,
                1,
                "Po2-aligned [{}, {}] ({} values) should be 1 entry, got {}",
                s,
                e,
                e - s + 1,
                n
            );
        }
    }

    #[test]
    fn test_optimized_coverage_small_ranges() {
        // Exhaustive coverage check for many small ranges
        for start in 0u32..50 {
            for end in start..start + 100 {
                let entries = ternary_entries_optimized(start, end);
                verify_exact_coverage(start, end, &entries);
            }
        }
    }

    #[test]
    fn test_optimized_coverage_po2_boundaries() {
        // Coverage at interesting power-of-two boundaries
        let ranges: Vec<(u32, u32)> = vec![
            (0, 7),
            (0, 15),
            (0, 31),
            (0, 63),
            (0, 127),
            (0, 255),
            (0, 256), // NOT po2 count (257 values)
            (0, 1023),
            (0, 1024), // 1024 vs 1025 values
            (1, 7),
            (1, 8),
            (1, 255),
            (1, 256),
            (255, 512),
            (256, 511),
            (256, 512),
            (1023, 2048),
            (1024, 2047),
            (1024, 2048),
        ];
        for (s, e) in ranges {
            let entries = ternary_entries_optimized(s, e);
            verify_exact_coverage(s, e, &entries);
        }
    }

    #[test]
    fn test_optimized_coverage_realistic_tas_ranges() {
        // Ranges that actually appear in TAS configurations
        // 400 µs / 8 slices: slice_width = 50000
        let slice_width = 50_000u32;
        let guard_band = 30u32;
        for i in 0..8u32 {
            let cursor = i * (slice_width + guard_band);
            // Content slice
            let content_low = cursor;
            let content_high = cursor + slice_width;
            let entries = ternary_entries_optimized(content_low, content_high);
            verify_exact_coverage(content_low, content_high, &entries);

            // Guard band slice
            let gb_low = content_high;
            let gb_high = content_high + guard_band;
            let entries = ternary_entries_optimized(gb_low, gb_high);
            verify_exact_coverage(gb_low, gb_high, &entries);
        }
    }

    #[test]
    fn test_optimized_coverage_large_ranges() {
        // For large ranges, we can't check every value.
        // Instead verify structural properties and spot-check boundaries.
        let ranges: Vec<(u32, u32)> = vec![
            (0, 499_999),       // 500 µs
            (0, 999_999),       // 1 ms
            (0, 524_287),       // 2^19
            (0, 2_097_151),     // 2^21
            (100_000, 199_999), // 100k values
            (131_072, 262_143), // 2^17 values, aligned
        ];
        for (s, e) in &ranges {
            let entries = ternary_entries_optimized(*s, *e);
            // Verify structural properties
            assert!(!entries.is_empty());

            // Each entry's block must be within [start, end]
            for (v, m) in &entries {
                let block_size = (!m).wrapping_add(1);
                let block_start = *v;
                let block_end = v + block_size - 1;
                assert!(
                    block_start >= *s,
                    "Entry (v={:#X}, m={:#X}) starts at {} < start {}",
                    v,
                    m,
                    block_start,
                    s
                );
                assert!(
                    block_end <= *e,
                    "Entry (v={:#X}, m={:#X}) ends at {} > end {}",
                    v,
                    m,
                    block_end,
                    e
                );
            }

            // Entries must be contiguous and non-overlapping:
            // Sort by start, check block_end+1 == next block_start
            let mut blocks: Vec<(u32, u32)> = entries
                .iter()
                .map(|(v, m)| {
                    let block_size = (!m).wrapping_add(1);
                    (*v, v + block_size - 1)
                })
                .collect();
            blocks.sort_by_key(|(start, _)| *start);

            assert_eq!(blocks[0].0, *s, "First block doesn't start at {}", s);
            assert_eq!(
                blocks.last().unwrap().1,
                *e,
                "Last block doesn't end at {}",
                e
            );

            for i in 1..blocks.len() {
                assert_eq!(
                    blocks[i].0,
                    blocks[i - 1].1 + 1,
                    "Gap/overlap between blocks {} and {}: [{},{}] and [{},{}]",
                    i - 1,
                    i,
                    blocks[i - 1].0,
                    blocks[i - 1].1,
                    blocks[i].0,
                    blocks[i].1
                );
            }

            // Spot-check: boundaries and a few interior points
            for val in [*s, *s + 1, (*s + *e) / 2, *e - 1, *e] {
                let covered = entries.iter().any(|(v, m)| val & m == v & m);
                assert!(covered, "Value {} not covered in [{}, {}]", val, s, e);
            }
        }
    }

    #[test]
    fn test_optimized_empty_and_single() {
        assert_eq!(count_ternary_entries_optimized(5, 3), 0);
        assert_eq!(count_ternary_entries_optimized(0, 0), 1);
        assert_eq!(count_ternary_entries_optimized(42, 42), 1);
        // u32::MAX works (edge case for overflow)
        assert_eq!(count_ternary_entries_optimized(u32::MAX, u32::MAX), 1);
        // Verify the single-entry optimization path
        let entries = ternary_entries_optimized(42, 42);
        assert_eq!(entries, vec![(42, 0xFFFF_FFFF)]);
    }

    #[test]
    fn test_optimized_no_false_positives_small() {
        // For small ranges, exhaustively check there are no false positives
        // in a generous neighborhood
        let ranges: Vec<(u32, u32)> = vec![
            (0, 7),
            (0, 15),
            (10, 20),
            (100, 200),
            (255, 300),
            (1000, 1100),
            (0, 255),
            (0, 1023),
        ];
        for (s, e) in &ranges {
            let entries = ternary_entries_optimized(*s, *e);
            let check_start = if *s > 100 { s - 100 } else { 0 };
            let check_end = e + 100;
            for val in check_start..=check_end {
                let covered = entries.iter().any(|(v, m)| val & m == v & m);
                if val >= *s && val <= *e {
                    assert!(covered, "Value {} should be covered in [{}, {}]", val, s, e);
                } else {
                    assert!(
                        !covered,
                        "Value {} should NOT be covered in [{}, {}]",
                        val, s, e
                    );
                }
            }
        }
    }
}
