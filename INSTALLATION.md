# 🚀 Guide d'installation et de première utilisation

## Étape 1 — Prérequis

### Windows

```powershell
# Rust : https://rustup.rs/
rustc --version
cargo --version
node --version
```

Il faut aussi les **outils de build C++ MSVC**, sans quoi la compilation Rust échoue
au moment de l'édition de liens :
Visual Studio Installer → *Modifier* → onglet **Composants individuels** →
cocher « MSVC v143 – VS 2022 C++ x64/x86 build tools » et le **SDK Windows 11**.

> Si `cargo` n'est pas reconnu, il est installé dans `%USERPROFILE%\.cargo\bin` sans
> être ajouté au PATH :
> `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"`
>
> Lancez les commandes `cargo` depuis **PowerShell**, pas Git Bash : Git Bash fournit
> son propre `link.exe` qui masque celui de MSVC et produit une erreur
> `link: extra operand` trompeuse.

### macOS

```bash
brew install rust
```

## Étape 2 — Installation

```powershell
cd "C:\Application Utile\AI Text Replacer"
npm install
```

## Étape 3 — Lancer

```powershell
npm run dev      # mode développement
npm run build    # exécutable dans src-tauri/target/release/
```

## Étape 4 — Configurer

Onglet **Paramètres** :

1. **Raccourci clavier global** — cliquez dans le champ et tapez la combinaison voulue
   (`Ctrl+Shift+T` par défaut). Elle prend effet dès la sauvegarde.
2. **Fournisseur par défaut** — celui qu'utilisera le menu au curseur.
3. **Clé API du fournisseur choisi** :
   - Groq (gratuit) : https://console.groq.com
   - Claude : https://console.anthropic.com
   - OpenAI : https://platform.openai.com/api-keys
   - Modèle local : aucune clé, démarrez simplement Ollama (`ollama serve`)
4. **Lancer au démarrage de l'ordinateur** — cochez la case pour que l'application
   soit là à chaque ouverture de session, sans avoir à y penser.
5. **Sauvegarder les paramètres**.

> La case de démarrage automatique enregistre le chemin de l'exécutable tel qu'il est
> au moment de la sauvegarde. Si vous déplacez l'application, relancez-la une fois :
> l'entrée est corrigée toute seule au lancement.

## Étape 5 — Tester dans Teams

1. Ouvrez une conversation Teams et tapez un message **sans l'envoyer** :
   `bonjour je voudrai savoir si il serais possible de decaler la reunion`
2. **Sélectionnez** ce texte à la souris ou avec `Ctrl+A` dans la zone de saisie
3. Appuyez sur `Ctrl+Shift+T`
4. Le menu apparaît près du curseur → cliquez sur **Corriger la grammaire**
5. Le texte est remplacé sur place dans la zone de saisie Teams

Le même scénario fonctionne dans Outlook, Word, un champ de navigateur, VS Code, etc.

## 🔧 Dépannage

### Le raccourci ne déclenche rien
- Une autre application l'utilise déjà : changez-le dans Paramètres. L'application
  affiche une erreur explicite si l'enregistrement échoue.
- Vérifiez que l'application tourne (icône dans la zone de notification). Fermer la
  fenêtre la met en veille, elle ne la quitte pas — utilisez *Quitter* dans le menu de
  l'icône.

### Le menu apparaît mais annonce « Aucun texte sélectionné »
- Vérifiez le mode dans Paramètres → **Que capturer au raccourci**. En mode « Ma
  sélection uniquement », le texte doit être sélectionné au moment de la pression.
- Certaines applications mettent du temps à répondre au Ctrl+C simulé ; réessayez.
- Une zone en lecture seule qui interdit la copie ne peut pas être lue.

### Windows bipe quand j'appuie sur le raccourci
Le bip vient d'un `Ctrl+C` envoyé alors que rien n'est sélectionné : Teams et beaucoup
d'applications répondent ainsi à une copie sans objet. Passez Paramètres → **Que capturer
au raccourci** sur **« Tout le champ de saisie »** : ce mode envoie `Ctrl+A` avant de
copier, donc il n'y a jamais de copie à vide.

### Le texte n'est pas remplacé mais copié
C'est le repli prévu : Windows a refusé de rendre le focus à l'application d'origine,
ou la zone n'accepte pas le collage. Le résultat est dans le presse-papiers, un
`Ctrl+V` manuel suffit.

### Erreur « Aucune clé API configurée »
Le fournisseur par défaut n'a pas de clé. Paramètres → section **Fournisseurs IA**.

### Erreur de compilation Rust
```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cd src-tauri
cargo clean
cargo build
```

## 📚 Ressources

- [Documentation Tauri](https://tauri.app/)
- [Groq Console](https://console.groq.com)
- [Ollama](https://ollama.com) pour un modèle 100 % local
