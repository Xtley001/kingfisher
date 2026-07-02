import { useBotStore } from '../stores/botStore'
import { fmt }          from '../utils/formatters'

export function OpportunityFeed() {
  const opps = useBotStore(s => s.recent_opps)

  if (opps.length === 0) {
    return (
      <div style={{ textAlign: 'center', padding: '32px 0' }}>
        <div className="label" style={{ marginBottom: 6 }}>Scanning…</div>
        <div className="label" style={{ color: 'var(--text-muted)' }}>
          Opportunities will appear here
        </div>
      </div>
    )
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4, maxHeight: 280, overflowY: 'auto' }}>
      {[...opps].reverse().map(opp => (
        <div
          key={opp.id}
          className="glass-inset"
          style={{
            padding:      '7px 10px',
            display:      'flex',
            justifyContent: 'space-between',
            gap:          12,
            borderColor:  opp.edge_trigger ? 'rgba(122,154,74,0.25)' : undefined,
          }}
        >
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{
              fontSize:     11,
              color:        'var(--text-primary)',
              overflow:     'hidden',
              textOverflow: 'ellipsis',
              whiteSpace:   'nowrap',
            }}>
              {opp.fired && <span style={{ color: 'var(--olive-text)', marginRight: 5 }}>▶</span>}
              {opp.route_description}
            </div>
            {opp.edge_trigger && (
              <div className="label" style={{ color: 'var(--olive-text)', marginTop: 2 }}>
                ◈ {opp.edge_trigger}
              </div>
            )}
          </div>
          <div style={{ textAlign: 'right', flexShrink: 0 }}>
            <div className="value" style={{ color: 'var(--green-text)', fontSize: 12 }}>
              {fmt.usd(opp.simulated_profit_usd ?? opp.estimated_profit_usd)}
            </div>
            <div className="label">{fmt.ago(opp.detected_at)}</div>
          </div>
        </div>
      ))}
    </div>
  )
}
