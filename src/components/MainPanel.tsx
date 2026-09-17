import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import {
  acceleratorFor,
  ACTIONS,
  AppConfig,
  MENU_ACTION,
  PROVIDER_IDS,
  PROVIDER_LABELS,
  ProviderId,
  TransformOutcome,
} from '../types'
import '../styles/MainPanel.css'

export default function MainPanel() {
  const [input, setInput] = useState('')
  const [output, setOutput] = useState('')
  const [selectedAction, setSelectedAction] = useState('grammar')
  const [provider, setProvider] = useState<ProviderId>('claude')
  const [shortcuts, setShortcuts] = useState({
    menu: '',
    grammar: '',
    rephrase: '',
    prompt: '',
  })
  const [isLoading, setIsLoading] = useState(false)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')

  useEffect(() => {
    invoke<AppConfig>('get_config')
      .then((cfg) => {
        setProvider(cfg.defaultProvider)
        setShortcuts({
          menu: acceleratorFor(cfg, MENU_ACTION),
          grammar: acceleratorFor(cfg, 'grammar'),
          rephrase: acceleratorFor(cfg, 'rephrase'),
          prompt: acceleratorFor(cfg, 'prompt'),
        })
      })
      .catch(() => {})
  }, [])

  const captureText = async () => {
    setIsLoading(true)
    setError('')
    setMessage('')
    try {
      const captured = await invoke<string>('capture_selection')
      if (captured.trim() === '') {
        setError("Aucun texte sélectionné dans l'application précédente.")
      } else {
        setInput(captured)
      }
    } catch (e) {
      setError(String(e))
    } finally {
      setIsLoading(false)
    }
  }

  const transform = async () => {
    if (!input.trim()) return
    setIsLoading(true)
    setError('')
    setMessage('')
    try {
      const result = await invoke<string>('transform_text', {
        text: input,
        action: selectedAction,
        provider,
      })
      setOutput(result)
    } catch (e) {
      setError(String(e))
      setOutput('')
    } finally {
      setIsLoading(false)
    }
  }

  /** Recolle le résultat dans l'application où le texte a été capturé. */
  const replaceInSource = async () => {
    setIsLoading(true)
    setError('')
    setMessage('')
    try {
      const outcome = await invoke<TransformOutcome>('replace_selection', { text: output })
      setMessage(
        outcome.replaced
          ? 'Texte remplacé dans l\'application d\'origine.'
          : outcome.message ?? 'Résultat copié dans le presse-papiers.',
      )
    } catch (e) {
      setError(String(e))
    } finally {
      setIsLoading(false)
    }
  }

  const copyToClipboard = async () => {
    await invoke('set_clipboard', { text: output })
    setMessage('Résultat copié.')
  }

  return (
    <div className="main-panel">
      <div className="banner">
        <p>
          Sélectionnez du texte dans Teams, Outlook ou votre navigateur, puis utilisez un
          raccourci — inutile de passer par cette fenêtre.
        </p>
        <ul className="banner-shortcuts">
          <li>
            <kbd>{shortcuts.grammar}</kbd> corrige et remplace immédiatement
          </li>
          <li>
            <kbd>{shortcuts.rephrase}</kbd> reformule et remplace immédiatement
          </li>
          <li>
            <kbd>{shortcuts.prompt}</kbd> traite le texte comme une consigne et le
            remplace par le résultat
          </li>
          <li>
            <kbd>{shortcuts.menu}</kbd> ouvre le menu au curseur et laisse choisir
          </li>
        </ul>
      </div>

      <div className="section">
        <h2>Action</h2>
        <div className="action-grid">
          {ACTIONS.map((action) => (
            <button
              key={action.id}
              className={`action-btn ${selectedAction === action.id ? 'active' : ''}`}
              onClick={() => setSelectedAction(action.id)}
            >
              <div className="action-label">
                {action.icon} {action.label}
              </div>
              <div className="action-desc">{action.description}</div>
            </button>
          ))}
        </div>
      </div>

      <div className="section">
        <label htmlFor="provider-select">Fournisseur IA :</label>
        <select
          id="provider-select"
          value={provider}
          onChange={(e) => setProvider(e.target.value as ProviderId)}
        >
          {PROVIDER_IDS.map((id) => (
            <option key={id} value={id}>
              {PROVIDER_LABELS[id]}
            </option>
          ))}
        </select>
      </div>

      <div className="section">
        <div className="input-header">
          <label htmlFor="input-text">Texte à transformer :</label>
          <button
            className="capture-btn"
            onClick={() => void captureText()}
            disabled={isLoading}
            title="Copie la sélection de l'application précédemment active"
          >
            {isLoading ? '⏳' : '📋'} Capturer la sélection
          </button>
        </div>
        <textarea
          id="input-text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="Entrez ou capturez le texte à transformer…"
          rows={6}
        />
      </div>

      <button
        className="transform-btn"
        onClick={() => void transform()}
        disabled={!input.trim() || isLoading}
      >
        {isLoading ? 'Traitement en cours…' : 'Transformer'}
      </button>

      {error && <div className="status-error">{error}</div>}
      {message && <div className="status-ok">{message}</div>}

      {output && (
        <div className="section output-section">
          <h3>Résultat</h3>
          <div className="output-text">{output}</div>
          <div className="output-buttons">
            <button className="copy-btn" onClick={() => void copyToClipboard()}>
              Copier
            </button>
            <button className="copy-btn" onClick={() => void replaceInSource()}>
              Remplacer dans l'application d'origine
            </button>
          </div>
        </div>
      )}
    </div>
  )
}
