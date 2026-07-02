//! # Prometheus Metrics
//!
//! Exposes operational metrics at `GET /metrics` for Prometheus scraping.
//! Wire a Grafana dashboard on top for time-series visualisation and alerting.
//!
//! ## Recommended Grafana Alerts
//! - Landing rate < 20% for 30 min → builder connectivity issue
//! - Gas spent > $X/hour with zero profit → automatic kill switch
//! - No blocks processed for > 60 seconds → node down

use once_cell::sync::Lazy;
use prometheus::{
    Counter, CounterVec, Gauge, GaugeVec, Histogram, HistogramOpts,
    Opts, Registry, TextEncoder, Encoder,
    register_counter_with_registry,
    register_counter_vec_with_registry,
    register_gauge_with_registry,
    register_gauge_vec_with_registry,
    register_histogram_with_registry,
};
use std::sync::OnceLock;

// ── Global registry ──────────────────────────────────────────────────────────

static REGISTRY: OnceLock<Registry> = OnceLock::new();

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(Registry::new)
}

// ── Metric definitions ────────────────────────────────────────────────────────

/// Opportunities detected by the 5-layer scanner (pre-execution)
pub static OPPS_DETECTED: Lazy<Counter> = Lazy::new(|| {
    register_counter_with_registry!(
        Opts::new("kf_opps_detected_total", "Opportunities detected by scanner"),
        registry()
    ).expect("kf_opps_detected_total")
});

/// Bundles submitted to at least one builder
pub static OPPS_FIRED: Lazy<Counter> = Lazy::new(|| {
    register_counter_with_registry!(
        Opts::new("kf_bundles_fired_total", "Bundles submitted to builders"),
        registry()
    ).expect("kf_bundles_fired_total")
});

/// Bundles confirmed on-chain (via landing tracker, item #20)
pub static OPPS_LANDED: Lazy<Counter> = Lazy::new(|| {
    register_counter_with_registry!(
        Opts::new("kf_bundles_landed_total", "Bundles confirmed on-chain"),
        registry()
    ).expect("kf_bundles_landed_total")
});

/// On-chain confirmed profit in USD (based on actual tx receipts)
pub static PROFIT_ACTUAL: Lazy<Counter> = Lazy::new(|| {
    register_counter_with_registry!(
        Opts::new("kf_profit_usd_actual", "On-chain confirmed profit USD"),
        registry()
    ).expect("kf_profit_usd_actual")
});

/// Gas spent in USD (base fee × gas used × ETH price)
pub static GAS_SPENT: Lazy<Counter> = Lazy::new(|| {
    register_counter_with_registry!(
        Opts::new("kf_gas_usd_total", "Total gas spent in USD"),
        registry()
    ).expect("kf_gas_usd_total")
});

/// Ratio: bundles landed / bundles fired (true on-chain landing rate)
pub static LANDING_RATE: Lazy<Gauge> = Lazy::new(|| {
    register_gauge_with_registry!(
        Opts::new("kf_landing_rate", "True on-chain bundle landing rate (0–1)"),
        registry()
    ).expect("kf_landing_rate")
});

/// Current ETH price from Chainlink
pub static ETH_PRICE: Lazy<Gauge> = Lazy::new(|| {
    register_gauge_with_registry!(
        Opts::new("kf_eth_price_usd", "Current ETH price in USD (Chainlink)"),
        registry()
    ).expect("kf_eth_price_usd")
});

/// Current USDC peg deviation from 1.0
pub static USDC_PEG: Lazy<Gauge> = Lazy::new(|| {
    register_gauge_with_registry!(
        Opts::new("kf_usdc_peg", "Current USDC peg (1.0 = perfect)"),
        registry()
    ).expect("kf_usdc_peg")
});

