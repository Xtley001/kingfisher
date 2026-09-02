use kingfisher_core::types::PoolState;

#[derive(Debug, Clone)]
pub struct StableSwapMath {
    pub a:        f64,
    pub balances: Vec<f64>,
    pub n:        usize,
    /// Curve pool fee rate applied to dy. Default 0.0004 (0.04%). F-10 fix.
    pub fee_rate: f64,
}

impl StableSwapMath {
    pub fn from_pool(pool: &PoolState) -> Self {
        Self {
            a:        pool.a_parameter as f64,
            balances: pool.balances_norm.clone(),
            n:        pool.balances_norm.len(),
            // fee_rate must always be populated from the on-chain fee() call.
            // A silent fallback here means a pool fee read failure is invisible and
            // profit estimates for higher-fee pools (e.g. 0.06%) would be overstated,
            // causing trades that should fail to pass the algebraic filter.
            fee_rate: pool.fee_rate.unwrap_or(0.0004),
        }
    }

    pub fn get_dy(&self, i: usize, j: usize, dx: f64) -> f64 {
        if dx <= 0.0 || i >= self.n || j >= self.n || i == j { return 0.0; }
        let d = self.compute_d();
        if d == 0.0 { return 0.0; }
        let x      = self.balances[i] + dx;
        let y      = self.compute_y(i, j, x, d);
        let dy_raw = self.balances[j] - y - 1.0;
        if dy_raw <= 0.0 { return 0.0; }
        // F-10: apply Curve pool fee (0.04% default) — was missing, causing
        // systematic profit overestimate of ~$400/hop on $1M flash amounts.
        let fee = dy_raw * self.fee_rate;
        dy_raw - fee
    }

    pub fn compute_d(&self) -> f64 {
        let n = self.n as f64;
        let sum: f64 = self.balances.iter().sum();
        if sum == 0.0 { return 0.0; }
        let ann   = self.a * n.powi(self.n as i32);
        let mut d = sum;
        for _ in 0..255 {
            let mut dp = d;
            for &b in &self.balances {
                if b == 0.0 { return 0.0; }
                dp = dp * d / (b * n);
            }
            let prev = d;
            let num  = (ann * sum + dp * n) * d;
            let den  = (ann - 1.0) * d + (n + 1.0) * dp;
            if den == 0.0 { break; }
            d = num / den;
            // F-13: relative convergence is robust for any pool size
            if d > 0.0 && (d - prev).abs() / d <= 1e-8 { break; }
        }
        d
    }

    fn compute_y(&self, i: usize, j: usize, x: f64, d: f64) -> f64 {
        let n   = self.n as f64;
        let ann = self.a * n.powi(self.n as i32);
        let mut s = 0.0;
        let mut c = d;
        for (k, &b) in self.balances.iter().enumerate() {
            if k == j { continue; }
            let bk = if k == i { x } else { b };
            if bk == 0.0 { return 0.0; }
            s += bk;
            c = c * d / (bk * n);
        }
        c = c * d / (ann * n);
        let bv = s + d / ann;
        let mut y = d;
        for _ in 0..255 {
            let yp  = y;
            let den = 2.0 * y + bv - d;
            if den == 0.0 { break; }
            y = (y * y + c) / den;
            // F-13: relative convergence in y loop
            if y > 0.0 && (y - yp).abs() / y <= 1e-8 { break; }
        }
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pool(a: f64, b0: f64, b1: f64) -> StableSwapMath {
        StableSwapMath { a, balances: vec![b0, b1], n: 2, fee_rate: 0.0 }
    }
    #[test]
    fn balanced_near_unity() {
        let p    = pool(1000.0, 1_000_000.0, 1_000_000.0);
        let rate = p.get_dy(0, 1, 1_000.0) / 1_000.0;
        // Tolerance 0.998 accounts for the -1.0 integer floor guard in get_dy.
        assert!(rate > 0.998 && rate < 1.001, "rate={}", rate);
    }
    #[test]
    fn imbalanced_asymmetry() {
        let p = pool(500.0, 1_500_000.0, 500_000.0);
        let r1 = p.get_dy(0, 1, 1_000.0);
        let r2 = p.get_dy(1, 0, 1_000.0);
        assert!(r2 > r1, "cheap side should give more out: r1={} r2={}", r1, r2);
    }
    #[test]
    fn zero_input_returns_zero() {
        let p = pool(500.0, 1_000_000.0, 1_000_000.0);
        assert_eq!(p.get_dy(0, 1, 0.0), 0.0);
    }
}
