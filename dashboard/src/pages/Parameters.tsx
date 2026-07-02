import { useState } from 'react'
import { useBotStore, BotParams } from '../stores/botStore'

type FieldDef = {
  key:   keyof BotParams
  label: string
  hint:  string
  step:  number
  unit:  string
  warn?: boolean
}

const FIELDS: FieldDef[] = [
  {
    key:   'min_profit_usd',
    label: 'Min Net Profit',
    hint:  'Net floor after Aave 5 bps fee + gas. Trades below this are skipped.',
    step:  5, unit: 'USD',
  },
  {
    key:   'min_imbalance_pct',
    label: 'Min Imbalance',
    hint:  'Pool must be this % off centre to enter the filter pipeline.',
    step:  0.5, unit: '%',
  },
  {
    key:   'min_velocity',
    label: 'Min Velocity',
    hint:  'Freshness filter — rejects slow drift already being arbitraged by others.',
    step:  0.001, unit: '',
  },
  {
    key:   'gas_reserve_eth',
    label: 'Gas Halt Floor',
    hint:  'Bot halts when wallet ETH drops below this. Operational target: 1.0 ETH.',
    step:  0.01, unit: 'ETH',
  },
  {
    key:   'alert_gas_eth',
    label: 'Gas Alert Threshold',
    hint:  'Telegram alert fires when wallet drops below this. Refill soon.',
    step:  0.05, unit: 'ETH',
  },
  {
    key:   'abs_cap_usd',
    label: 'Emergency Borrow Ceiling',
    hint:  'Last-resort cap. Borrow sizing is automatic from the spread curve — leave at $100M.',
    step:  1_000_000, unit: 'USD', warn: true,
  },
]

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

  return (
    <div style={{ maxWidth: 560, display: 'flex', flexDirection: 'column', gap: 12 }}>

      {/* Info banner */}
      <div
        className="glass"
        style={{ padding: '10px 14px', display: 'flex', gap: 10, borderColor: 'rgba(122,154,74,0.25)' }}
      >
        <span style={{ color: 'var(--olive-text)', flexShrink: 0 }}>ℹ</span>
        <p style={{ margin: 0, fontSize: 11, color: 'var(--text-secondary)', lineHeight: 1.6 }}>
          Borrow size is computed automatically each cycle from the Curve spread curve optimum —
          it is not a manual input. Changes apply on the next block scan. No bot restart needed.
        </p>
      </div>

      {/* Field list */}
      <div className="glass-card" style={{ overflow: 'hidden' }}>
        {FIELDS.map((f, idx) => (
          <div
            key={f.key}
            style={{
              padding:      '12px 14px',
              borderBottom: idx < FIELDS.length - 1 ? '1px solid var(--glass-border)' : 'none',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 5 }}>
              <label
                className="label"
                style={{ color: f.warn ? 'var(--amber-text)' : 'var(--text-secondary)' }}
                htmlFor={`param-${f.key}`}
              >
                {f.warn && '⚠ '}{f.label}{f.unit ? ` (${f.unit})` : ''}
              </label>
              <span className="value" style={{ fontSize: 12 }}>{local[f.key]}</span>
            </div>
            <input
              id={`param-${f.key}`}
              type="number"
              step={f.step}
              value={local[f.key]}
              onChange={e => setLocal(prev => ({
                ...prev,
                [f.key]: parseFloat(e.target.value) || 0,
              }))}
              style={{
                width:       '100%',
                background:  'rgba(0,0,0,0.25)',
                border:      `1px solid ${f.warn ? 'rgba(124,94,42,0.30)' : 'var(--glass-border)'}`,
                borderRadius:'var(--r-sm)',
                padding:     '6px 10px',
                color:       'var(--text-primary)',
                fontFamily:  'var(--font)',
                fontSize:    12,
                outline:     'none',
              }}
            />
            <p style={{ margin: '4px 0 0', fontSize: 10, color: 'var(--text-muted)', lineHeight: 1.5 }}>
              {f.hint}
            </p>
          </div>
        ))}
      </div>

      {/* Action row */}
      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, alignItems: 'center' }}>
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
          {saving ? 'Saving…' : 'Apply'}
        </button>
      </div>
    </div>
  )
}
