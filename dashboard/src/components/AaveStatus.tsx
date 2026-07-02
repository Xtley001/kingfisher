import { useBotStore } from '../stores/botStore'
import { fmt }          from '../utils/formatters'

export function AaveStatus() {
  const { aave_status } = useBotStore()
  if (!aave_status) return <span className="label">Loading…</span>

  const { available_liquidity, borrow_cap, reserve_active, last_updated_block } = aave_status
  const utilPct = borrow_cap > 0
    ? ((borrow_cap - available_liquidity) / borrow_cap) * 100
    : 0

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      {/* Status */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <div className={`dot ${reserve_active ? 'dot-green' : 'dot-red'}`} />
        <span className="value" style={{
          color: reserve_active ? 'var(--green-text)' : 'var(--red-text)',
          fontSize: 12,
        }}>
          {reserve_active ? 'Reserve Active' : '⚠ Reserve Inactive'}
        </span>
      </div>

      {/* Liquidity */}
      <div style={{ display: 'flex', justifyContent: 'space-between' }}>
        <div>
          <div className="label" style={{ marginBottom: 2 }}>Available</div>
          <div className="value" style={{ fontSize: 13 }}>
            {fmt.aaveUsd(available_liquidity)}
          </div>
        </div>
        <div style={{ textAlign: 'right' }}>
          <div className="label" style={{ marginBottom: 2 }}>Borrow Cap</div>
          <div className="value" style={{ fontSize: 13 }}>
            {fmt.aaveUsd(borrow_cap)}
          </div>
        </div>
      </div>

      {/* Utilisation bar */}
      <div className="glass-inset" style={{ height: 4, overflow: 'hidden' }}>
        <div style={{
          height:     '100%',
          width:      `${Math.min(utilPct, 100)}%`,
          background: utilPct > 85
            ? 'var(--amber-text)'
            : 'linear-gradient(90deg, var(--olive-mid), var(--olive-bright))',
          transition: 'width 1s ease',
          borderRadius: 2,
        }} />
      </div>
      <div style={{ display: 'flex', justifyContent: 'space-between' }}>
        <span className="label">Utilisation {utilPct.toFixed(1)}%</span>
        <span className="label">Block #{last_updated_block.toLocaleString()}</span>
      </div>
    </div>
  )
}
