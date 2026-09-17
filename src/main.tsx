import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import Popup from './components/Popup'
import Toast from './components/Toast'
import './index.css'

// Les trois fenêtres partagent le même bundle ; c'est la query string déclarée
// dans tauri.conf.json qui les distingue.
const view = new URLSearchParams(window.location.search).get('view')

const root = ReactDOM.createRoot(document.getElementById('root')!)

// Le mode strict monte les composants deux fois en développement, ce qui
// dupliquerait l'abonnement aux événements Tauri des fenêtres secondaires.
root.render(
  view === 'popup' ? (
    <Popup />
  ) : view === 'toast' ? (
    <Toast />
  ) : (
    <React.StrictMode>
      <App />
    </React.StrictMode>
  ),
)
