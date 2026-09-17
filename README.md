# 🤖 AI Text Replacer

Application de bureau (Windows, macOS partiellement) qui corrige, reformule ou traduit
le texte **directement dans l'application où vous écrivez** — Teams, Outlook, un
navigateur, n'importe quelle zone de saisie.

## Comment ça marche

Trois raccourcis globaux, tous reconfigurables dans les Paramètres :

| Raccourci | Effet |
|---|---|
| `Ctrl+Shift+G` | Corrige la grammaire et **remplace immédiatement**, sans rien afficher |
| `Ctrl+Shift+R` | Reformule et **remplace immédiatement**, sans rien afficher |
| `Ctrl+Shift+T` | Ouvre un menu près du curseur : **Corriger / Reformuler / Ton professionnel / Raccourcir / Traduire / Résumer** |

> ⚠️ Un raccourci global est capté dans *toutes* les applications. `Ctrl+Shift+R`
> prive donc le navigateur de son rechargement forcé — changez-le si besoin.

Les raccourcis directs n'ouvrent aucune fenêtre. Un bandeau de 168 × 34 px s'affiche à
côté du point d'insertion : « Analyse… » pendant l'appel au modèle, puis « Corrigé ✓ »
une fois le texte remplacé, et il s'efface au bout d'une seconde et demie. Ce bandeau ne
prend jamais le focus (`WS_EX_NOACTIVATE`) — vous pouvez continuer à taper pendant qu'il
est affiché. La popup ne s'ouvre qu'en cas d'erreur, ou si rien n'a pu être capturé.

Une pression pendant qu'une transformation tourne est ignorée, pour éviter de
retransformer un texte déjà remplacé.

### Ce qui est capturé

Une application ne peut pas lire la sélection d'une autre : il faut lui envoyer `Ctrl+C`.
Mais envoyer `Ctrl+C` alors que rien n'est sélectionné est une devinette, et beaucoup
d'applications — dont Teams — y répondent par le **bip système** de Windows. Le mode de
capture se choisit donc explicitement dans les Paramètres :

| Mode | Frappes envoyées | Bip |
|---|---|---|
| **Tout le champ de saisie** (défaut) | `Ctrl+A` puis `Ctrl+C` | jamais |
| **Ma sélection uniquement** | `Ctrl+C` | si vous oubliez de sélectionner |
| **Ma sélection, sinon tout le champ** | `Ctrl+C`, puis `Ctrl+A` + `Ctrl+C` en repli | à chaque fois que rien n'est sélectionné |

Le mode par défaut correspond à l'usage courant : on tape son message dans Teams, on
laisse le curseur dedans, on appuie sur le raccourci. Il ignore en revanche une sélection
manuelle, et dans un traitement de texte son `Ctrl+A` prend le document entier.

### Pourquoi un raccourci et pas un vrai clic droit

Windows ne permet pas d'ajouter des entrées au menu contextuel d'une application
tierce : Teams, Slack et les navigateurs dessinent leur propre menu en HTML, hors de
portée du système. Le menu clic droit de l'Explorateur (celui qu'on configure dans le
registre) ne s'applique qu'aux fichiers, jamais à du texte sélectionné.

La popup au curseur déclenchée par un raccourci est l'équivalent le plus proche, et
c'est l'approche retenue par tous les outils du même genre (Grammarly, Wordtune,
Raycast).

### La mécanique du remplacement

Une application ne peut pas lire la sélection d'une autre application. Le trajet réel
est donc :

```
raccourci → mémorisation de la fenêtre active (HWND)
         → Ctrl+C simulé → lecture du presse-papiers
         → appel au modèle IA
         → retour du focus sur la fenêtre mémorisée
         → Ctrl+V simulé → restauration du presse-papiers d'origine
```

Le presse-papiers sert de véhicule, puis son contenu initial est rendu à
l'utilisateur. Le retour du focus utilise `AttachThreadInput`, sans quoi Windows
refuse qu'un processus en arrière-plan reprenne le premier plan.

Avant **chaque** frappe simulée, `wait_for_clean_modifiers` attend que plus aucun
modificateur ne soit physiquement enfoncé. Les modificateurs se répètent automatiquement
sous Windows : un Shift relâché de force se ré-enfonce tout seul tant que l'utilisateur
tient son raccourci, et le `Ctrl+C` envoyé ensuite devient `Ctrl+Shift+C` — inconnu de la
plupart des applications, qui répondent par le bip système. Attendre est la seule
approche correcte ; le relâchement forcé n'intervient qu'après 700 ms, et uniquement sur
les touches réellement enfoncées (un `WM_KEYUP` isolé sur Alt active la barre de menus,
ce qui fait biper à son tour).

## Fournisseurs IA

