import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import {
  acceleratorFor,
  AppConfig,
  CAPTURE_MODES,
  CaptureMode,
  PROVIDER_IDS,
  PROVIDER_LABELS,
  ProviderConfig,
  ProviderId,
  SHORTCUT_SLOTS,
} from '../types'
import '../styles/SettingsPanel.css'

const PROVIDER_HELP: Record<ProviderId, string> = {
  openai: 'Clé sur platform.openai.com/api-keys',
  claude: 'Clé sur console.anthropic.com',
  groq: 'Clé gratuite sur console.groq.com',
  local: 'Ollama ou LM Studio — aucune clé requise, laissez le champ vide',
}

const EMPTY_PROVIDER: ProviderConfig = { apiKey: '', model: '', endpoint: '' }

// La valeur est reprise telle quelle dans le prompt ("Tu traduis le texte en …"),
// d'où des libellés en minuscules côté valeur.
const TARGET_LANGUAGES = [
  { value: 'anglais', label: 'Anglais' },
  { value: 'français', label: 'Français' },
]

const IS_MAC = navigator.userAgent.includes('Mac')

/** Traduit un événement clavier en accélérateur Tauri ("Ctrl+Shift+T"). */
function toAccelerator(e: React.KeyboardEvent<HTMLInputElement>): string | null {
  const parts: string[] = []
  // Sur macOS, la touche Command se nomme « Cmd » côté Tauri ; « Super » y est
  // accepté mais s'afficherait de façon déroutante dans les Paramètres.
  if (e.metaKey && IS_MAC) parts.push('Cmd')
  if (e.ctrlKey) parts.push('Ctrl')
  if (e.altKey) parts.push(IS_MAC ? 'Option' : 'Alt')
  if (e.shiftKey) parts.push('Shift')
  if (e.metaKey && !IS_MAC) parts.push('Super')

  const code = e.code
  let key = ''
  if (code.startsWith('Key')) key = code.slice(3)
  else if (code.startsWith('Digit')) key = code.slice(5)
  else if (/^F\d{1,2}$/.test(code)) key = code
  else if (code === 'Space') key = 'Space'
  else if (code === 'Enter') key = 'Enter'
  else if (code === 'Period') key = '.'
  else if (code === 'Comma') key = ','

  // Une combinaison sans touche "réelle", ou sans modificateur, ne ferait pas
  // un raccourci global utilisable.
  if (!key || parts.length === 0) return null
  parts.push(key)
  return parts.join('+')
}

