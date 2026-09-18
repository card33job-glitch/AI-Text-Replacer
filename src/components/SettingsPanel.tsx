import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import {
  acceleratorFor,
  AppConfig,
  CAPTURE_MODES,
  CaptureMode,
  PROVIDER_IDS,
  PROVIDER_LABELS,
  IDLE_SECONDS_MAX,
  IDLE_SECONDS_MIN,
  ProactiveConfig,
  ProviderConfig,
  ProviderId,
  SHORTCUT_SLOTS,
  Snippet,
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
  // Dernière application active hors la nôtre, proposée à la surveillance.
  const [lastApp, setLastApp] = useState<string | null>(null)

  useEffect(() => {
    invoke<AppConfig>('get_config')
      .then(setConfig)
      .catch((e) => setError(String(e)))
  }, [])

  // Le backend ne retient que les applications tierces : dès que ces
  // Paramètres sont au premier plan, la valeur cesse de bouger et reste donc
  // celle de l'application quittée pour venir ici.
  useEffect(() => {
    const read = () => {
      invoke<string | null>('last_foreground_app')
        .then(setLastApp)
        .catch(() => {})
    }
    read()
    const timer = window.setInterval(read, 1500)
    return () => window.clearInterval(timer)
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

  const patchSnippet = (id: string, changes: Partial<Snippet>) => {
    patch({
      snippets: config.snippets.map((s) => (s.id === id ? { ...s, ...changes } : s)),
    })
  }

  const addSnippet = () => {
    // `crypto.randomUUID` n'existe pas dans tous les contextes de webview :
    // horodatage plus aléa suffit, l'id ne sert qu'à suivre la ligne à l'écran.
    const id = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
    patch({ snippets: [...config.snippets, { id, label: '', accelerator: '', text: '' }] })
  }

  const removeSnippet = (id: string) => {
    patch({ snippets: config.snippets.filter((s) => s.id !== id) })
  }

  const patchProactive = (changes: Partial<ProactiveConfig>) => {
    patch({ proactive: { ...config.proactive, ...changes } })
  }

  const addWatchedApp = (name: string) => {
    const trimmed = name.trim()
    if (!trimmed) return
    const already = config.proactive.apps.some(
      (a) => a.trim().toLowerCase() === trimmed.toLowerCase(),
    )
    if (already) return
    patchProactive({ apps: [...config.proactive.apps, trimmed] })
  }

  const onSnippetKeyDown = (e: React.KeyboardEvent<HTMLInputElement>, id: string) => {
    e.preventDefault()
    if (e.key === 'Escape') {
      setCapturing('')
      return
    }
    // Retour arrière sur un champ déjà rempli : libère la combinaison, sans
    // quoi il n'y aurait aucun moyen de retirer un raccourci sans supprimer
    // le texte figé lui-même.
    if (e.key === 'Backspace' || e.key === 'Delete') {
      patchSnippet(id, { accelerator: '' })
      setCapturing('')
      return
    }
    const accelerator = toAccelerator(e)
    if (!accelerator) return
    patchSnippet(id, { accelerator })
    setCapturing('')
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
    // Les lignes vides du champ « applications surveillées » sont tolérées
    // pendant la frappe, pas stockées : elles reviendraient à chaque
    // rechargement des Paramètres.
    const cleaned: AppConfig = {
      ...config,
      proactive: {
        ...config.proactive,
        apps: config.proactive.apps.map((a) => a.trim()).filter(Boolean),
      },
    }
    try {
      await invoke('save_config', { config: cleaned })
      setConfig(cleaned)
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
        <h2>Textes figés</h2>
        <p className="help-text">
          Une combinaison, un texte inséré tel quel au point d'insertion — signature,
          adresse, formule qui revient sans cesse. Aucun appel au modèle : c'est de la
          frappe automatique, donc c'est instantané et gratuit.
        </p>

        {config.snippets.length === 0 && (
          <p className="help-text">Aucun texte figé pour l'instant.</p>
        )}

        {config.snippets.map((snippet) => (
          <div key={snippet.id} className="api-config">
            <div className="api-header snippet-header">
              <input
                type="text"
                placeholder="Nom (ex : Signature)"
                value={snippet.label}
                onChange={(e) => patchSnippet(snippet.id, { label: e.target.value })}
              />
              <button
                type="button"
                className="remove-btn"
                onClick={() => removeSnippet(snippet.id)}
                aria-label={`Supprimer ${snippet.label || 'ce texte figé'}`}
              >
                Supprimer
              </button>
            </div>
            <input
              className="shortcut-input"
              type="text"
              readOnly
              placeholder="Cliquez puis tapez la combinaison"
              value={
                capturing === `snippet:${snippet.id}`
                  ? 'Appuyez sur la combinaison…'
                  : snippet.accelerator
              }
              onFocus={() => setCapturing(`snippet:${snippet.id}`)}
              onBlur={() => setCapturing('')}
              onKeyDown={(e) => onSnippetKeyDown(e, snippet.id)}
            />
            <textarea
              rows={3}
              placeholder="Texte inséré, tel quel, retours à la ligne compris."
              value={snippet.text}
              onChange={(e) => patchSnippet(snippet.id, { text: e.target.value })}
            />
          </div>
        ))}

        <button type="button" className="add-btn" onClick={addSnippet}>
          Ajouter un texte figé
        </button>
        <p className="help-text">
          Le texte remplace ce qui est sélectionné, comme un collage ordinaire — sinon
          il s'insère au curseur. Votre presse-papiers est restauré juste après.
          <kbd>Retour arrière</kbd> dans le champ de combinaison la libère.
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
              checked={config.startAtLogin}
              onChange={(e) => patch({ startAtLogin: e.target.checked })}
            />
            <span>Lancer au démarrage de l'ordinateur</span>
          </label>
          <p className="help-text">
            L'application démarre à l'ouverture de votre session, directement dans la
            zone de notification : les raccourcis sont actifs sans rien avoir à ouvrir.
            {IS_MAC
              ? " L'entrée est posée dans ~/Library/LaunchAgents, pour votre compte uniquement."
              : ' L\'entrée apparaît dans le Gestionnaire des tâches, onglet « Démarrage », pour votre compte uniquement.'}
          </p>
        </div>

        <div className="checkbox-group">
          <label>
            <input
              type="checkbox"
              checked={config.startMinimized}
              onChange={(e) => patch({ startMinimized: e.target.checked })}
            />
            <span>Démarrer réduit dans la zone de notification</span>
          </label>
          <p className="help-text">
            S'applique aussi quand vous lancez l'application vous-même. Un démarrage
            automatique est de toute façon toujours réduit.
          </p>
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
        <h2>Correction automatique</h2>
        <div className="checkbox-group">
          <label>
            <input
              type="checkbox"
              checked={config.proactive.enabled}
              onChange={(e) => patchProactive({ enabled: e.target.checked })}
            />
            <span>Corriger la grammaire sans rien demander</span>
          </label>
          <p className="help-text">
            Dans les applications listées ci-dessous, une pause dans votre frappe
            déclenche une relecture. Si le modèle trouve des fautes, votre texte est
            remplacé immédiatement.
          </p>
        </div>

        <p className="help-text warning-text">
          ⚠️ Pour relire votre texte, l'application doit envoyer <kbd>Ctrl+A</kbd> puis
          <kbd>Ctrl+C</kbd> — aucun programme ne peut lire le champ d'un autre. Votre
          champ est donc <strong>entièrement sélectionné pendant un instant</strong>.
          C'est à quoi sert le délai : on ne relit que quelqu'un qui s'est arrêté
          d'écrire. Si vous reprenez la frappe, ou changez d'application, pendant l'appel
          au modèle, le remplacement est abandonné.
        </p>

        <label htmlFor="proactive-apps">Applications surveillées :</label>
        <textarea
          id="proactive-apps"
          rows={4}
          value={config.proactive.apps.join('\n')}
          onChange={(e) =>
            patchProactive({
              // Une application par ligne ; les lignes vides disparaissent à la
              // sauvegarde, mais pas pendant la frappe, sans quoi il serait
              // impossible d'appuyer sur Entrée pour aller à la ligne.
              apps: e.target.value.split('\n'),
            })
          }
          placeholder={IS_MAC ? 'Microsoft Teams\nMail' : 'ms-teams.exe\noutlook.exe'}
        />
        <p className="help-text">
          Une par ligne.{' '}
          {IS_MAC
            ? "Le nom affiché dans le Dock — « Microsoft Teams », « Mail »."
            : "Le nom de l'exécutable — « ms-teams.exe », « outlook.exe ». Le « .exe » est facultatif."}{' '}
          La liste vide n'active rien : ce mode ne fonctionne jamais partout.
        </p>

        {lastApp && (
          <p className="help-text">
            Dernière application active : <strong>{lastApp}</strong>{' '}
            <button type="button" className="add-btn inline" onClick={() => addWatchedApp(lastApp)}>
              L'ajouter
            </button>
          </p>
        )}

        <label htmlFor="proactive-idle">
          Relire après {config.proactive.idleSeconds} seconde
          {config.proactive.idleSeconds > 1 ? 's' : ''} sans frappe :
        </label>
        <input
          id="proactive-idle"
          type="range"
          min={IDLE_SECONDS_MIN}
          max={60}
          value={config.proactive.idleSeconds}
          onChange={(e) => patchProactive({ idleSeconds: Number(e.target.value) })}
        />
        <p className="help-text">
          Court, la relecture part pendant que vous réfléchissez au milieu d'une phrase.
          Long, elle risque de partir après l'envoi de votre message. Entre{' '}
          {IDLE_SECONDS_MIN} et {IDLE_SECONDS_MAX} secondes.
        </p>

        <label htmlFor="proactive-min">Longueur minimale : {config.proactive.minChars} caractères</label>
        <input
          id="proactive-min"
          type="range"
          min={0}
          max={200}
          step={5}
          value={config.proactive.minChars}
          onChange={(e) => patchProactive({ minChars: Number(e.target.value) })}
        />
        <p className="help-text">
          En dessous, rien n'est envoyé au modèle. Un « ok » ou une adresse n'ont pas de
          grammaire à corriger, et <strong>chaque relecture est un appel facturé</strong> à
          votre fournisseur — c'est le principal garde-fou contre une note de fin de mois
          inattendue.
        </p>
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
