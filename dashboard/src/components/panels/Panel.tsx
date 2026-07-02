import { useState, useRef, useEffect, CSSProperties, ReactNode } from 'react'
import { createPortal } from 'react-dom'

interface PanelProps {
  title:    string
  children: ReactNode
  style?:   CSSProperties
  defaultCollapsed?: boolean
}

export function Panel({ title, children, style, defaultCollapsed = false }: PanelProps) {
  const [collapsed,  setCollapsed]  = useState(defaultCollapsed)
  const [fullscreen, setFullscreen] = useState(false)

  // ESC exits fullscreen
  useEffect(() => {
    if (!fullscreen) return
    const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') setFullscreen(false) }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [fullscreen])

  const header = (
    <div style={{
      display:        'flex',
      justifyContent: 'space-between',
      alignItems:     'center',
      padding:        '8px 12px',
      borderBottom:   collapsed ? 'none' : '1px solid var(--glass-border)',
      cursor:         'pointer',
      userSelect:     'none',
    }}>
      <span
        className="label"
        style={{ color: 'var(--text-secondary)', letterSpacing: '0.10em' }}
        onClick={() => setCollapsed(c => !c)}
      >
        {collapsed ? '▶' : '▼'} {title}
      </span>
      <div style={{ display: 'flex', gap: 4 }}>
        <button
          className="btn btn-ghost"
          style={{ padding: '2px 7px', fontSize: 10 }}
          onClick={e => { e.stopPropagation(); setFullscreen(f => !f) }}
          title="Fullscreen (ESC to exit)"
        >⛶</button>
      </div>
    </div>
  )

  const body = collapsed ? null : (
    <div style={{ padding: '10px 12px' }}>
      {children}
    </div>
  )

  const panelContent = (
    <div className="glass-card" style={style}>
      {header}
      {body}
    </div>
  )

  if (fullscreen) {
    return (
      <>
        {/* Non-fullscreen placeholder so layout doesn't jump */}
        <div className="glass-card" style={{ ...style, opacity: 0.3 }}>
          {header}
        </div>
        {createPortal(
          <div style={{
            position:   'fixed',
            inset:      0,
            zIndex:     9999,
            background: 'rgba(8,10,8,0.94)',
            display:    'flex',
            flexDirection: 'column',
            padding:    '16px',
            backdropFilter: 'blur(8px)',
          }}>
            <div className="glass-card" style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
              <div style={{
                display:        'flex',
                justifyContent: 'space-between',
                alignItems:     'center',
                padding:        '10px 14px',
                borderBottom:   '1px solid var(--glass-border)',
              }}>
                <span className="label" style={{ color: 'var(--text-secondary)' }}>{title}</span>
                <button
                  className="btn btn-ghost"
                  onClick={() => setFullscreen(false)}
                >
                  ✕ ESC
                </button>
              </div>
              <div style={{ padding: '12px 14px', flex: 1, overflowY: 'auto' }}>
                {children}
              </div>
            </div>
          </div>,
          document.body
        )}
      </>
    )
  }

  return panelContent
}
