import { useBotStore } from '../stores/botStore'
import { fmt }          from '../utils/formatters'

export function History() {
  const txs = useBotStore(s => s.recent_txs)

  return (
    <div style={{ maxWidth: 900 }}>
      <div className="glass-card" style={{ overflow: 'hidden' }}>
        {/* Header */}
        <div style={{
          padding:      '10px 14px',
          borderBottom: '1px solid var(--glass-border)',
          display:      'flex',
          justifyContent: 'space-between',
        }}>
          <span className="label">Transaction History (last 100)</span>
          <span className="label">{txs.length} records</span>
        </div>

        {txs.length === 0 ? (
          <div style={{ padding: '48px 0', textAlign: 'center' }}>
            <div className="label" style={{ marginBottom: 6 }}>No transaction history yet</div>
            <div className="label" style={{ color: 'var(--text-muted)' }}>
              Trades will appear here once the bot executes
            </div>
          </div>
        ) : (
          <>
            {/* Column headers */}
            <div style={{
              display:     'grid',
              gridTemplateColumns: '2fr 3fr 1fr 1fr 1fr',
              padding:     '6px 14px',
              borderBottom:'1px solid var(--glass-border)',
            }}>
              {['Time', 'Hash / Reason', 'Block', 'Profit', 'Status'].map(h => (
                <span key={h} className="label">{h}</span>
              ))}
            </div>

            {/* Rows */}
            {[...txs].reverse().map(tx => (
              <div
                key={tx.id}
                style={{
                  display:     'grid',
                  gridTemplateColumns: '2fr 3fr 1fr 1fr 1fr',
                  padding:     '8px 14px',
                  borderBottom:'1px solid rgba(255,255,255,0.03)',
                  alignItems:  'center',
                }}
              >
                <span className="label">{fmt.ago(tx.submitted_at)}</span>
                <span style={{
                  fontSize:     11,
                  color:        'var(--text-secondary)',
                  overflow:     'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace:   'nowrap',
                }}>
                  {tx.tx_hash
                    ? `${tx.tx_hash.slice(0, 24)}…`
                    : tx.revert_reason ?? '—'}
                </span>
                <span className="label">{tx.block_target.toLocaleString()}</span>
                <span className="value" style={{
                  fontSize: 11,
                  color:    tx.profit_usd ? 'var(--green-text)' : 'var(--text-muted)',
                }}>
                  {tx.profit_usd != null ? fmt.usd(tx.profit_usd) : '—'}
                </span>
                <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
                  <div className={`dot ${tx.success ? 'dot-green' : 'dot-red'}`} />
                  <span className="label" style={{
                    color: tx.success ? 'var(--green-text)' : 'var(--red-text)',
                  }}>
                    {tx.success ? 'Landed' : 'Reverted'}
                  </span>
                </div>
              </div>
            ))}
          </>
        )}
      </div>
    </div>
  )
}
