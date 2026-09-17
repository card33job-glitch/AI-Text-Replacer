import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { open } from '@tauri-apps/api/shell'
import '../styles/AccessibilityNotice.css'

interface AccessibilityStatus {
  granted: boolean
  required: boolean
  settingsUrl: string | null
}

/**
 * Bandeau affiché uniquement sur macOS, tant que l'autorisation
 * « Accessibilité » n'est pas accordée.
 *
 * Sans elle, les raccourcis s'enregistrent et se déclenchent, mais les frappes
 * simulées n'atteignent jamais l'application cible : l'outil paraît cassé sans
 * qu'aucune erreur ne soit levée. Le bandeau est la seule façon de le dire.
 */
export default function AccessibilityNotice() {
  const [status, setStatus] = useState<AccessibilityStatus | null>(null)

  useEffect(() => {
    const refresh = () => {
      invoke<AccessibilityStatus>('accessibility_status').then(setStatus).catch(() => {})
    }
    refresh()
    // L'autorisation s'accorde dans les Réglages Système, hors de
    // l'application : on revérifie au retour du focus.
    window.addEventListener('focus', refresh)
    return () => window.removeEventListener('focus', refresh)
  }, [])

  if (!status || !status.required || status.granted) return null

  return (
    <div className="permission-banner" role="alert">
      <strong>Autorisation « Accessibilité » requise</strong>
      <p>
        macOS empêche AI Text Replacer de lire et de remplacer du texte dans les autres
        applications. Tant qu'elle n'est pas accordée, les raccourcis se déclenchent sans
        rien produire.
      </p>
      <p className="steps">
        Réglages Système → Confidentialité et sécurité → Accessibilité → activer{' '}
        <strong>AI Text Replacer</strong>, puis relancer l'application.
      </p>
      {status.settingsUrl && (
        <button onClick={() => void open(status.settingsUrl!)}>
          Ouvrir les Réglages Système
        </button>
      )}
    </div>
  )
}
