import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { listen } from '@tauri-apps/api/event'
import {
  ACTIONS,
  actionLabel,
  CapturedSelection,
  PROVIDER_LABELS,
  TransformOutcome,
} from '../types'
import '../styles/Popup.css'

type Phase = 'actions' | 'working' | 'preview' | 'error'

const EMPTY: CapturedSelection = {
  text: '',
  defaultProvider: 'claude',
  previewBeforeReplace: false,
  targetLanguage: 'anglais',
}

export default function Popup() {
  const [selection, setSelection] = useState<CapturedSelection>(EMPTY)
  const [phase, setPhase] = useState<Phase>('actions')
  const [pendingAction, setPendingAction] = useState<string>('')
  const [result, setResult] = useState('')
  const [error, setError] = useState('')

  const close = () => {
    void invoke('hide_popup')
  }

  const reset = (next: CapturedSelection) => {
    setSelection(next)
    setPhase('actions')
    setPendingAction('')
    setResult('')
    setError('')
  }

  useEffect(() => {
    document.body.classList.add('popup-body')

    // La popup est créée au démarrage et seulement masquée/réaffichée : elle
    // peut donc rater l'événement du tout premier affichage. On demande aussi
    // explicitement la sélection en attente au montage.
    void invoke<CapturedSelection>('get_pending_selection').then(reset).catch(() => {})

    const unlisten = listen<CapturedSelection>('selection-captured', (event) => {
      reset(event.payload)
    })

    // Un raccourci direct n'affiche rien quand tout va bien ; en cas d'échec,
    // la popup est sa seule surface pour le dire.
    const unlistenError = listen<{ message: string; result: string | null }>(
      'transform-error',
      (event) => {
        setResult(event.payload.result ?? '')
        setError(event.payload.message)
        setPhase('error')
      },
    )

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close()
    }
    window.addEventListener('keydown', onKeyDown)

    return () => {
      window.removeEventListener('keydown', onKeyDown)
      void unlisten.then((fn) => fn())
      void unlistenError.then((fn) => fn())
      document.body.classList.remove('popup-body')
    }
  }, [])

  const run = async (action: string) => {
    setPendingAction(action)
    setPhase('working')
    setError('')
    try {
      if (selection.previewBeforeReplace) {
        const text = await invoke<string>('transform_text', {
          text: selection.text,
          action,
        })
        setResult(text)
        setPhase('preview')
        return
      }
      const outcome = await invoke<TransformOutcome>('transform_and_replace', {
        text: selection.text,
        action,
      })
      if (outcome.replaced) {
        close()
      } else {
        setResult(outcome.text)
        setError(outcome.message ?? '')
        setPhase('error')
      }
    } catch (e) {
      setError(String(e))
      setPhase('error')
    }
  }

  const confirmReplace = async () => {
    setPhase('working')
    try {
      const outcome = await invoke<TransformOutcome>('replace_selection', { text: result })
      if (outcome.replaced) {
        close()
      } else {
        setError(outcome.message ?? '')
        setPhase('error')
      }
    } catch (e) {
      setError(String(e))
      setPhase('error')
    }
  }

  const copyResult = async () => {
    await invoke('set_clipboard', { text: result })
    close()
  }

  const preview = selection.text.replace(/\s+/g, ' ').trim()

  return (
    <div className="popup">
      <header className="popup-header" data-tauri-drag-region>
        <span className="popup-title">AI Text Replacer</span>
        <button className="icon-btn" onClick={close} title="Fermer (Échap)">
          ✕
        </button>
      </header>

      {/* L'erreur passe avant tout le reste : elle peut venir d'un raccourci
          direct, auquel cas la popup n'a jamais affiché de sélection. */}
      {phase === 'error' ? (
        <div className="popup-preview">
          <div className="popup-error">{error || 'Une erreur est survenue.'}</div>
          {result && <div className="preview-text">{result}</div>}
          <div className="popup-buttons">
            {result && (
              <button className="primary-btn" onClick={() => void copyResult()}>
                Copier le résultat
              </button>
            )}
            {selection.text.trim() !== '' && (
              <button className="ghost-btn" onClick={() => setPhase('actions')}>
                Retour
              </button>
            )}
            <button className="ghost-btn" onClick={() => void invoke('open_main_window')}>
              Paramètres
            </button>
          </div>
        </div>
      ) : selection.text.trim() === '' ? (
        <div className="popup-empty">
          <p>Aucun texte sélectionné.</p>
          <p className="hint">
            Sélectionnez du texte dans Teams, Outlook ou votre navigateur, puis appuyez
            sur le raccourci.
          </p>
          <button className="ghost-btn" onClick={() => void invoke('open_main_window')}>
            Ouvrir l'application
          </button>
        </div>
      ) : (
        <>
          <div className="popup-selection" title={selection.text}>
            « {preview.length > 110 ? `${preview.slice(0, 110)}…` : preview} »
          </div>

          {phase === 'actions' && (
            <div className="popup-actions">
              {ACTIONS.map((action) => (
                <button key={action.id} className="popup-action" onClick={() => void run(action.id)}>
                  <span className="popup-action-icon">{action.icon}</span>
                  <span className="popup-action-label">
                    {action.id === 'translate'
                      ? `Traduire en ${selection.targetLanguage}`
                      : action.label}
                  </span>
                </button>
              ))}
            </div>
          )}

          {phase === 'working' && (
            <div className="popup-status">
              <span className="spinner" />
              {pendingAction ? `${actionLabel(pendingAction)}…` : 'Traitement…'}
            </div>
          )}

          {phase === 'preview' && (
            <div className="popup-preview">
              <div className="preview-text">{result}</div>
              <div className="popup-buttons">
                <button className="primary-btn" onClick={() => void confirmReplace()}>
                  Remplacer
                </button>
                <button className="ghost-btn" onClick={() => void copyResult()}>
                  Copier
                </button>
                <button className="ghost-btn" onClick={() => setPhase('actions')}>
                  Retour
                </button>
              </div>
            </div>
          )}

        </>
      )}

      <footer className="popup-footer">
        <span>{PROVIDER_LABELS[selection.defaultProvider] ?? selection.defaultProvider}</span>
        <button className="icon-btn" onClick={() => void invoke('open_main_window')} title="Paramètres">
          ⚙
        </button>
      </footer>
    </div>
  )
}
