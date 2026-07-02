//! Edge 4 — Peg Stress Monitor
//! Monitors Chainlink USDC/USD and USDT/USD feeds.
//! Deviation > 0.2% = stress regime: pool imbalances are 3-10x normal size.
//! This is Kingfisher's highest single-event alpha (see 00_GREY_PAPER.md §7, Edge 4).
//! Pre-signed templates fire immediately without compute delay.


#[derive(Debug, Clone)]
pub struct PegStatus {
    pub usdc_price: f64,
    pub usdt_price: f64,
    pub frax_price: f64,
    pub is_stress:  bool,
    pub stress_tokens: Vec<StressedToken>,
}

#[derive(Debug, Clone)]
pub struct StressedToken {
    pub symbol:        String,
    pub price:         f64,
    pub deviation_pct: f64,
}

impl PegStatus {
    /// Construct from live Chainlink prices (already fetched by multicall).
    pub fn from_prices(usdc: f64, usdt: f64, frax: f64) -> Self {
        let mut stressed = vec![];

        for (sym, price, threshold) in [
            ("USDC", usdc, 0.002_f64),
            ("USDT", usdt, 0.002_f64),
            ("FRAX", frax, 0.005_f64),  // FRAX allowed 0.5% before stress
        ] {
            let dev = (price - 1.0).abs();
            if dev > threshold {
                stressed.push(StressedToken {
                    symbol:        sym.into(),
                    price,
                    deviation_pct: dev * 100.0,
                });
            }
        }

        let is_stress = !stressed.is_empty();

        if is_stress {
            for t in &stressed {
                tracing::warn!(
                    token = %t.symbol,
                    price = t.price,
                    dev_pct = t.deviation_pct,
                    "⚡ PEG STRESS — optimal sizing mode active"
                );
            }
        }

        Self {
            usdc_price: usdc,
            usdt_price: usdt,
            frax_price: frax,
            is_stress,
            stress_tokens: stressed,
        }
    }

    /// Worst-case deviation across all monitored pegs
    pub fn max_deviation_pct(&self) -> f64 {
        self.stress_tokens
            .iter()
            .map(|t| t.deviation_pct)
            .fold(0.0_f64, f64::max)
    }

    /// Which pool direction profits during this stress?
    /// Down = the stressed token is cheap → buy it (route: other_token → stressed_token)
    pub fn profitable_directions(&self) -> Vec<(&str, bool)> {
        self.stress_tokens
            .iter()
            .map(|t| (t.symbol.as_str(), t.price < 1.0))
            .collect()
    }
}

impl PegStatus {
    /// Returns true if any monitored peg is in stress.
    pub fn any_stressed(&self) -> bool {
        self.is_stress
    }
}
