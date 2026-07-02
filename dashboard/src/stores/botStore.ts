import { create } from 'zustand'

export interface BotParams {
  min_profit_usd:    number
  min_imbalance_pct: number
  min_velocity:      number
  gas_reserve_eth:   number
  alert_gas_eth:     number
  abs_cap_usd:       number
}

export interface AaveStatus {
  available_liquidity: number
  borrow_cap:          number
  reserve_active:      boolean
  last_updated_block:  number
}

export interface PoolState {
  address:         string
  name:            string
  imbalance_ratio: number
  a_parameter:     number
  virtual_price:   number
  is_healthy:      boolean
  balances_norm:   number[]
  total_norm:      number
  velocity:        number
}

export interface Opportunity {
  id:                   string
  route_description:    string
  estimated_profit_usd: number
  simulated_profit_usd: number | null
  edge_trigger:         string | null
  detected_at:          string
  fired:                boolean
}

export interface TxResult {
  id:            string
  tx_hash:       string | null
  success:       boolean
  profit_usd:    number | null
  block_target:  number
  submitted_at:  string
  revert_reason: string | null
}

interface Store {
  connected:        boolean
  running:          boolean
  network:          'Testnet' | 'Mainnet'
  uptime_secs:      number
  last_block:       number
  last_block_at:    string | null
  eth_price_usd:    number
  usdc_peg:         number
  usdt_peg:         number
  stress_regime:    boolean
  wallet_eth_balance: number
  gas_regime:       'Normal' | 'Alert' | 'Critical'
  aave_status:      AaveStatus
  total_profit_usd: number
  today_profit_usd: number
  total_trades:     number
  today_trades:     number
  win_rate:         number
  consecutive_reverts: number
  recent_opps:      Opportunity[]
  recent_txs:       TxResult[]
  pool_states:      PoolState[]
  params:           BotParams

  setConnected:     (c: boolean) => void
  updateFromServer: (data: any) => void
  updateParams:     (p: Partial<BotParams>) => Promise<void>
  sendCommand:      (cmd: 'pause' | 'resume') => Promise<void>
  triggerWithdrawal: () => Promise<void>
}

const API_URL = import.meta.env.VITE_API_URL ?? 'http://localhost:3001'
const API_KEY = import.meta.env.VITE_API_KEY ?? ''

const DEFAULT_AAVE: AaveStatus = {
  available_liquidity: 0,
  borrow_cap:          0,
  reserve_active:      false,
  last_updated_block:  0,
}

export const useBotStore = create<Store>((set, get) => ({
  connected:        false,
  running:          false,
  network:          'Testnet',
  uptime_secs:      0,
  last_block:       0,
  last_block_at:    null,
  eth_price_usd:    0,
  usdc_peg:         1.0,
  usdt_peg:         1.0,
  stress_regime:    false,
  wallet_eth_balance: 0,
  gas_regime:       'Normal',
  aave_status:      DEFAULT_AAVE,
  total_profit_usd: 0,
  today_profit_usd: 0,
  total_trades:     0,
  today_trades:     0,
  win_rate:         0,
  consecutive_reverts: 0,
  recent_opps:      [],
  recent_txs:       [],
  pool_states:      [],
  params: {
    min_profit_usd:    75,
    min_imbalance_pct: 5,
    min_velocity:      0.015,
    gas_reserve_eth:   0.10,
    alert_gas_eth:     0.30,
    abs_cap_usd:       100_000_000,
  },

  setConnected: c => set({ connected: c }),

  updateFromServer: data => set(prev => ({
    ...prev,
    ...data,
    // Pool states come as HashMap in Rust — convert to array for dashboard
    pool_states: data.pool_states
      ? Object.values(data.pool_states as Record<string, any>).map((ps: any) => ({
          address:         ps.address,
          name:            ps.name,
          imbalance_ratio: ps.balances_norm?.length
            ? Math.max(...ps.balances_norm.map((b: number) => Math.abs(b / ps.total_norm - 1 / ps.balances_norm.length)))
            : 0,
          a_parameter:     ps.a_parameter,
          virtual_price:   ps.virtual_price,
          is_healthy:      ps.virtual_price >= 1e18,
          balances_norm:   ps.balances_norm ?? [],
          total_norm:      ps.total_norm ?? 0,
          velocity:        ps.balance_history?.length >= 2 ? 0 : 0,
        }))
      : prev.pool_states,
    recent_opps: data.recent_opps ?? prev.recent_opps,
    recent_txs:  data.recent_txs  ?? prev.recent_txs,
    aave_status: data.aave_status ?? prev.aave_status,
  })),

  updateParams: async updates => {
    const res = await fetch(`${API_URL}/api/params`, {
      method:  'PATCH',
      headers: { 'Content-Type': 'application/json', 'X-Api-Key': API_KEY },
      body:    JSON.stringify(updates),
    })
    if (res.ok) {
      set(prev => ({ params: { ...prev.params, ...updates } }))
    }
  },

  sendCommand: async cmd => {
    await fetch(`${API_URL}/api/command`, {
      method:  'POST',
      headers: { 'Content-Type': 'application/json', 'X-Api-Key': API_KEY },
      body:    JSON.stringify({ command: cmd }),
    })
  },

  triggerWithdrawal: async () => {
    await fetch(`${API_URL}/api/withdraw`, {
      method:  'POST',
      headers: { 'X-Api-Key': API_KEY },
    })
  },
}))
