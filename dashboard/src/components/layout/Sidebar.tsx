import { useState } from 'react'
import { NavLink, useLocation } from 'react-router-dom'

const NAV = [
  { path: '/',           icon: '◈', label: 'Dashboard' },
  { path: '/parameters', icon: '⚙', label: 'Parameters' },
  { path: '/history',    icon: '▤', label: 'History' },
]

export function Sidebar() {
  const [expanded, setExpanded] = useState(true)
  const location = useLocation()

  return (
    <nav style={{
      width:          expanded ? 160 : 44,
      minWidth:       expanded ? 160 : 44,
      background:     'var(--bg-secondary)',
      borderRight:    '1px solid var(--glass-border)',
      display:        'flex',
      flexDirection:  'column',
      transition:     'width 0.18s ease, min-width 0.18s ease',
      overflow:       'hidden',
      flexShrink:     0,
    }}>
      {/* Logo / collapse toggle */}
      <div
        style={{
          padding:      '14px 12px',
          display:      'flex',
          alignItems:   'center',
          gap:          8,
          cursor:       'pointer',
          borderBottom: '1px solid var(--glass-border)',
          userSelect:   'none',
        }}
        onClick={() => setExpanded(e => !e)}
        title={expanded ? 'Collapse sidebar' : 'Expand sidebar'}
      >
        <span style={{ color: 'var(--olive-text)', fontSize: 16, flexShrink: 0 }}>🦅</span>
        {expanded && (
          <span style={{
            color:       'var(--olive-text)',
            fontWeight:  600,
            fontSize:    12,
            letterSpacing: '0.08em',
            whiteSpace:  'nowrap',
          }}>
            KINGFISHER
          </span>
        )}
      </div>

      {/* Nav links */}
      <div style={{ flex: 1, paddingTop: 8 }}>
        {NAV.map(({ path, icon, label }) => {
          const active = location.pathname === path
          return (
            <NavLink
              key={path}
              to={path}
              style={{
                display:     'flex',
                alignItems:  'center',
                gap:         10,
                padding:     expanded ? '9px 14px' : '9px 0',
                justifyContent: expanded ? 'flex-start' : 'center',
                textDecoration: 'none',
                background:  active ? 'rgba(122,154,74,0.08)' : 'transparent',
                borderLeft:  `2px solid ${active ? 'var(--olive-bright)' : 'transparent'}`,
                color:       active ? 'var(--olive-text)' : 'var(--text-secondary)',
                transition:  'background 0.12s, color 0.12s',
              }}
            >
              <span style={{ fontSize: 13, flexShrink: 0 }}>{icon}</span>
              {expanded && (
                <span style={{
                  fontSize:    11,
                  fontWeight:  active ? 600 : 400,
                  letterSpacing: '0.06em',
                  whiteSpace:  'nowrap',
                }}>
                  {label}
                </span>
              )}
            </NavLink>
          )
        })}
      </div>

      {/* Collapse button at bottom */}
      <div style={{ padding: '10px 0', borderTop: '1px solid var(--glass-border)', textAlign: 'center' }}>
        <button
          className="btn btn-ghost"
          style={{ padding: '4px 8px', fontSize: 11, width: expanded ? 'calc(100% - 20px)' : 28 }}
          onClick={() => setExpanded(e => !e)}
        >
          {expanded ? '◂ Collapse' : '▸'}
        </button>
      </div>
    </nav>
  )
}
