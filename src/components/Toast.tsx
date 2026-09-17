import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import '../styles/Toast.css'

interface ToastState {
  phase: 'analyzing' | 'done'
  label: string
}

/**
 * Bandeau minuscule affiché près du texte : « Analyse… » pendant l'appel au
 * modèle, puis « Corrigé ✓ » une fois le remplacement fait.
 *
 * La fenêtre n'est jamais détruite, seulement masquée et réaffichée : une
 * animation CSS ne se rejouerait pas d'elle-même, d'où la clé qui change à
 * chaque état pour forcer React à remonter le nœud.
 */
export default function Toast() {
  const [state, setState] = useState<ToastState>({ phase: 'analyzing', label: 'Analyse' })
  const [generation, setGeneration] = useState(0)

  useEffect(() => {
    document.body.classList.add('toast-body')
    const unlisten = listen<ToastState>('toast-update', (event) => {
      setState(event.payload)
      setGeneration((n) => n + 1)
    })
    return () => {
      void unlisten.then((fn) => fn())
      document.body.classList.remove('toast-body')
    }
  }, [])

  return (
    <div className={`toast toast-${state.phase}`} key={generation}>
      <span className="toast-label">
        {state.label}
        {state.phase === 'analyzing' && <span className="toast-ellipsis">…</span>}
      </span>
      <span className="toast-mark">
        {state.phase === 'analyzing' ? (
          <span className="toast-spinner" />
        ) : (
          <svg viewBox="0 0 24 24" aria-label="Terminé" role="img">
            <path
              d="M5 12.5l4.5 4.5L19 7.5"
              fill="none"
              stroke="currentColor"
              strokeWidth="3.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        )}
      </span>
    </div>
  )
}
