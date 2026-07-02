import { useBotStore } from '../stores/botStore'

export function NetworkBadge() {
  const network = useBotStore(s => s.network)
  const isMain  = network === 'Mainnet'
  return (
    <div style={{
      display:      'inline-flex',
      alignItems:   'center',
      gap:          5,
      padding:      '3px 9px',
      borderRadius: 'var(--r-xs)',
      border:       `1px solid ${isMain ? 'rgba(114,184,120,0.40)' : 'rgba(212,164,114,0.40)'}`,
      background:   isMain ? 'rgba(74,124,78,0.10)' : 'rgba(124,94,42,0.10)',
      fontSize:     10,
      fontWeight:   600,
      letterSpacing:'0.12em',
      textTransform:'uppercase',
      color:        isMain ? 'var(--green-text)' : 'var(--amber-text)',
      userSelect:   'none',
    }}>
      <div className={`dot ${isMain ? 'dot-green' : 'dot-amber'}`} />
      {isMain ? 'Mainnet' : 'Testnet'}
    </div>
  )
}
