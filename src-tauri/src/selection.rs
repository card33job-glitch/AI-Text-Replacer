//! Capture du texte sélectionné dans l'application active, et remplacement
//! de cette sélection par le texte transformé.
//!
//! Le principe : on ne peut pas lire la sélection d'une application tierce
//! (Teams, Outlook, un navigateur…) directement. On passe donc par le
//! presse-papiers, en simulant Ctrl+C puis Ctrl+V, et on restaure le contenu
//! initial du presse-papiers pour ne rien casser côté utilisateur.

use enigo::{Enigo, Key, KeyboardControllable};
use lazy_static::lazy_static;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

lazy_static! {
    /// Fenêtre qui avait le focus au moment où le raccourci a été pressé.
    /// C'est là que le résultat devra être collé.
    static ref TARGET_WINDOW: Mutex<isize> = Mutex::new(0);
    /// Contenu du presse-papiers avant qu'on ne s'en serve comme véhicule.
    static ref SAVED_CLIPBOARD: Mutex<Option<String>> = Mutex::new(None);
}

#[cfg(target_os = "windows")]
mod platform {
    use std::ptr::null_mut;
    use winapi::shared::windef::{HWND, POINT};
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{GetCurrentThreadId, OpenProcess};
    use winapi::um::sysinfoapi::GetTickCount;
    // Contrairement à `OpenProcess`, cette fonction-ci vit dans `winbase`.
    use winapi::um::winbase::QueryFullProcessImageNameW;
    use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;
    use winapi::um::winuser::{
        AttachThreadInput, BringWindowToTop, ClientToScreen, GetAsyncKeyState, GetCursorPos,
        GetForegroundWindow, GetGUIThreadInfo, GetLastInputInfo, GetWindowThreadProcessId,
        IsIconic, SetForegroundWindow, ShowWindow, GUITHREADINFO, LASTINPUTINFO, SW_RESTORE,
    };

    pub fn foreground_window() -> isize {
        unsafe { GetForegroundWindow() as isize }
    }

