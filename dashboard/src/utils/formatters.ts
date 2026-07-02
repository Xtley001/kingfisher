export const fmt = {
  usd: (n: number): string => {
    if (n >= 1_000_000) return `$${(n / 1_000_000).toFixed(2)}M`
    if (n >= 1_000)     return `$${(n / 1_000).toFixed(1)}k`
    return `$${n.toFixed(2)}`
  },
  usdSigned: (n: number): string =>
    `${n >= 0 ? '+' : ''}${fmt.usd(n)}`,
  eth:   (n: number): string => `${n.toFixed(4)} ETH`,
  peg:   (n: number): string => `$${n.toFixed(5)}`,
  pct:   (n: number): string => `${(n * 100).toFixed(1)}%`,
  block: (n: number): string => `#${n.toLocaleString()}`,
  uptime: (s: number): string => {
    const h = Math.floor(s / 3600)
    const m = Math.floor((s % 3600) / 60)
    const sec = s % 60
    if (h > 0) return `${h}h ${m}m`
    if (m > 0) return `${m}m ${sec}s`
    return `${sec}s`
  },
  ago: (ts: string | null | undefined): string => {
    if (!ts) return '—'
    const ms = Date.now() - new Date(ts).getTime()
    if (ms < 5_000)    return 'just now'
    if (ms < 60_000)   return `${Math.floor(ms / 1_000)}s ago`
    if (ms < 3_600_000) return `${Math.floor(ms / 60_000)}m ago`
    return `${Math.floor(ms / 3_600_000)}h ago`
  },
  shortAddr: (addr: string): string =>
    addr ? `${addr.slice(0, 6)}…${addr.slice(-4)}` : '—',
  aaveUsd: (wei: number): string => fmt.usd(wei / 1e6),
}
