import { useEffect, useRef } from 'react'
import { useBotStore } from '../stores/botStore'

const WS_URL = import.meta.env.VITE_WS_URL ?? 'ws://localhost:3001/ws'
const API_KEY = import.meta.env.VITE_API_KEY ?? ''

// MED-02: The browser WebSocket API does not allow setting arbitrary HTTP headers
// during the initial Upgrade handshake. The standard workaround is to pass the
// credential as the Sec-WebSocket-Protocol value (subprotocol field), which IS
// forwarded in the Upgrade request headers and is not logged by default by
// nginx or a reverse proxy. The server reads it from the Connection: Upgrade headers
// via the X-Api-Key field set during the HTTP phase of the upgrade.
//
// Implementation: we pass the key as the WebSocket subprotocol string.
// The Axum handler on the server side reads it from headers.get("sec-websocket-protocol").
// This keeps the key out of access logs (unlike ?key= query params).
export function useWebSocket() {
  const wsRef        = useRef<WebSocket | null>(null)
  const retryDelay   = useRef(1000)
  const retryTimer   = useRef<ReturnType<typeof setTimeout> | null>(null)
  const unmounted    = useRef(false)
  const updateStore  = useBotStore(s => s.updateFromServer)
  const setConnected = useBotStore(s => s.setConnected)

  const connect = () => {
    if (unmounted.current) return

    // Pass key as subprotocol — this header is included in the Upgrade request
    // but is NOT written to standard access logs (unlike URL query params).
    const ws = new WebSocket(WS_URL, [`kingfisher-v1`, API_KEY])
    wsRef.current = ws

    ws.onopen = () => {
      retryDelay.current = 1000
      setConnected(true)
    }

    ws.onmessage = e => {
      try {
        const msg = JSON.parse(e.data as string)
        updateStore(msg)
      } catch { /* ignore parse errors */ }
    }

    ws.onclose = () => {
      setConnected(false)
      if (!unmounted.current) {
        retryTimer.current = setTimeout(() => {
          retryDelay.current = Math.min(retryDelay.current * 1.5, 30_000)
          connect()
        }, retryDelay.current)
      }
    }

    ws.onerror = () => ws.close()
  }

  useEffect(() => {
    unmounted.current = false
    connect()
    return () => {
      unmounted.current = true
      if (retryTimer.current) clearTimeout(retryTimer.current)
      wsRef.current?.close()
    }
  }, [])
}
