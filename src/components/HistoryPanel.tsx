import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { actionLabel, HistoryEntry, PROVIDER_LABELS, ProviderId } from '../types'
import '../styles/HistoryPanel.css'

export default function HistoryPanel() {
  const [history, setHistory] = useState<HistoryEntry[]>([])
  const [isLoading, setIsLoading] = useState(true)

  const load = async () => {
    setIsLoading(true)
    try {
      setHistory(await invoke<HistoryEntry[]>('get_history'))
    } catch (error) {
      console.error('Chargement de l\'historique:', error)
    } finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [])

  const clear = async () => {
    await invoke('clear_history')
    setHistory([])
  }

  const truncate = (text: string) =>
    text.length > 160 ? `${text.slice(0, 160)}…` : text

  return (
    <div className="history-panel">
      <div className="history-toolbar">
        <h2>Historique des transformations</h2>
        {history.length > 0 && (
          <button className="copy-btn" onClick={() => void clear()}>
            Vider l'historique
          </button>
        )}
      </div>

      {isLoading ? (
        <div className="loading">Chargement…</div>
      ) : history.length === 0 ? (
        <div className="empty-state">Aucune transformation enregistrée</div>
      ) : (
        <div className="history-list">
          {history.map((entry) => (
            <div key={entry.id} className="history-item">
              <div className="history-header">
                <span className="action-tag">{actionLabel(entry.action)}</span>
                <span className="timestamp">
                  {PROVIDER_LABELS[entry.provider as ProviderId] ?? entry.provider} ·{' '}
                  {new Date(entry.timestamp).toLocaleString('fr-FR')}
                </span>
              </div>
              <div className="history-content">
                <div className="original">
                  <strong>Original :</strong>
                  <p>{truncate(entry.original)}</p>
                </div>
                <div className="transformed">
                  <strong>Transformé :</strong>
                  <p>{truncate(entry.transformed)}</p>
                </div>
              </div>
              <button
                className="copy-btn"
                onClick={() => void invoke('set_clipboard', { text: entry.transformed })}
              >
                Copier résultat
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
