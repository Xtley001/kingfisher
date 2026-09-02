import { useState } from 'react'
import { useBotStore, BotParams, SlippageModelParams } from '../stores/botStore'

export function Parameters() {
  const { params, updateParams } = useBotStore()
  const [local,   setLocal]      = useState<BotParams>({ ...params })
  const [saving,  setSaving]     = useState(false)
  const [savedAt, setSavedAt]    = useState<Date | null>(null)

  const dirty = JSON.stringify(local) !== JSON.stringify(params)

  const save = async () => {
    setSaving(true)
    await updateParams(local)
    setSaving(false)
    setSavedAt(new Date())
  }

  const reset = () => setLocal({ ...params })

  const updateNumeric = (key: keyof BotParams, val: number) => {
    setLocal(prev => ({ ...prev, [key]: val }))
  }

  const updateSlippage = (key: keyof SlippageModelParams, val: number) => {
    setLocal(prev => ({
      ...prev,
      slippage_model: {
        ...prev.slippage_model,
        [key]: val,
      },
    }))
  }

  return (
    <div style={{ maxWidth: 640, display: 'flex', flexDirection: 'column', gap: 16 }}>

      {/* Info banner */}
      <div
        className="glass"
        style={{ padding: '10px 14px', display: 'flex', gap: 10, borderColor: 'rgba(122,154,74,0.25)' }}
      >
        <span style={{ color: 'var(--olive-text)', flexShrink: 0 }}>ℹ</span>
        <p style={{ margin: 0, fontSize: 11, color: 'var(--text-secondary)', lineHeight: 1.6 }}>
          All parameters update live on the next block scan and persist to storage across restarts.
          Tune execution thresholds, gas budgets, Timeboost bidding, and slippage tolerances dynamically.
        </p>
      </div>

      {/* 1. Profit & Sizing */}
      <div className="glass-card" style={{ padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 12 }}>
        <h3 style={{ margin: 0, fontSize: 13, color: 'var(--olive-text)' }}>💰 Profit & Sizing</h3>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <div>
            <label className="label" htmlFor="param-min_profit_usd">Min Net Profit (USD)</label>
            <input
              id="param-min_profit_usd"
              type="number"
              step={5}
              value={local.min_profit_usd}
              onChange={e => updateNumeric('min_profit_usd', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Absolute net profit floor. Trades below this are skipped.</p>
          </div>
          <div>
            <label className="label" htmlFor="param-min_gas_roi">Min Gas ROI Multiplier</label>
            <input
              id="param-min_gas_roi"
              type="number"
              step={0.5}
              value={local.min_gas_roi}
              onChange={e => updateNumeric('min_gas_roi', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Dynamic profit floor multiplier: max(min_profit, gas * ROI).</p>
          </div>
        </div>
        <div>
          <label className="label" htmlFor="param-abs_cap_usd" style={{ color: 'var(--amber-text)' }}>
            ⚠ Emergency Borrow Ceiling (USD)
          </label>
          <input
            id="param-abs_cap_usd"
            type="number"
            step={1_000_000}
            value={local.abs_cap_usd}
            onChange={e => updateNumeric('abs_cap_usd', parseFloat(e.target.value) || 0)}
            className="input-field"
          />
          <p className="hint">Upper safety ceiling across all pool sizes (max $25M).</p>
        </div>
      </div>

      {/* 2. Filters */}
      <div className="glass-card" style={{ padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 12 }}>
        <h3 style={{ margin: 0, fontSize: 13, color: 'var(--olive-text)' }}>🔍 Market Filters</h3>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <div>
            <label className="label" htmlFor="param-min_imbalance_pct">Min Imbalance (%)</label>
            <input
              id="param-min_imbalance_pct"
              type="number"
              step={0.5}
              value={local.min_imbalance_pct}
              onChange={e => updateNumeric('min_imbalance_pct', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Pool must be this % off balance to enter the scan pipeline.</p>
          </div>
          <div>
            <label className="label" htmlFor="param-min_velocity">Min Velocity</label>
            <input
              id="param-min_velocity"
              type="number"
              step={0.001}
              value={local.min_velocity}
              onChange={e => updateNumeric('min_velocity', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Freshness filter — rejects slow drift already being arbitraged.</p>
          </div>
        </div>
      </div>

      {/* 3. Gas & Safety */}
      <div className="glass-card" style={{ padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 12 }}>
        <h3 style={{ margin: 0, fontSize: 13, color: 'var(--olive-text)' }}>⛽ Gas & Safety</h3>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <div>
            <label className="label" htmlFor="param-gas_reserve_eth">Gas Halt Floor (ETH)</label>
            <input
              id="param-gas_reserve_eth"
              type="number"
              step={0.01}
              value={local.gas_reserve_eth}
              onChange={e => updateNumeric('gas_reserve_eth', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Bot halts when wallet ETH drops below this.</p>
          </div>
          <div>
            <label className="label" htmlFor="param-alert_gas_eth">Gas Alert Threshold (ETH)</label>
            <input
              id="param-alert_gas_eth"
              type="number"
              step={0.05}
              value={local.alert_gas_eth}
              onChange={e => updateNumeric('alert_gas_eth', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Alert notification fires when wallet drops below this.</p>
          </div>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 12 }}>
          <div>
            <label className="label" htmlFor="param-gas_limit_2hop">2-Hop Gas Limit</label>
            <input
              id="param-gas_limit_2hop"
              type="number"
              step={25_000}
              value={local.gas_limit_2hop}
              onChange={e => updateNumeric('gas_limit_2hop', parseInt(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Tight gas budget for 2-pool trades (default 350k).</p>
          </div>
          <div>
            <label className="label" htmlFor="param-gas_limit_4hop">4-Hop Gas Limit</label>
            <input
              id="param-gas_limit_4hop"
              type="number"
              step={25_000}
              value={local.gas_limit_4hop}
              onChange={e => updateNumeric('gas_limit_4hop', parseInt(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Max budget for multi-hop routes (default 750k).</p>
          </div>
          <div>
            <label className="label" htmlFor="param-stress_priority_fee_multiplier">Stress Tip Mult</label>
            <input
              id="param-stress_priority_fee_multiplier"
              type="number"
              step={0.05}
              value={local.stress_priority_fee_multiplier}
              onChange={e => updateNumeric('stress_priority_fee_multiplier', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Base fee multiplier for priority tip during stress (e.g. 0.25 = 25%).</p>
          </div>
        </div>
      </div>

      {/* 4. Timeboost */}
      <div className="glass-card" style={{ padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 12 }}>
        <h3 style={{ margin: 0, fontSize: 13, color: 'var(--olive-text)' }}>⚡ Arbitrum Timeboost</h3>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <div>
            <label className="label" htmlFor="param-timeboost_min_profit_usd">Timeboost Min Profit (USD)</label>
            <input
              id="param-timeboost_min_profit_usd"
              type="number"
              step={10}
              value={local.timeboost_min_profit_usd}
              onChange={e => updateNumeric('timeboost_min_profit_usd', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Only opportunities above this profit will bid for the express lane.</p>
          </div>
          <div>
            <label className="label" htmlFor="param-timeboost_race_loss_threshold">Race Loss Rate Trigger</label>
            <input
              id="param-timeboost_race_loss_threshold"
              type="number"
              step={0.05}
              value={local.timeboost_race_loss_threshold}
              onChange={e => updateNumeric('timeboost_race_loss_threshold', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">If recent race losses exceed this fraction (e.g. 0.25 = 25%), switch to Timeboost.</p>
          </div>
        </div>
      </div>

      {/* 5. Execution Optimizations */}
      <div className="glass-card" style={{ padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 12 }}>
        <h3 style={{ margin: 0, fontSize: 13, color: 'var(--olive-text)' }}>🚀 Execution Optimizations</h3>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 12, alignItems: 'center' }}>
          <div>
            <label className="label" htmlFor="param-flash_source_preference">Flash Loan Source</label>
            <select
              id="param-flash_source_preference"
              value={local.flash_source_preference}
              onChange={e => setLocal(prev => ({
                ...prev,
                flash_source_preference: e.target.value as 'aave_only' | 'balancer_preferred',
              }))}
              className="input-field"
            >
              <option value="balancer_preferred">Balancer Preferred (0% fee)</option>
              <option value="aave_only">Aave Only (5 bps fee)</option>
            </select>
            <p className="hint">Prefer Balancer V2 zero-fee loans when liquidity allows.</p>
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            <label className="label" style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={local.calldata_cache_enabled}
                onChange={e => setLocal(prev => ({ ...prev, calldata_cache_enabled: e.target.checked }))}
              />
              Calldata Cache
            </label>
            <p className="hint">Cache route calldata within each block for sub-1ms re-encoding.</p>
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            <label className="label" style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={local.presigned_pool_enabled}
                onChange={e => setLocal(prev => ({ ...prev, presigned_pool_enabled: e.target.checked }))}
              />
              Pre-Signed Pool
            </label>
            <p className="hint">Keep pre-computed nonce and gas envelopes off the hot path.</p>
          </div>
        </div>
      </div>

      {/* 6. Slippage Model */}
      <div className="glass-card" style={{ padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 12 }}>
        <h3 style={{ margin: 0, fontSize: 13, color: 'var(--olive-text)' }}>📐 Dynamic Slippage Model</h3>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 12 }}>
          <div>
            <label className="label">Depth Base (Deep)</label>
            <input
              type="number"
              step={0.001}
              value={local.slippage_model.depth_base}
              onChange={e => updateSlippage('depth_base', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Baseline tolerance for deep pools (default 0.003 = 0.3%).</p>
          </div>
          <div>
            <label className="label">Depth Shallow Factor</label>
            <input
              type="number"
              step={0.002}
              value={local.slippage_model.depth_shallow}
              onChange={e => updateSlippage('depth_shallow', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Additional tolerance for shallow pools (default 0.012).</p>
          </div>
          <div>
            <label className="label">Time Drift Rate</label>
            <input
              type="number"
              step={0.0001}
              value={local.slippage_model.time_drift_rate}
              onChange={e => updateSlippage('time_drift_rate', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Tolerance added per block since scan (default 0.0002).</p>
          </div>
          <div>
            <label className="label">Time Drift Cap</label>
            <input
              type="number"
              step={0.001}
              value={local.slippage_model.time_drift_cap}
              onChange={e => updateSlippage('time_drift_cap', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Max lag tolerance ceiling (default 0.005 = 0.5%).</p>
          </div>
          <div>
            <label className="label">Size Ratio Weight</label>
            <input
              type="number"
              step={0.005}
              value={local.slippage_model.size_ratio_weight}
              onChange={e => updateSlippage('size_ratio_weight', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Impact allowance for trade size / pool depth (default 0.02).</p>
          </div>
          <div>
            <label className="label" style={{ color: 'var(--amber-text)' }}>Max Slippage Cap</label>
            <input
              type="number"
              step={0.005}
              value={local.slippage_model.hard_cap}
              onChange={e => updateSlippage('hard_cap', parseFloat(e.target.value) || 0)}
              className="input-field"
            />
            <p className="hint">Strict upper safety ceiling (max 0.03 = 3%).</p>
          </div>
        </div>
      </div>

      {/* Action row */}
      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, alignItems: 'center', marginTop: 4 }}>
        {savedAt && !dirty && (
          <span className="label" style={{ color: 'var(--green-text)' }}>
            ✓ Saved {savedAt.toLocaleTimeString()}
          </span>
        )}
        {dirty && (
          <button className="btn btn-ghost" onClick={reset}>Reset</button>
        )}
        <button
          className={`btn ${dirty ? 'btn-olive' : 'btn-ghost'}`}
          onClick={save}
          disabled={saving || !dirty}
        >
          {saving ? 'Saving…' : 'Apply Changes'}
        </button>
      </div>

      <style>{`
        .input-field {
          width: 100%;
          background: rgba(0,0,0,0.25);
          border: 1px solid var(--glass-border);
          border-radius: var(--r-sm);
          padding: 6px 10px;
          color: var(--text-primary);
          font-family: var(--font);
          fontSize: 12px;
          outline: none;
          box-sizing: border-box;
        }
        .hint {
          margin: 4px 0 0;
          font-size: 10px;
          color: var(--text-muted);
          line-height: 1.4;
        }
      `}</style>
    </div>
  )
}
