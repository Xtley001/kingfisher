import { useState } from 'react'
import { useBotStore } from '../../stores/botStore'
import { NetworkBadge } from '../NetworkBadge'
import { KillSwitch }   from '../KillSwitch'
import { fmt }           from '../../utils/formatters'

export function TopBar() {
  const {
    last_block, last_block_at, eth_price_usd,
    connected, triggerWithdrawal, network,
  } = useBotStore()

  const [withdrawing, setWithdrawing] = useState(false)
  const [withdrawConfirm, setWithdrawConfirm] = useState(false)

  const handleWithdraw = async () => {
    if (!withdrawConfirm) { setWithdrawConfirm(true); return }
    setWithdrawing(true)
    await triggerWithdrawal()
    setWithdrawing(false)
    setWithdrawConfirm(false)
  }

  return (
    <header style={{
      height:         44,
      background:     'var(--bg-secondary)',
      borderBottom:   '1px solid var(--glass-border)',
      display:        'flex',
      alignItems:     'center',
      justifyContent: 'space-between',
      padding:        '0 16px',
      gap:            16,
      flexShrink:     0,
    }}>
      {/* Left: network badge + connection indicator */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <NetworkBadge />
        <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
          <div className={`dot ${connected ? 'dot-olive' : 'dot-red'}`} />
          <span className="label" style={{
            color: connected ? 'var(--olive-text)' : 'var(--red-text)',
          }}>
            {connected ? 'Live' : 'Disconnected'}
          </span>
        </div>
      </div>

      {/* Centre: block ticker + ETH price */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 18 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span className="label">Block</span>
          <span className="value" style={{ fontSize: 12, color: 'var(--olive-text)' }}>
            {last_block ? fmt.block(last_block) : '—'}
          </span>
          {last_block_at && (
            <span className="label" style={{ color: 'var(--text-muted)' }}>
              {fmt.ago(last_block_at)}
            </span>
          )}
        </div>
        <div style={{ width: 1, height: 14, background: 'var(--glass-border)' }} />
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span className="label">ETH</span>
          <span className="value" style={{ fontSize: 12 }}>
            {eth_price_usd > 0 ? fmt.usd(eth_price_usd) : '—'}
          </span>
        </div>
      </div>

      {/* Right: withdraw + kill switch */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        {/* Withdraw profits button — mainnet only */}
        {network === 'Mainnet' && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            {withdrawConfirm && (
              <span className="label" style={{ color: 'var(--amber-text)' }}>
                Withdraw to cold wallet?
              </span>
            )}
            <button
              className={`btn ${withdrawConfirm ? 'btn-olive' : 'btn-ghost'}`}
              style={{ fontSize: 10 }}
              onClick={handleWithdraw}
              disabled={withdrawing}
              onBlur={() => setWithdrawConfirm(false)}
            >
              {withdrawing ? '…' : withdrawConfirm ? '✓ Confirm' : '↑ Withdraw'}
            </button>
            {withdrawConfirm && (
              <button
                className="btn btn-ghost"
                style={{ fontSize: 10 }}
                onClick={() => setWithdrawConfirm(false)}
              >✕</button>
            )}
          </div>
        )}
        <KillSwitch />
      </div>
    </header>
  )
}
