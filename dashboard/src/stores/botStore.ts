import { create } from 'zustand'

export interface SlippageModelParams {
  depth_base:        number
  depth_shallow:     number
  time_drift_rate:   number
  time_drift_cap:    number
  size_ratio_weight: number
  hard_cap:          number
}

export interface BotParams {
  min_profit_usd:                 number
  min_gas_roi:                    number
  min_imbalance_pct:              number
  min_velocity:                   number
  gas_reserve_eth:                number
  alert_gas_eth:                  number
  abs_cap_usd:                    number
  gas_limit_override:             number
  timeboost_min_profit_usd:       number
  timeboost_race_loss_threshold:  number
  stress_priority_fee_multiplier: number
  gas_limit_2hop:                 number
  gas_limit_4hop:                 number
  calldata_cache_enabled:         boolean
  presigned_pool_enabled:         boolean
  flash_source_preference:        'aave_only' | 'balancer_preferred'
  slippage_model:                 SlippageModelParams
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

import { getApiKey } from '../utils/auth'

const API_URL = import.meta.env.VITE_API_URL ?? 'http://localhost:3001'

const DEFAULT_AAVE: AaveStatus = {
  available_liquidity: 0,
  borrow_cap:          0,
  reserve_active:      false,
  last_updated_block:  0,
}

const DEFAULT_SLIPPAGE: SlippageModelParams = {
  depth_base:        0.003,
  depth_shallow:     0.012,
  time_drift_rate:   0.0002,
  time_drift_cap:    0.005,
  size_ratio_weight: 0.02,
  hard_cap:          0.03,
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
    min_profit_usd:                 75,
    min_gas_roi:                    3.0,
    min_imbalance_pct:              5,
    min_velocity:                   0.015,
    gas_reserve_eth:                0.10,
    alert_gas_eth:                  0.30,
    abs_cap_usd:                    25_000_000,
    gas_limit_override:             750_000,
    timeboost_min_profit_usd:       75,
    timeboost_race_loss_threshold:  0.25,
    stress_priority_fee_multiplier: 0.25,
    gas_limit_2hop:                 350_000,
    gas_limit_4hop:                 750_000,
    calldata_cache_enabled:         true,
    presigned_pool_enabled:         true,
    flash_source_preference:        'balancer_preferred',
    slippage_model:                 DEFAULT_SLIPPAGE,
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
    const apiKey = getApiKey()
    const res = await fetch(`${API_URL}/api/params`, {
      method:  'PATCH',
      headers: { 'Content-Type': 'application/json', 'X-Api-Key': apiKey },
      body:    JSON.stringify(updates),
    })
    if (res.ok) {
      set(prev => ({ params: { ...prev.params, ...updates } }))
    }
  },

  sendCommand: async cmd => {
    const apiKey = getApiKey()
    await fetch(`${API_URL}/api/command`, {
      method:  'POST',
      headers: { 'Content-Type': 'application/json', 'X-Api-Key': apiKey },
      body:    JSON.stringify({ command: cmd }),
    })
  },

  triggerWithdrawal: async () => {
    const apiKey = getApiKey()
    await fetch(`${API_URL}/api/withdraw`, {
      method:  'POST',
      headers: { 'X-Api-Key': apiKey },
    })
  },
}))
