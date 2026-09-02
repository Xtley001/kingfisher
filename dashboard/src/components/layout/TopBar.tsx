import { useState } from 'react'
import { useBotStore } from '../../stores/botStore'
import { NetworkBadge } from '../NetworkBadge'
import { KillSwitch }   from '../KillSwitch'
import { fmt }           from '../../utils/formatters'
import { getApiKey, setApiKey } from '../../utils/auth'

export function TopBar() {
  const {
    last_block, last_block_at, eth_price_usd,
    connected, triggerWithdrawal, network,
  } = useBotStore()

  const [withdrawing, setWithdrawing] = useState(false)
  const [withdrawConfirm, setWithdrawConfirm] = useState(false)
  const [showKeyModal, setShowKeyModal] = useState(false)
  const [keyInput, setKeyInput] = useState(getApiKey())

  const handleWithdraw = async () => {
    if (!withdrawConfirm) { setWithdrawConfirm(true); return }
    setWithdrawing(true)
    await triggerWithdrawal()
    setWithdrawing(false)
    setWithdrawConfirm(false)
  }

  const handleSaveKey = (e: React.FormEvent) => {
    e.preventDefault()
    setApiKey(keyInput, true)
    setShowKeyModal(false)
    window.location.reload()
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
      position:       'relative',
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

      {/* Right: API key + withdraw + kill switch */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <button
          className="btn btn-ghost"
          style={{ fontSize: 11, padding: '4px 8px' }}
          onClick={() => { setKeyInput(getApiKey()); setShowKeyModal(true) }}
          title="Configure API Key (stored in browser, never baked into public bundle)"
        >
          🔑 Key
        </button>

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

      {/* API Key Modal */}
      {showKeyModal && (
        <div style={{
          position: 'fixed',
          top: 0, left: 0, right: 0, bottom: 0,
          background: 'rgba(0,0,0,0.6)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          zIndex: 9999,
        }}>
          <form onSubmit={handleSaveKey} style={{
            background: 'var(--bg-secondary)',
            border: '1px solid var(--glass-border)',
            borderRadius: 8,
            padding: 20,
            width: 380,
            display: 'flex',
            flexDirection: 'column',
            gap: 12,
            boxShadow: '0 8px 32px rgba(0,0,0,0.4)',
          }}>
            <h3 style={{ margin: 0, fontSize: 14 }}>🔑 API Key Configuration</h3>
            <p style={{ margin: 0, fontSize: 11, color: 'var(--text-muted)' }}>
              Set your bot API key. Stored locally in your browser storage so it is never compiled into the public client bundle.
            </p>
            <input
              type="password"
              value={keyInput}
              onChange={e => setKeyInput(e.target.value)}
              placeholder="Enter API_KEY"
              style={{
                background: 'var(--bg-primary)',
                border: '1px solid var(--glass-border)',
                borderRadius: 4,
                padding: '8px 10px',
                color: 'var(--text-primary)',
                fontFamily: 'monospace',
                fontSize: 12,
              }}
              autoFocus
            />
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 4 }}>
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() => setShowKeyModal(false)}
              >
                Cancel
              </button>
              <button type="submit" className="btn btn-olive">
                Save & Connect
              </button>
            </div>
          </form>
        </div>
      )}
    </header>
  )
}
