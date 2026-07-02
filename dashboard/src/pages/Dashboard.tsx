import { Panel }           from '../components/panels/Panel'
import { useBotStore }     from '../stores/botStore'
import { fmt }              from '../utils/formatters'
import { GasTank }          from '../components/GasTank'
import { AaveStatus }       from '../components/AaveStatus'
import { OpportunityFeed }  from '../components/OpportunityFeed'

export function Dashboard() {
  const {
    running, uptime_secs, stress_regime,
    usdc_peg, usdt_peg,
    total_profit_usd, today_profit_usd,
    total_trades, today_trades,
    win_rate, pool_states, recent_txs,
    consecutive_reverts,
  } = useBotStore()

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 10, maxWidth: 1400 }}>

      {/* Status bar */}
      <div
        className="glass"
        style={{
          display:     'flex',
          justifyContent: 'space-between',
          alignItems:  'center',
          padding:     '8px 12px',
          borderColor: stress_regime ? 'rgba(124,94,42,0.40)' : undefined,
          background:  stress_regime ? 'rgba(124,94,42,0.05)' : undefined,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            <div className={`dot ${running ? 'dot-green' : 'dot-red'}`} />
            <span className="label">{running ? 'Active' : 'Paused'}</span>
          </div>
          {consecutive_reverts > 0 && (
            <span className="label" style={{ color: 'var(--amber-text)' }}>
              ⚠ {consecutive_reverts} consecutive reverts
            </span>
          )}
          {stress_regime && (
            <span className="label" style={{ color: 'var(--amber-text)' }}>
              ⚡ Stress Regime — Optimal Sizing Active
            </span>
          )}
        </div>
        <span className="label">Uptime {fmt.uptime(uptime_secs)}</span>
      </div>

      {/* Metrics row */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(5,1fr)', gap: 8 }}>
        {([
          { label: 'Today P&L',    val: fmt.usdSigned(today_profit_usd),  color: today_profit_usd >= 0 ? 'var(--green-text)' : 'var(--red-text)' },
          { label: 'Total P&L',   val: fmt.usdSigned(total_profit_usd),  color: total_profit_usd >= 0 ? 'var(--green-text)' : 'var(--red-text)' },
          { label: 'Today Trades', val: String(today_trades),             color: 'var(--text-primary)' },
          { label: 'Total Trades', val: String(total_trades),             color: 'var(--text-primary)' },
          { label: 'Win Rate',    val: fmt.pct(win_rate),                color: win_rate >= 0.70 ? 'var(--green-text)' : 'var(--amber-text)' },
        ] as const).map(m => (
          <div key={m.label} className="glass-card" style={{ padding: '10px 12px' }}>
            <div className="label" style={{ marginBottom: 4 }}>{m.label}</div>
            <div className="value-lg" style={{ color: m.color }}>{m.val}</div>
          </div>
        ))}
      </div>

      {/* Gas / Peg / Aave row */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 8 }}>
        <Panel title="Gas Tank" style={{ minHeight: 140 }}>
          <GasTank />
        </Panel>

        <Panel title="Peg Monitor" style={{ minHeight: 140 }}>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {([['USDC/USD', usdc_peg], ['USDT/USD', usdt_peg]] as [string, number][]).map(([sym, price]) => {
              const dev = (price - 1.0) * 100
              const stressed = Math.abs(dev) > 0.2
              return (
                <div key={sym} style={{
                  display:        'flex',
                  justifyContent: 'space-between',
                  alignItems:     'center',
                  padding:        '6px 0',
                  borderBottom:   '1px solid var(--glass-border)',
                }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                    <div className={`dot ${stressed ? 'dot-amber' : 'dot-green'}`} />
                    <span className="label" style={{ color: 'var(--text-secondary)' }}>{sym}</span>
                  </div>
                  <div style={{ textAlign: 'right' }}>
                    <div className="value" style={{ color: stressed ? 'var(--amber-text)' : 'var(--green-text)' }}>
                      {fmt.peg(price)}
                    </div>
                    <div className="label">{dev >= 0 ? '+' : ''}{dev.toFixed(3)}%</div>
                  </div>
                </div>
              )
            })}
          </div>
        </Panel>

        <Panel title="Aave V3" style={{ minHeight: 140 }}>
          <AaveStatus />
        </Panel>
      </div>

      {/* Pool grid */}
      <Panel title="Pool States">
        {pool_states.length === 0 ? (
          <div className="label" style={{ textAlign: 'center', padding: '20px 0' }}>
            Fetching pool states…
          </div>
        ) : (
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))', gap: 6 }}>
            {pool_states.map(p => {
              const pct = p.imbalance_ratio * 100
              const hot = pct > 5
              return (
                <div
                  key={p.address}
                  className="glass-inset"
                  style={{
                    padding:     '8px 10px',
                    borderColor: pct > 15 ? 'rgba(124,94,42,0.45)'
                               : hot      ? 'rgba(122,154,74,0.22)'
                               : undefined,
                  }}
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6 }}>
                    <span style={{ fontSize: 11, color: 'var(--text-primary)', fontWeight: 500 }}>{p.name}</span>
                    {!p.is_healthy && <span style={{ color: 'var(--red-text)', fontSize: 10 }}>⚠ unhealthy</span>}
                  </div>
                  {/* Balance bar */}
                  <div className="glass-inset" style={{ height: 3, display: 'flex', overflow: 'hidden', marginBottom: 6, padding: 0 }}>
                    {p.balances_norm.map((b, i) => (
                      <div key={i} style={{
                        flex:       b / Math.max(p.total_norm, 1),
                        background: i === 0 ? 'rgba(168,196,112,0.65)' : 'rgba(255,255,255,0.18)',
                        transition: 'flex 0.5s',
                      }} />
                    ))}
                  </div>
                  <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                    <span className="label" style={{ color: hot ? 'var(--olive-text)' : undefined }}>
                      Δ {pct.toFixed(2)}%
                    </span>
                    <span className="label">A={p.a_parameter}</span>
                  </div>
                </div>
              )
            })}
          </div>
        )}
      </Panel>

      {/* Feed row */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
        <Panel title="Opportunity Feed">
          <OpportunityFeed />
        </Panel>

        <Panel title="Recent Trades">
          <div style={{ display: 'flex', flexDirection: 'column', gap: 4, maxHeight: 280, overflowY: 'auto' }}>
            {recent_txs.length === 0 ? (
              <div style={{ textAlign: 'center', padding: '32px 0' }}>
                <span className="label">No trades yet</span>
              </div>
            ) : (
              [...recent_txs].reverse().map(tx => (
                <div
                  key={tx.id}
                  className="glass-inset"
                  style={{ padding: '7px 10px', display: 'flex', justifyContent: 'space-between' }}
                >
                  <div>
                    <div style={{ fontSize: 11, color: tx.success ? 'var(--green-text)' : 'var(--red-text)' }}>
                      {tx.success ? '✓ Landed' : '✗ Reverted'}
                    </div>
                    {tx.tx_hash && (
                      <div className="label">{tx.tx_hash.slice(0, 20)}…</div>
                    )}
                    {tx.revert_reason && (
                      <div className="label" style={{ color: 'var(--red-text)' }}>{tx.revert_reason}</div>
                    )}
                  </div>
                  <div style={{ textAlign: 'right' }}>
                    {tx.profit_usd != null && (
                      <div className="value" style={{ color: 'var(--green-text)', fontSize: 12 }}>
                        {fmt.usd(tx.profit_usd)}
                      </div>
                    )}
                    <div className="label">{fmt.ago(tx.submitted_at)}</div>
                  </div>
                </div>
              ))
            )}
          </div>
        </Panel>
      </div>
    </div>
  )
}
