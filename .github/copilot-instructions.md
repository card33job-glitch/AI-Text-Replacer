# AI Text Replacer - Setup Instructions

## Project Overview
Tauri + React + TypeScript desktop application for AI-powered text transformations with global keyboard shortcuts, multi-model support, request history, and advanced auto-selection feature.

## Key Features
- ✅ Text selection + keyboard shortcut (more reliable than universal right-click)
- ✅ Rephrase in different styles
- ✅ Instant translation
- ✅ Grammar correction
- ✅ Summarization
- ✅ Tone adjustment (professional, friendly, formal, etc.)
- ✅ Multi-model support (OpenAI, Claude, Groq, local models)
- ✅ Request history with full transformation records
- ✅ Custom prompt library
- ✅ **Auto-selection** - Automatically capture last typed text when shortcut is pressed

## Development Stack
- **Framework**: Tauri (Rust backend)
- **Frontend**: React + TypeScript
- **Features**: Global hotkeys, clipboard management, multi-model AI integration, auto-selection of recently typed text
- **Platform Support**: Windows + macOS

## Project Structure
```
src/                           # React + TypeScript frontend
├── components/
│   ├── MainPanel.tsx         # Main transformation UI
│   ├── SettingsPanel.tsx     # Configuration & API keys
│   └── HistoryPanel.tsx      # View transformation history
├── styles/                    # Component stylesheets
└── App.tsx                   # Main app component

src-tauri/                     # Rust backend
├── src/
│   ├── main.rs               # Tauri app entry point
│   ├── commands.rs           # IPC commands for frontend
│   ├── clipboard.rs          # Clipboard operations
│   ├── shortcuts.rs          # Global shortcut management
│   └── auto_select.rs        # Auto-selection logic
└── Cargo.toml
```

## Getting Started
```bash
npm install
npm run tauri dev
```

## Configuration
1. **Global Shortcut**: Customize in Settings (default: Ctrl+Shift+T)
2. **Auto-Selection**: Enable to capture last typed text automatically
3. **API Keys**: Add OpenAI, Claude, or Groq keys in Settings
4. **Auto-Open**: Option to automatically open app when shortcut is pressed

## Auto-Selection Feature
Captures the last text typed in any application when the global shortcut is pressed. Supports both Windows and macOS with native APIs. Configurable in Settings panel.

## Build & Deploy
```bash
npm run tauri build    # Production build for current platform
```

## Dependencies
- React 18.3+
- Tauri 1.5+
- TypeScript 5.4+
- Vite 5.2+
- Rust 1.56+ (for Tauri)