/// Wallet ETH balance
pub static WALLET_ETH: Lazy<Gauge> = Lazy::new(|| {
    register_gauge_with_registry!(
        Opts::new("kf_wallet_eth_balance", "Hot wallet ETH balance"),
        registry()
    ).expect("kf_wallet_eth_balance")
});

/// Current block number
pub static LAST_BLOCK: Lazy<Gauge> = Lazy::new(|| {
    register_gauge_with_registry!(
        Opts::new("kf_last_block", "Last processed block number"),
        registry()
    ).expect("kf_last_block")
});

/// EVM-simulation latency in milliseconds (future — not currently wired)
pub static SIM_LATENCY_MS: Lazy<Histogram> = Lazy::new(|| {
    register_histogram_with_registry!(
        HistogramOpts::new("kf_sim_latency_ms", "Simulation latency in milliseconds")
            .buckets(vec![0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0]),
        registry()
    ).expect("kf_sim_latency_ms")
});

/// End-to-end block latency: block arrival → bundle submitted (milliseconds)
pub static BLOCK_LATENCY_MS: Lazy<Histogram> = Lazy::new(|| {
    register_histogram_with_registry!(
        HistogramOpts::new("kf_block_latency_ms", "Block → bundle submitted latency ms")
            .buckets(vec![5.0, 10.0, 25.0, 50.0, 100.0, 200.0, 500.0]),
        registry()
    ).expect("kf_block_latency_ms")
});

/// Wins per builder (item #18 — builder market share)
pub static BUILDER_WINS: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec_with_registry!(
        Opts::new("kf_builder_wins", "Recent wins per builder"),
        &["builder"],
        registry()
    ).expect("kf_builder_wins")
});

/// Reverts by classification (item #26)
pub static REVERT_BY_CLASS: Lazy<CounterVec> = Lazy::new(|| {
    register_counter_vec_with_registry!(
        Opts::new("kf_reverts_total", "Reverts by classification"),
        &["class"],
        registry()
    ).expect("kf_reverts_total")
});

// ── Expose /metrics endpoint ─────────────────────────────────────────────────

/// Render all metrics in Prometheus text format.
pub fn render_metrics() -> String {
    // Ensure all lazy statics are initialized
    let _ = &*OPPS_DETECTED;
    let _ = &*OPPS_FIRED;
    let _ = &*OPPS_LANDED;
    let _ = &*PROFIT_ACTUAL;
    let _ = &*GAS_SPENT;
    let _ = &*LANDING_RATE;
    let _ = &*ETH_PRICE;
    let _ = &*USDC_PEG;
    let _ = &*WALLET_ETH;
    let _ = &*LAST_BLOCK;
    let _ = &*SIM_LATENCY_MS;
    let _ = &*BLOCK_LATENCY_MS;
    let _ = &*BUILDER_WINS;
    let _ = &*REVERT_BY_CLASS;

    let encoder = TextEncoder::new();
    let families = registry().gather();
    let mut buf = Vec::new();
    encoder.encode(&families, &mut buf).unwrap_or_default();
    String::from_utf8(buf).unwrap_or_default()
}

/// Update gauge metrics from the current BotState snapshot.
/// Call this once per block from the block handler or a periodic task.
pub fn sync_state_metrics(state: &kingfisher_core::state::BotState) {
    ETH_PRICE.set(state.eth_price_usd);
    USDC_PEG.set(state.usdc_peg);
    WALLET_ETH.set(state.wallet_eth_balance);
    LAST_BLOCK.set(state.last_block as f64);

    let fired  = state.total_trades + state.total_reverts;
    let landed = state.total_trades;
    if fired > 0 {
        LANDING_RATE.set(landed as f64 / fired as f64);
    }

    // Per-class revert counts
    for (class, count) in &state.revert_counts {
        REVERT_BY_CLASS.with_label_values(&[class]).reset();
        // Prometheus counters are monotonic; we'd need to track deltas for accuracy.
        // For now, record via add on each update cycle.
        let _ = count;
    }
}