    /// Nom de l'exécutable de l'application au premier plan ("ms-teams.exe").
    ///
    /// Le nom de fichier, pas le chemin : c'est ce qu'un utilisateur reconnaît
    /// et peut saisir dans les Paramètres, et il ne change pas d'une
    /// installation à l'autre.
    pub fn foreground_app() -> Option<String> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 {
                return None;
            }
            // LIMITED_INFORMATION suffit pour lire le chemin de l'image et
            // reste accordé pour des processus d'intégrité plus élevée, là où
            // PROCESS_QUERY_INFORMATION serait refusé.
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let mut buffer = [0u16; 512];
            let mut size = buffer.len() as u32;
            let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut size);
            CloseHandle(process);
            if ok == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&buffer[..size as usize]);
            path.rsplit('\\').next().map(|name| name.to_string())
        }
    }

    /// Millisecondes écoulées depuis la dernière entrée clavier ou souris.
    ///
    /// Compte aussi les frappes que nous simulons : l'appelant en tient compte
    /// en prenant sa mesure de référence *après* sa propre simulation.
    pub fn idle_millis() -> u64 {
        unsafe {
            let mut info: LASTINPUTINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<LASTINPUTINFO>() as u32;
            if GetLastInputInfo(&mut info) == 0 {
                return 0;
            }
            // Les deux compteurs débordent après 49 jours, mais leur différence
            // reste juste : `wrapping_sub` évite la panique en mode debug.
            GetTickCount().wrapping_sub(info.dwTime) as u64
        }
    }

    /// Redonne le focus à une fenêtre.
    ///
    /// `SetForegroundWindow` est volontairement bridé par Windows : un
    /// processus qui n'a pas le focus ne peut pas le voler. La parade
    /// classique est d'attacher temporairement notre file d'entrées à celle
    /// du thread de la fenêtre cible, ce qui nous fait passer pour "le même"
    /// contexte d'entrée le temps de l'appel.
    pub fn focus_window(handle: isize) -> bool {
        if handle == 0 {
            return false;
        }
        unsafe {
            let hwnd = handle as HWND;
            // Déclenché au clavier, rien ne lui a pris le focus : inutile de
            // jouer la bascule d'entrées ci-dessous, qui n'est pas sans effets
            // de bord sur la fenêtre cible.
            if GetForegroundWindow() == hwnd {
                return true;
            }
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            let current = GetCurrentThreadId();
            let target = GetWindowThreadProcessId(hwnd, null_mut());
            let foreground = GetWindowThreadProcessId(GetForegroundWindow(), null_mut());

            AttachThreadInput(current, target, 1);
            AttachThreadInput(foreground, target, 1);
            BringWindowToTop(hwnd);
            let ok = SetForegroundWindow(hwnd) != 0;
            AttachThreadInput(foreground, target, 0);
            AttachThreadInput(current, target, 0);
            ok
        }
    }

    pub fn cursor_position() -> Option<(i32, i32)> {
        unsafe {
            let mut point = std::mem::zeroed();
            if GetCursorPos(&mut point) != 0 {
                Some((point.x, point.y))
            } else {
                None
            }
        }
    }

    pub const VK_SHIFT: i32 = 0x10;
    pub const VK_CONTROL: i32 = 0x11;
    pub const VK_ALT: i32 = 0x12;

    /// Vrai si la touche est physiquement enfoncée à cet instant.
    pub fn is_key_down(vk: i32) -> bool {
        unsafe { (GetAsyncKeyState(vk) as u16) & 0x8000 != 0 }
    }

    pub fn modifiers_down() -> bool {
        is_key_down(VK_SHIFT) || is_key_down(VK_CONTROL) || is_key_down(VK_ALT)
    }

    /// Position du point d'insertion dans la fenêtre cible, en coordonnées écran.
    ///
    /// Plus pertinent que la position de la souris pour poser un témoin « à côté
    /// de la phrase » : quand on déclenche au clavier, la souris peut être
    /// n'importe où. Beaucoup d'applications (dont certaines Electron) ne
    /// publient pas de caret, d'où le `None` fréquent.
    pub fn caret_position(target: isize) -> Option<(i32, i32)> {
        if target == 0 {
            return None;
        }
        unsafe {
            let mut info: GUITHREADINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            let thread = GetWindowThreadProcessId(target as HWND, null_mut());
            if GetGUIThreadInfo(thread, &mut info) == 0 || info.hwndCaret.is_null() {
                return None;
            }
            let mut point = POINT {
                x: info.rcCaret.right,
                y: info.rcCaret.bottom,
            };
            if ClientToScreen(info.hwndCaret, &mut point) == 0 {
                return None;
            }
            Some((point.x, point.y))
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use cocoa::appkit::{NSEvent, NSScreen};
    use cocoa::base::{id, nil, BOOL, YES};
    use cocoa::foundation::{NSArray, NSPoint, NSRect};
    use objc::{class, msg_send, sel, sel_impl};

    /// macOS ne raisonne pas en fenêtres mais en applications : ce qu'on
    /// mémorise est le PID de l'application au premier plan, et c'est elle
    /// qu'on réactivera pour coller le résultat.
    pub fn foreground_window() -> isize {
        unsafe {
            let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
            if workspace == nil {
                return 0;
            }
            let app: id = msg_send![workspace, frontmostApplication];
            if app == nil {
                return 0;
            }
            let pid: i32 = msg_send![app, processIdentifier];
            pid as isize
        }
    }

    /// Nom affiché de l'application au premier plan ("Mail", "Microsoft Teams").
    ///
    /// macOS n'a pas d'équivalent direct du nom d'exécutable Windows ; le nom
    /// localisé est ce que l'utilisateur lit dans le Dock, donc ce qu'il saura
    /// écrire dans les Paramètres.
    pub fn foreground_app() -> Option<String> {
        unsafe {
            let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
            if workspace == nil {
                return None;
            }
            let app: id = msg_send![workspace, frontmostApplication];
            if app == nil {
                return None;
            }
            let name: id = msg_send![app, localizedName];
            if name == nil {
                return None;
            }
            let utf8: *const std::os::raw::c_char = msg_send![name, UTF8String];
            if utf8.is_null() {
                return None;
            }
            std::ffi::CStr::from_ptr(utf8)
                .to_str()
                .ok()
                .map(|s| s.to_string())
        }
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    }

    /// Millisecondes écoulées depuis la dernière entrée clavier ou souris.
    pub fn idle_millis() -> u64 {
        // kCGEventSourceStateCombinedSessionState, kCGAnyInputEventType
        const COMBINED_SESSION_STATE: i32 = 0;
        const ANY_INPUT_EVENT: u32 = u32::MAX;
        unsafe {
            let seconds = CGEventSourceSecondsSinceLastEventType(
                COMBINED_SESSION_STATE,
                ANY_INPUT_EVENT,
            );
            if seconds.is_finite() && seconds > 0.0 {
                (seconds * 1000.0) as u64
            } else {
                0
            }
        }
    }

    pub fn focus_window(target: isize) -> bool {
        if target == 0 {
            return false;
        }
        unsafe {
            let app: id = msg_send![
                class!(NSRunningApplication),
                runningApplicationWithProcessIdentifier: target as i32
            ];
            if app == nil {
                return false;
            }
            // NSApplicationActivateIgnoringOtherApps : l'équivalent macOS du
            // SetForegroundWindow de Windows, et il n'a pas besoin de ruse
            // équivalente à AttachThreadInput.
            let options: u64 = 1 << 1;
            let activated: BOOL = msg_send![app, activateWithOptions: options];
            activated == YES
        }
    }

    /// `NSEvent` compte les pixels depuis le **bas** de l'écran principal,
    /// Tauri depuis le haut : il faut retourner l'axe vertical.
    pub fn cursor_position() -> Option<(i32, i32)> {
        unsafe {
            let point: NSPoint = NSEvent::mouseLocation(nil);
            let screens: id = NSScreen::screens(nil);
            if screens == nil || screens.count() == 0 {
                return None;
            }
            let primary: id = screens.objectAtIndex(0);
            let frame: NSRect = NSScreen::frame(primary);
            Some((point.x as i32, (frame.size.height - point.y) as i32))
        }
    }

    /// L'API d'accessibilité donnerait la position du curseur de texte, mais
    /// au prix d'un aller-retour `AXUIElement` par application. Le repli sur
    /// la souris, déjà prévu par l'appelant, suffit.
    pub fn caret_position(_target: isize) -> Option<(i32, i32)> {
        None
    }

    pub fn modifiers_down() -> bool {
        // NSEventModifierFlags
        const SHIFT: u64 = 1 << 17;
        const CONTROL: u64 = 1 << 18;
        const OPTION: u64 = 1 << 19;
        const COMMAND: u64 = 1 << 20;
        unsafe {
            let flags: u64 = msg_send![class!(NSEvent), modifierFlags];
            flags & (SHIFT | CONTROL | OPTION | COMMAND) != 0
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    pub fn foreground_window() -> isize {
        0
    }
    pub fn foreground_app() -> Option<String> {
        None
    }
    pub fn idle_millis() -> u64 {
        0
    }
    pub fn focus_window(_handle: isize) -> bool {
        false
    }
    pub fn cursor_position() -> Option<(i32, i32)> {
        None
    }
    pub fn caret_position(_target: isize) -> Option<(i32, i32)> {
        None
    }
    pub fn modifiers_down() -> bool {
        false
    }
}

pub use platform::{caret_position, cursor_position, foreground_app, foreground_window, idle_millis};

/// Mémorise la fenêtre actuellement au premier plan comme cible du remplacement.
pub fn remember_target_window() {
    *TARGET_WINDOW.lock().unwrap() = platform::foreground_window();
}

pub fn target_window() -> isize {
    *TARGET_WINDOW.lock().unwrap()
}

/// Attend que plus aucun modificateur ne soit physiquement enfoncé.
///
/// À appeler **avant chaque frappe simulée**, et pas une seule fois au début.
/// Les touches modificatrices se répètent automatiquement sous Windows : si
/// l'utilisateur tient encore son raccourci une demi-seconde, un Shift qu'on
/// aurait relâché se ré-enfonce tout seul. Le Ctrl+C envoyé ensuite devient
/// alors Ctrl+Shift+C — un raccourci que la plupart des applications
/// n'utilisent pas, et Windows répond par son bip.
///
/// En dernier recours seulement, on force le relâchement — et uniquement des
/// touches réellement enfoncées : un WM_KEYUP isolé sur Alt est interprété
/// comme un appui bref sur Alt seul, ce qui active la barre de menus et fait
/// biper les applications qui n'en ont pas.
fn wait_for_clean_modifiers(enigo: &mut Enigo) {
    const POLL_MS: u64 = 15;
    const MAX_WAIT_MS: u64 = 700;

    let mut waited = 0;
    while waited < MAX_WAIT_MS {
        if !platform::modifiers_down() {
            return;
        }
        thread::sleep(Duration::from_millis(POLL_MS));
        waited += POLL_MS;
    }

    // Filet de sécurité : l'utilisateur tient toujours ses touches.
    #[cfg(target_os = "windows")]
    {
        // Ne relâcher que ce qui est réellement enfoncé.
        if platform::is_key_down(platform::VK_SHIFT) {
            enigo.key_up(Key::Shift);
        }
        if platform::is_key_down(platform::VK_CONTROL) {
            enigo.key_up(Key::Control);
        }
        if platform::is_key_down(platform::VK_ALT) {
            enigo.key_up(Key::Alt);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Le piège du WM_KEYUP isolé sur Alt est propre à Windows : ailleurs,
        // relâcher sans condition ne coûte rien.
        enigo.key_up(Key::Shift);
        enigo.key_up(Key::Control);
        enigo.key_up(Key::Alt);
        enigo.key_up(Key::Meta);
    }
    thread::sleep(Duration::from_millis(40));
}

#[cfg(target_os = "macos")]
const CMD: Key = Key::Meta;

fn send_copy(enigo: &mut Enigo) {
    #[cfg(target_os = "macos")]
    {
        enigo.key_down(CMD);
        enigo.key_click(Key::Layout('c'));
        enigo.key_up(CMD);
    }
    #[cfg(not(target_os = "macos"))]
    {
        enigo.key_down(Key::Control);
        enigo.key_click(Key::Layout('c'));
        enigo.key_up(Key::Control);
    }
}

fn send_select_all(enigo: &mut Enigo) {
    #[cfg(target_os = "macos")]
    {
        enigo.key_down(CMD);
        enigo.key_click(Key::Layout('a'));
        enigo.key_up(CMD);
    }
    #[cfg(not(target_os = "macos"))]
    {
        enigo.key_down(Key::Control);
        enigo.key_click(Key::Layout('a'));
        enigo.key_up(Key::Control);
    }
}

fn send_paste(enigo: &mut Enigo) {
    #[cfg(target_os = "macos")]
    {
        enigo.key_down(CMD);
        enigo.key_click(Key::Layout('v'));
        enigo.key_up(CMD);
    }
    #[cfg(not(target_os = "macos"))]
    {
        enigo.key_down(Key::Control);
        enigo.key_click(Key::Layout('v'));
        enigo.key_up(Key::Control);
    }
}

/// Copie la sélection courante de l'application active et la retourne.
///
/// Retourne `Ok("")` si rien n'était sélectionné.
pub fn capture_selection(app: &AppHandle) -> Result<String, String> {
    let mode = crate::config::get(app).capture_mode;
    capture_with_mode(app, &mode)
}

/// Même capture, mais avec un mode imposé par l'appelant.
///
/// Le contrôle spontané n'a pas de sélection à lire — l'utilisateur tape, il ne
/// sélectionne rien — donc il lui faut tout le champ quel que soit le mode
/// choisi dans les Paramètres pour les raccourcis.
pub fn capture_with_mode(app: &AppHandle, mode: &str) -> Result<String, String> {
    let previous = crate::clipboard::get_clipboard(app).unwrap_or_default();
    *SAVED_CLIPBOARD.lock().unwrap() = Some(previous.clone());

    // On vide le presse-papiers pour distinguer "rien n'était sélectionné"
    // de "l'utilisateur avait déjà ce texte dans son presse-papiers".
    let _ = crate::clipboard::set_clipboard(app, String::new());

    let mut enigo = Enigo::new();
    let mut captured = String::new();

    // La copie d'emblée est une devinette : on envoie Ctrl+C sans savoir s'il y
    // a une sélection. Quand il n'y en a pas, beaucoup d'applications (dont
    // Teams) répondent par le bip système. Le mode « tout le champ » s'en passe.
    if mode != crate::config::CAPTURE_FIELD {
        wait_for_clean_modifiers(&mut enigo);
        send_copy(&mut enigo);
        captured = poll_clipboard(app);
    }

    // Ctrl+A laisse la sélection active, donc le collage remplacera bien tout
    // le champ.
    if captured.is_empty() && mode != crate::config::CAPTURE_SELECTION {
        wait_for_clean_modifiers(&mut enigo);
        send_select_all(&mut enigo);
        thread::sleep(Duration::from_millis(80));
        wait_for_clean_modifiers(&mut enigo);
        send_copy(&mut enigo);
        captured = poll_clipboard(app);
    }

    // Le presse-papiers a joué son rôle de véhicule, on rend à l'utilisateur
    // ce qu'il y avait avant.
    let _ = crate::clipboard::set_clipboard(app, previous);

    Ok(captured)
}

/// Replie la sélection laissée par un `Ctrl+A` dont on ne fera rien.
///
/// **Indispensable après toute capture non suivie d'un collage.** `Ctrl+A`
/// laisse le champ entièrement sélectionné : la frappe suivante de
/// l'utilisateur effacerait tout ce qu'il a écrit. La flèche droite replie la
/// sélection sur sa fin, et le point d'insertion se retrouve à la fin du champ.
pub fn collapse_selection() {
    let mut enigo = Enigo::new();
    wait_for_clean_modifiers(&mut enigo);
    enigo.key_click(Key::RightArrow);
}

/// Attend que l'application cible ait honoré le Ctrl+C.
///
/// Les applications Electron (Teams, Slack, VS Code) sont nettement plus lentes
/// que les applications natives : on sonde plutôt que d'attendre un délai fixe
/// généreux, ce qui rend le cas rapide rapide sans pénaliser le cas lent.
fn poll_clipboard(app: &AppHandle) -> String {
    for _ in 0..12 {
        thread::sleep(Duration::from_millis(40));
        if let Ok(text) = crate::clipboard::get_clipboard(app) {
            if !text.is_empty() {
                return text;
            }
        }
    }
    String::new()
}

/// Colle un texte au point d'insertion de l'application d'origine, sans rien
/// capturer au préalable.
///
/// Sert aux textes figés : il n'y a pas de sélection à lire, seulement un
/// contenu à déposer. S'il se trouve qu'une sélection est active, elle est
/// remplacée — c'est le comportement normal d'un collage, et celui qu'attend
/// quelqu'un qui sélectionne avant d'insérer.
pub fn insert_text(app: &AppHandle, text: String) -> Result<(), String> {
    // `replace_selection` restaure ce que `capture_selection` avait mis de côté.
    // Ici personne n'a capturé : c'est à nous de sauvegarder le presse-papiers,
    // sinon le texte figé y resterait à la place du contenu de l'utilisateur.
    let previous = crate::clipboard::get_clipboard(app).unwrap_or_default();
    *SAVED_CLIPBOARD.lock().unwrap() = Some(previous);
    replace_selection(app, text)
}

/// Remet le focus sur l'application d'origine et y colle `text`,
/// ce qui remplace la sélection encore active.
pub fn replace_selection(app: &AppHandle, text: String) -> Result<(), String> {
    let handle = target_window();
    if !platform::focus_window(handle) {
        return Err(
            "Impossible de redonner le focus à l'application d'origine. Le résultat a été copié dans le presse-papiers."
                .to_string(),
        );
    }

    // Laisser à la fenêtre le temps de reprendre le focus clavier.
    thread::sleep(Duration::from_millis(120));

    crate::clipboard::set_clipboard(app, text)?;
    thread::sleep(Duration::from_millis(60));

    let mut enigo = Enigo::new();
    wait_for_clean_modifiers(&mut enigo);
    send_paste(&mut enigo);

    // Restaurer le presse-papiers trop tôt ferait coller l'ancien contenu.
    thread::sleep(Duration::from_millis(350));
    if let Some(previous) = SAVED_CLIPBOARD.lock().unwrap().take() {
        let _ = crate::clipboard::set_clipboard(app, previous);
    }

    Ok(())
}
