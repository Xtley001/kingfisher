import { Routes, Route } from 'react-router-dom'
import { Shell }       from './components/layout/Shell'
import { Dashboard }   from './pages/Dashboard'
import { Parameters }  from './pages/Parameters'
import { History }     from './pages/History'
import { useWebSocket } from './hooks/useWebSocket'

export default function App() {
  useWebSocket()
  return (
    <Shell>
      <Routes>
        <Route path="/"           element={<Dashboard />} />
        <Route path="/parameters" element={<Parameters />} />
        <Route path="/history"    element={<History />} />
      </Routes>
    </Shell>
  )
}
