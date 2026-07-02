import { ReactNode } from 'react'
import { Sidebar } from './Sidebar'
import { TopBar }  from './TopBar'

export function Shell({ children }: { children: ReactNode }) {
  return (
    <div style={{
      display:    'flex',
      height:     '100dvh',
      overflow:   'hidden',
      background: 'var(--bg-void)',
    }}>
      <Sidebar />
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
        <TopBar />
        <main style={{
          flex:      1,
          overflowY: 'auto',
          padding:   '14px 16px',
        }}>
          {children}
        </main>
      </div>
    </div>
  )
}
