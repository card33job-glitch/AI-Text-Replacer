import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/api/shell'
import {
  ACTIONS,
  actionLabel,
  CapturedSelection,
  PROVIDER_LABELS,
  ReplySuggestion,
  TransformOutcome,
} from '../types'
import '../styles/Popup.css'

type Phase = 'actions' | 'working' | 'preview' | 'reply' | 'error'

const REPLY = 'reply'

const EMPTY: CapturedSelection = {
  text: '',
  intent: '',
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
  const [suggestion, setSuggestion] = useState<ReplySuggestion | null>(null)
  const [hint, setHint] = useState('')
  // Numéro de la dernière demande de réponse : une réponse arrivée après une
  // nouvelle capture ne doit pas s'afficher sur le mauvais message.
  const replyRequest = useRef(0)
  // Au tout premier affichage, l'événement et la relecture au montage peuvent
  // livrer la même capture : un seul appel au modèle doit partir.
  const lastReplyStart = useRef({ text: '', at: 0 })

  const close = () => {
    void invoke('hide_popup')
  }

  const startReply = async (text: string, extraHint: string) => {
    const request = ++replyRequest.current
    setPendingAction(REPLY)
    setPhase('working')
    setError('')
    try {
      const next = await invoke<ReplySuggestion>('suggest_reply', {
        text,
        hint: extraHint || null,
      })
      if (request !== replyRequest.current) return
      setSuggestion(next)
      setResult(next.text)
      setPhase('reply')
    } catch (e) {
      if (request !== replyRequest.current) return
      setError(String(e))
      setPhase('error')
    }
  }

  const reset = (next: CapturedSelection) => {
    replyRequest.current++
    setSelection(next)
    setPhase('actions')
    setPendingAction('')
    setResult('')
    setError('')
    setSuggestion(null)
    setHint('')

    if (next.intent === REPLY && next.text.trim() !== '') {
      const now = Date.now()
      const last = lastReplyStart.current
      if (last.text === next.text && now - last.at < 2000) return
      lastReplyStart.current = { text: next.text, at: now }
      void startReply(next.text, '')
    }
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
    if (action === REPLY) {
      await invoke('expand_popup')
      await startReply(selection.text, '')
      return
    }
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

  // Ce que l'utilisateur garde, retouches comprises, devient un exemple pour
  // les prochaines propositions.
  const acceptReply = () =>
    invoke('accept_reply', { message: selection.text, reply: result }).catch(() => {})

  const copyReply = async () => {
    await acceptReply()
    await copyResult()
  }

  const pasteReply = async () => {
    await acceptReply()
    await confirmReplace()
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
          {selection.intent === REPLY ? (
            <>
              <p>Aucun message trouvé.</p>
              <p className="hint">
                Ouvrez le courriel dans Outlook ou la conversation dans Teams, ou
                sélectionnez le message auquel répondre, puis appuyez sur le raccourci.
              </p>
            </>
          ) : (
            <>
              <p>Aucun texte sélectionné.</p>
              <p className="hint">
                Sélectionnez du texte dans Teams, Outlook ou votre navigateur, puis appuyez
                sur le raccourci.
              </p>
            </>
          )}
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
              {pendingAction === REPLY
                ? 'Recherche dans Confluence et rédaction…'
                : pendingAction
                  ? `${actionLabel(pendingAction)}…`
                  : 'Traitement…'}
            </div>
          )}

          {phase === 'reply' && (
            <div className="popup-preview">
              <textarea
                className="preview-text reply-editor"
                value={result}
                onChange={(e) => setResult(e.target.value)}
                aria-label="Réponse proposée, modifiable"
              />
              <div className="reply-sources">
                {suggestion && suggestion.sources.length > 0 ? (
                  <>
                    <span>Confluence :</span>
                    {suggestion.sources.map((s) =>
                      s.url ? (
                        <button
                          key={`${s.url}-${s.title}`}
                          className="source-link"
                          title={s.url}
                          onClick={() => void open(s.url)}
                        >
                          {s.title}
                        </button>
                      ) : (
                        <span key={s.title}>{s.title}</span>
                      ),
                    )}
                  </>
                ) : (
                  <span>Aucune page Confluence pertinente trouvée.</span>
                )}
                {suggestion && suggestion.pastReplies > 0 && (
                  <span>
                    · {suggestion.pastReplies} réponse{suggestion.pastReplies > 1 ? 's' : ''}{' '}
                    passée{suggestion.pastReplies > 1 ? 's' : ''} en exemple
                  </span>
                )}
              </div>
              <form
                className="reply-hint"
                onSubmit={(e) => {
                  e.preventDefault()
                  void startReply(selection.text, hint)
                }}
              >
                <input
                  type="text"
                  value={hint}
                  onChange={(e) => setHint(e.target.value)}
                  placeholder="Ajuster : plus court, tutoie-le, propose un appel…"
                />
                <button type="submit" className="ghost-btn">
                  Régénérer
                </button>
              </form>
              <div className="popup-buttons">
                <button className="primary-btn" onClick={() => void copyReply()}>
                  Copier
                </button>
                <button
                  className="ghost-btn"
                  onClick={() => void pasteReply()}
                  title="Colle la réponse dans l'application d'origine, à la place de ce qui y est sélectionné"
                >
                  Coller à la place de la sélection
                </button>
              </div>
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
