import React from 'react'
import './App.css'
import MainPanel from './components/MainPanel'
import SettingsPanel from './components/SettingsPanel'
import HistoryPanel from './components/HistoryPanel'

type View = 'main' | 'settings' | 'history'

function App() {
  const [currentView, setCurrentView] = React.useState<View>('main')

  return (
    <div className="app">
      <nav className="navbar">
        <button
          className={currentView === 'main' ? 'active' : ''}
          onClick={() => setCurrentView('main')}
        >
          Transformer
        </button>
        <button
          className={currentView === 'history' ? 'active' : ''}
          onClick={() => setCurrentView('history')}
        >
          Historique
        </button>
        <button
          className={currentView === 'settings' ? 'active' : ''}
          onClick={() => setCurrentView('settings')}
        >
          Paramètres
        </button>
      </nav>

      <main className="content">
        {currentView === 'main' && <MainPanel />}
        {currentView === 'history' && <HistoryPanel />}
        {currentView === 'settings' && <SettingsPanel />}
      </main>
    </div>
  )
}

export default App