| Fournisseur | Clé requise | Gratuit |
|---|---|---|
| **Groq** | console.groq.com | Oui, avec des limites de débit généreuses pour cet usage |
| **Claude (Anthropic)** | console.anthropic.com | Non (à l'usage) |
| **OpenAI** | platform.openai.com | Non (à l'usage) |
| **Modèle local** | aucune | Oui — Ollama ou LM Studio, rien ne sort de la machine |

Le fournisseur par défaut — celui qu'utilise la popup — se choisit dans
**Paramètres → Fournisseur par défaut**. Le modèle et l'endpoint de chacun sont
modifiables ; le champ « Modèle local » vise `http://localhost:11434/v1/chat/completions`
(Ollama) par défaut.

## Paramètres

- **Raccourcis globaux** : cliquez dans un champ et tapez la combinaison voulue. Elle
  est appliquée immédiatement, sans redémarrage. Si elle est déjà prise par une autre
  application, les anciennes sont restaurées et l'erreur est affichée. Deux actions ne
  peuvent pas partager la même combinaison.
- **Aperçu avant remplacement** : concerne uniquement le menu au curseur — il affiche le
  résultat avec un bouton *Remplacer* au lieu de coller directement. Les raccourcis
  directs remplacent toujours sans aperçu, c'est leur raison d'être.
- **Que capturer au raccourci** : voir le tableau des modes plus haut.
- **Langue de traduction** et **consignes permanentes** (ex : « vouvoie toujours »),
  ajoutées à chaque demande.
- **Lancer au démarrage de l'ordinateur** : l'application s'ouvre à votre session,
  directement dans la zone de notification. L'entrée est posée pour votre compte
  seul — clé `HKCU\…\CurrentVersion\Run` sous Windows (visible dans le Gestionnaire
  des tâches, onglet *Démarrage*), LaunchAgent `~/Library/LaunchAgents/` sous macOS —
  et elle est réécrite au lancement si l'application a changé d'emplacement.
- **Démarrer réduit** : l'application vit dans la zone de notification. Un démarrage
  automatique est réduit de toute façon, quelle que soit cette case.

Configuration et historique sont stockés dans
`%APPDATA%\com.aitextreplacer.app\` (`config.json`, `history.json`).

> ⚠️ Les clés API sont stockées en clair dans `config.json`. Suffisant pour un poste
> personnel, à ne pas utiliser sur une machine partagée.

## Stack

- **Backend** : Rust + Tauri 1.5 — `enigo` pour la simulation clavier, `winapi` pour le
  focus et les modificateurs sous Windows, `cocoa`/`objc` pour leurs équivalents macOS
- **CI** : GitHub Actions compile les deux plateformes à chaque poussée
- **Frontend** : React + TypeScript + Vite
- Deux fenêtres partagent le même bundle : la principale et la popup
  (`index.html?view=popup`)

## Démarrage

```bash
npm install
npm run dev     # tauri dev
npm run build   # tauri build
```

Prérequis : Node 16+, Rust 1.70+, et sur Windows les outils de build MSVC
(« MSVC v143 C++ build tools » + SDK Windows) installés via Visual Studio Installer.

## Architecture

```
src/                        React + TypeScript
├── components/
│   ├── Popup.tsx           menu contextuel au curseur
│   ├── MainPanel.tsx       transformation manuelle
│   ├── SettingsPanel.tsx   raccourci, fournisseur par défaut, clés
│   └── HistoryPanel.tsx
└── types.ts                types partagés avec le backend

src-tauri/src/
├── main.rs                 fenêtres, tray, enregistrement du raccourci
├── shortcuts.rs            raccourci global et son déclencheur
├── selection.rs            capture Ctrl+C / collage Ctrl+V / focus Windows
├── popup.rs                positionnement de la popup au curseur
├── ai.rs                   appels OpenAI / Claude / Groq / local
├── config.rs               config et historique persistés
└── commands.rs             commandes exposées au frontend
```

## Limites connues

- Le remplacement sur place ne fonctionne que dans les zones de saisie qui acceptent
  Ctrl+V. Dans un champ en lecture seule, le résultat est copié dans le presse-papiers
  et un message le signale.
- macOS : l'implémentation est complète (réactivation de l'application d'origine via
  `NSRunningApplication`, position du curseur via `NSEvent`, témoin non activable via
  `orderFrontRegardless`), **mais elle n'a jamais été exécutée sur un Mac** — elle n'est
  vérifiée qu'à la compilation, par la CI. Attendez-vous à des ajustements.
- macOS : le témoin se place sous la souris et non au point d'insertion. L'équivalent du
  `GetGUIThreadInfo` de Windows demanderait un aller-retour `AXUIElement` par application.
- macOS : l'autorisation « Accessibilité » est obligatoire. Sans elle les raccourcis se
  déclenchent sans rien produire ; l'application affiche un bandeau et un lien vers le
  bon panneau des Réglages Système.

## Licence

MIT
