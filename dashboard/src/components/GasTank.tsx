import { useBotStore } from '../stores/botStore'
import { fmt }          from '../utils/formatters'

export function GasTank() {
  const { wallet_eth_balance, gas_regime, params } = useBotStore()

  const fillPct = Math.min(
    (wallet_eth_balance / 1.0) * 100,  // 1.0 ETH = full tank
    100
  )

  const color = gas_regime === 'Critical' ? 'var(--red-text)'
              : gas_regime === 'Alert'    ? 'var(--amber-text)'
              : 'var(--green-text)'

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
      {/* Balance */}
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline' }}>
        <span className="value-lg" style={{ color }}>
          {fmt.eth(wallet_eth_balance)}
        </span>
        <span className="label" style={{ color }}>
          {gas_regime === 'Critical' ? '⛽ Critical — Halted'
          : gas_regime === 'Alert'   ? '⛽ Low — Refill Soon'
          : '⛽ OK'}
        </span>
      </div>

      {/* Bar */}
      <div className="glass-inset" style={{ height: 6, overflow: 'hidden' }}>
        <div style={{
          height:     '100%',
          width:      `${fillPct}%`,
          background: `linear-gradient(90deg, ${color}aa, ${color})`,
          transition: 'width 1s ease',
          borderRadius: 2,
        }} />
      </div>

      {/* Thresholds */}
      <div style={{ display: 'flex', justifyContent: 'space-between' }}>
        <span className="label">Alert @ {params.alert_gas_eth} ETH</span>
        <span className="label">Halt @ {params.gas_reserve_eth} ETH</span>
      </div>
    </div>
  )
}
