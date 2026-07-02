//! Edge 3 — Admin Fee Collection
//! Curve collects admin fees when withdraw_admin_fees() is called on a pool.
//! This slightly tilts the pool. Low-value edge but zero competition.


/// withdraw_admin_fees() selector: 0x30c54085
pub const WITHDRAW_ADMIN_FEES_SELECTOR: [u8; 4] = [0x30, 0xc5, 0x40, 0x85];