export default function SettingsPanel() {
  const [config, setConfig] = useState<AppConfig | null>(null)
  const [status, setStatus] = useState('')
  const [error, setError] = useState('')
  // Action dont le champ attend actuellement une combinaison ('' = aucun).
  const [capturing, setCapturing] = useState('')
  const [isSaving, setIsSaving] = useState(false)

  useEffect(() => {
    invoke<AppConfig>('get_config')
      .then(setConfig)
      .catch((e) => setError(String(e)))
  }, [])

  if (!config) {
    return <div className="settings-panel">{error || 'Chargement des paramètres…'}</div>
  }

  const patch = (changes: Partial<AppConfig>) => {
    setConfig({ ...config, ...changes })
    setStatus('')
    setError('')
  }

  const providerConfig = (id: ProviderId): ProviderConfig => config.providers[id] ?? EMPTY_PROVIDER

  const patchProvider = (id: ProviderId, changes: Partial<ProviderConfig>) => {
    patch({
      providers: {
        ...config.providers,
        [id]: { ...providerConfig(id), ...changes },
      },
    })
  }

  const onShortcutKeyDown = (e: React.KeyboardEvent<HTMLInputElement>, action: string) => {
    e.preventDefault()
    if (e.key === 'Escape') {
      setCapturing('')
      return
    }
    const accelerator = toAccelerator(e)
    if (!accelerator) return

    // Remplace la liaison existante, ou l'ajoute si la config vient d'une
    // version qui ne connaissait pas cette action.
    const existing = config.shortcuts.some((b) => b.action === action)
    patch({
      shortcuts: existing
        ? config.shortcuts.map((b) => (b.action === action ? { ...b, accelerator } : b))
        : [...config.shortcuts, { accelerator, action }],
    })
    setCapturing('')
  }

  const save = async () => {
    setIsSaving(true)
    setStatus('')
    setError('')
    try {
      await invoke('save_config', { config })
      setStatus('Paramètres enregistrés. Les raccourcis sont actifs immédiatement.')
    } catch (e) {
      setError(String(e))
      // Le backend a pu revenir aux anciens raccourcis : on resynchronise.
      invoke<AppConfig>('get_config').then(setConfig).catch(() => {})
    } finally {
      setIsSaving(false)
    }
  }

  return (
    <div className="settings-panel">
      <div className="section">
        <h2>Raccourcis clavier globaux</h2>
        <p className="help-text">
          Cliquez dans un champ puis appuyez sur la combinaison voulue (au moins un
          modificateur + une touche). Chaque raccourci capture le texte sélectionné dans
          l'application active.
        </p>

        {SHORTCUT_SLOTS.map((slot) => (
          <div key={slot.action} className="shortcut-row">
            <label htmlFor={`shortcut-${slot.action}`}>{slot.label}</label>
            <input
              id={`shortcut-${slot.action}`}
              className="shortcut-input"
              type="text"
              readOnly
              value={
                capturing === slot.action
                  ? 'Appuyez sur la combinaison…'
                  : acceleratorFor(config, slot.action)
              }
              onFocus={() => setCapturing(slot.action)}
              onBlur={() => setCapturing('')}
              onKeyDown={(e) => onShortcutKeyDown(e, slot.action)}
            />
            <p className="help-text">{slot.help}</p>
          </div>
        ))}

        <p className="help-text warning-text">
          ⚠️ Un raccourci global est capté dans <em>toutes</em> les applications. Avec la
          valeur par défaut, <kbd>Ctrl+Shift+R</kbd> ne rechargera plus de force les pages
          de votre navigateur — changez-la si vous y tenez.
        </p>
      </div>

      <div className="section">
        <h2>Fournisseur par défaut</h2>
        <select
          value={config.defaultProvider}
          onChange={(e) => patch({ defaultProvider: e.target.value as ProviderId })}
        >
          {PROVIDER_IDS.map((id) => (
            <option key={id} value={id}>
              {PROVIDER_LABELS[id]}
            </option>
          ))}
        </select>
        <p className="help-text">
          C'est ce fournisseur qu'utilise le menu contextuel. Les autres restent
          disponibles depuis l'onglet Transformer.
        </p>
      </div>

      <div className="section">
        <h2>Comportement</h2>
        <div className="checkbox-group">
          <label>
            <input
              type="checkbox"
              checked={config.previewBeforeReplace}
              onChange={(e) => patch({ previewBeforeReplace: e.target.checked })}
            />
            <span>Afficher un aperçu avant de remplacer</span>
          </label>
          <p className="help-text">
            Ne concerne que le <strong>menu au curseur</strong>. Décoché, le texte est
            remplacé dès que vous cliquez sur une action. Les deux raccourcis directs
            remplacent toujours sans aperçu — c'est leur raison d'être.
          </p>
        </div>

        <label htmlFor="capture-mode">Que capturer au raccourci :</label>
        <select
          id="capture-mode"
          value={config.captureMode}
          onChange={(e) => patch({ captureMode: e.target.value as CaptureMode })}
        >
          {CAPTURE_MODES.map((mode) => (
            <option key={mode.value} value={mode.value}>
              {mode.label}
            </option>
          ))}
        </select>
        <p className="help-text">
          {CAPTURE_MODES.find((m) => m.value === config.captureMode)?.help}
        </p>
        <p className="help-text warning-text">
          ⚠️ « Tout le champ » envoie <kbd>Ctrl+A</kbd>. Dans un traitement de texte, cela
          prend le document entier — choisissez « Ma sélection » si vous utilisez l'outil
          ailleurs que dans des champs de saisie courts.
        </p>

        <div className="checkbox-group">
          <label>
            <input
              type="checkbox"
              checked={config.startMinimized}
              onChange={(e) => patch({ startMinimized: e.target.checked })}
            />
            <span>Démarrer réduit dans la zone de notification</span>
          </label>
        </div>

        <label htmlFor="target-language">Langue de traduction :</label>
        <select
          id="target-language"
          value={config.targetLanguage}
          onChange={(e) => patch({ targetLanguage: e.target.value })}
        >
          {TARGET_LANGUAGES.map((lang) => (
            <option key={lang.value} value={lang.value}>
              {lang.label}
            </option>
          ))}
          {/* Une config plus ancienne peut contenir une autre langue :
              on la garde comme option plutôt que d'afficher un champ vide. */}
          {!TARGET_LANGUAGES.some((l) => l.value === config.targetLanguage) && (
            <option value={config.targetLanguage}>{config.targetLanguage}</option>
          )}
        </select>
        <p className="help-text">
          Utilisée uniquement par l'action « Traduire ». Les autres actions conservent
          la langue d'origine du texte.
        </p>

        <label htmlFor="custom-instructions">Consignes permanentes :</label>
        <textarea
          id="custom-instructions"
          rows={3}
          value={config.customInstructions}
          onChange={(e) => patch({ customInstructions: e.target.value })}
          placeholder="Ex: vouvoie toujours, évite les anglicismes, garde un ton direct."
        />
        <p className="help-text">Ces consignes sont ajoutées à chaque demande.</p>
      </div>

      <div className="section">
        <h2>Fournisseurs IA</h2>
        {PROVIDER_IDS.map((id) => {
          const pc = providerConfig(id)
          return (
            <div key={id} className="api-config">
              <div className="api-header">
                <strong>{PROVIDER_LABELS[id]}</strong>
                {config.defaultProvider === id && <span className="badge">par défaut</span>}
              </div>
              <p className="help-text">{PROVIDER_HELP[id]}</p>
              {id !== 'local' && (
                <input
                  type="password"
                  placeholder={`Clé API ${PROVIDER_LABELS[id]}`}
                  value={pc.apiKey}
                  onChange={(e) => patchProvider(id, { apiKey: e.target.value })}
                />
              )}
              <input
                type="text"
                placeholder="Modèle"
                value={pc.model}
                onChange={(e) => patchProvider(id, { model: e.target.value })}
              />
              <input
                type="text"
                placeholder="Endpoint"
                value={pc.endpoint}
                onChange={(e) => patchProvider(id, { endpoint: e.target.value })}
              />
            </div>
          )
        })}
      </div>

      {status && <div className="status-ok">{status}</div>}
      {error && <div className="status-error">{error}</div>}

      <button className="save-btn" onClick={() => void save()} disabled={isSaving}>
        {isSaving ? 'Sauvegarde…' : 'Sauvegarder les paramètres'}
      </button>
    </div>
  )
}
