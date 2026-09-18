// Types partagés avec le backend Rust (src-tauri/src/config.rs, commands.rs).
// Les structures Rust sont sérialisées en camelCase.

export type ProviderId = 'openai' | 'claude' | 'groq' | 'local'

export interface ProviderConfig {
  apiKey: string
  model: string
  endpoint: string
}

/** Une combinaison de touches et l'action qu'elle déclenche. */
export interface ShortcutBinding {
  accelerator: string
  /** 'menu' ouvre la popup ; toute autre valeur est appliquée directement. */
  action: string
}

export const MENU_ACTION = 'menu'

/** Un texte figé inséré tel quel par sa combinaison de touches. */
export interface Snippet {
  id: string
  label: string
  accelerator: string
  text: string
}

/** Les trois liaisons proposées dans les Paramètres, dans cet ordre. */
export const SHORTCUT_SLOTS: { action: string; label: string; help: string }[] = [
  {
    action: MENU_ACTION,
    label: 'Menu au curseur',
    help: 'Ouvre le menu près du curseur et laisse choisir l\'action.',
  },
  {
    action: 'grammar',
    label: 'Corriger et remplacer',
    help: 'Corrige la grammaire et remplace immédiatement, sans rien afficher.',
  },
  {
    action: 'rephrase',
    label: 'Reformuler et remplacer',
    help: 'Reformule et remplace immédiatement, sans rien afficher.',
  },
  {
    action: 'prompt',
    label: 'Exécuter une consigne',
    help: "Traite le texte comme un ordre — « écris-moi un message pour demander une augmentation » — et le remplace par le résultat.",
  },
]

/** Contrôle grammatical spontané — voir src-tauri/src/proactive.rs. */
export interface ProactiveConfig {
  enabled: boolean
  /** Nom d'exécutable sous Windows, nom affiché sous macOS. */
  apps: string[]
  idleSeconds: number
  minChars: number
}

export const IDLE_SECONDS_MIN = 1
export const IDLE_SECONDS_MAX = 300

export interface AppConfig {
  shortcuts: ShortcutBinding[]
  snippets: Snippet[]
  proactive: ProactiveConfig
  defaultProvider: ProviderId
  providers: Record<string, ProviderConfig>
  previewBeforeReplace: boolean
  targetLanguage: string
  customInstructions: string
  startMinimized: boolean
  startAtLogin: boolean
  captureMode: CaptureMode
}

export type CaptureMode = 'field' | 'selection' | 'selectionThenField'

export const CAPTURE_MODES: { value: CaptureMode; label: string; help: string }[] = [
  {
    value: 'field',
    // Le compromis de chaque mode est dans le libellé, pas seulement dans le
    // texte d'aide : c'est au moment de choisir qu'il faut le voir.
    label: 'Tout le champ de saisie — silencieux',
    help: "Envoie Ctrl+A puis Ctrl+C. Placez le curseur dans votre message, rien à sélectionner. C'est le seul mode qui ne fait jamais biper Windows, car il n'envoie jamais de copie à vide. Une sélection manuelle est ignorée : c'est tout le champ qui est traité.",
  },
  {
    value: 'selection',
    label: 'Ma sélection uniquement — bipe si vous oubliez de sélectionner',
    help: "N'envoie que Ctrl+C. Vous devez sélectionner votre texte. Si vous oubliez, rien n'est capturé et Windows peut biper.",
  },
  {
    value: 'selectionThenField',
    label: 'Ma sélection, sinon tout le champ — bipe sans sélection',
    help: "Le plus souple, mais il commence par une copie à l'aveugle : quand rien n'est sélectionné, Windows bipe avant que le repli ne prenne le relais.",
  },
]

export interface HistoryEntry {
  id: string
  original: string
  transformed: string
  action: string
  provider: string
  timestamp: number
}

export interface TransformOutcome {
  text: string
  replaced: boolean
  message: string | null
}

/** Payload de l'événement `selection-captured` émis vers la popup. */
export interface CapturedSelection {
  text: string
  defaultProvider: ProviderId
  previewBeforeReplace: boolean
  targetLanguage: string
}

export interface ActionDefinition {
  id: string
  label: string
  icon: string
  description: string
}

export const ACTIONS: ActionDefinition[] = [
  { id: 'grammar', label: 'Corriger la grammaire', icon: '✍️', description: 'Orthographe, grammaire, ponctuation' },
  { id: 'rephrase', label: 'Reformuler', icon: '🔄', description: 'Plus clair et plus fluide' },
  { id: 'professional', label: 'Ton professionnel', icon: '👔', description: 'Registre de communication de travail' },
  { id: 'concise', label: 'Raccourcir', icon: '✂️', description: 'Plus court, sans perdre le sens' },
  { id: 'translate', label: 'Traduire', icon: '🌍', description: 'Vers la langue configurée' },
  { id: 'summarize', label: 'Résumer', icon: '📝', description: 'Quelques phrases' },
  {
    id: 'prompt',
    label: 'Exécuter la consigne',
    icon: '✨',
    description: 'Le texte est un ordre, pas un contenu à réécrire',
  },
]

export const PROVIDER_LABELS: Record<ProviderId, string> = {
  openai: 'OpenAI',
  claude: 'Claude (Anthropic)',
  groq: 'Groq',
  local: 'Modèle local',
}

export const PROVIDER_IDS: ProviderId[] = ['openai', 'claude', 'groq', 'local']

export function actionLabel(id: string): string {
  return ACTIONS.find((a) => a.id === id)?.label ?? id
}

export function acceleratorFor(config: AppConfig | null, action: string): string {
  return config?.shortcuts.find((b) => b.action === action)?.accelerator ?? ''
}
