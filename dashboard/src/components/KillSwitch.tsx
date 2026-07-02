import { useState } from 'react'
import { useBotStore } from '../stores/botStore'

export function KillSwitch() {
  const { running, sendCommand } = useBotStore()
  const [confirm, setConfirm]   = useState(false)
  const [busy, setBusy]         = useState(false)

  const handlePause = async () => {
    if (!confirm) { setConfirm(true); return }
    setBusy(true)
    await sendCommand('pause')
    setConfirm(false)
    setBusy(false)
  }

  const handleResume = async () => {
    setBusy(true)
    await sendCommand('resume')
    setBusy(false)
  }

  if (!running) {
    return (
      <button
        className="btn btn-olive"
        onClick={handleResume}
        disabled={busy}
      >
        {busy ? '…' : '▶ Resume'}
      </button>
    )
  }

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
      {confirm && (
        <span className="label" style={{ color: 'var(--amber-text)' }}>
          Confirm pause?
        </span>
      )}
      <button
        className={`btn ${confirm ? 'btn-confirm' : 'btn-danger'}`}
        onClick={handlePause}
        disabled={busy}
        onBlur={() => setConfirm(false)}
      >
        {busy ? '…' : confirm ? '⏸ Confirm' : '⏸ Pause'}
      </button>
      {confirm && (
        <button
          className="btn btn-ghost"
          onClick={() => setConfirm(false)}
        >
          Cancel
        </button>
      )}
    </div>
  )
}
