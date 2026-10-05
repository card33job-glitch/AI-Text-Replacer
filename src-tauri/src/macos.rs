//! Appels macOS qui ne tolèrent pas d'être faits depuis n'importe quel thread.
//!
//! Les raccourcis, le contrôle spontané et les commandes travaillent sur des
//! threads à eux. Or AppKit exige le thread principal pour toucher à une
//! fenêtre, et l'API de disposition clavier (`TISGetInputSourceProperty`)
//! fait planter le processus depuis macOS 14 si on l'appelle ailleurs — c'est
//! elle qu'enigo appelle pour chaque `Key::Layout`. On passe donc par ici.

use std::ffi::c_void;

type CFTypeRef = *const c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventSourceCreate(state: i32) -> CFTypeRef;
    fn CGEventCreateKeyboardEvent(source: CFTypeRef, keycode: u16, down: bool) -> CFTypeRef;
    fn CGEventSetFlags(event: CFTypeRef, flags: u64);
    fn CGEventPost(tap: u32, event: CFTypeRef);
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    static kTISPropertyUnicodeKeyLayoutData: CFTypeRef;
    fn TISCopyCurrentKeyboardLayoutInputSource() -> CFTypeRef;
    fn TISGetInputSourceProperty(source: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn LMGetKbdType() -> u8;
    fn UCKeyTranslate(
        layout: *const c_void,
        keycode: u16,
        action: u16,
        modifiers: u32,
        keyboard_type: u32,
        options: u32,
        dead_key_state: *mut u32,
        max_length: usize,
        actual_length: *mut usize,
        chars: *mut u16,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFDataGetBytePtr(data: CFTypeRef) -> *const u8;
}

extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_sync_f(queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn pthread_main_np() -> i32;
}

/// Exécute `f` sur le thread principal et attend son résultat.
///
/// Appelée depuis le thread principal lui-même, elle exécute `f` sur place :
/// un `dispatch_sync` vers sa propre file serait un interblocage.
pub fn on_main<F: FnOnce() -> R, R>(f: F) -> R {
    if unsafe { pthread_main_np() } != 0 {
        return f();
    }

    struct Job<F, R> {
        f: Option<F>,
        out: Option<R>,
    }
    extern "C" fn run<F: FnOnce() -> R, R>(context: *mut c_void) {
        let job = unsafe { &mut *(context as *mut Job<F, R>) };
        job.out = job.f.take().map(|f| f());
    }

    let mut job = Job { f: Some(f), out: None };
    unsafe {
        dispatch_sync_f(
            &_dispatch_main_q as *const c_void,
            &mut job as *mut Job<F, R> as *mut c_void,
            run::<F, R>,
        );
    }
    job.out.expect("tâche du thread principal non exécutée")
}

/// Touche physique qui produit `letter` dans la disposition clavier active.
///
/// `Cmd+C` doit viser la touche marquée « C », où qu'elle soit : en AZERTY,
/// le code « A » d'un clavier américain donnerait `Cmd+Q`, qui quitte
/// l'application. Repli sur la position américaine si la disposition ne
/// publie pas de table (certaines méthodes de saisie asiatiques).
fn keycode_for(letter: char) -> u16 {
    let wanted = letter as u16;
    let found = on_main(|| unsafe {
        let source = TISCopyCurrentKeyboardLayoutInputSource();
        if source.is_null() {
            return None;
        }
        let data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
        let mut result = None;
        if !data.is_null() {
            let layout = CFDataGetBytePtr(data) as *const c_void;
            let keyboard = LMGetKbdType() as u32;
            for keycode in 0..128u16 {
                let mut dead = 0u32;
                let mut length = 0usize;
                let mut chars = [0u16; 4];
                // kUCKeyActionDisplay, sans modificateur ni touche morte.
                let status = UCKeyTranslate(
                    layout,
                    keycode,
                    3,
                    0,
                    keyboard,
                    1,
                    &mut dead,
                    chars.len(),
                    &mut length,
                    chars.as_mut_ptr(),
                );
                if status == 0 && length == 1 && chars[0] == wanted {
                    result = Some(keycode);
                    break;
                }
            }
        }
        CFRelease(source);
        result
    });

    found.unwrap_or(match letter {
        'a' => 0,
        'c' => 8,
        'v' => 9,
        'r' => 15,
        _ => 0,
    })
}

/// Envoie `Cmd` + une lettre (`Cmd+C`, `Cmd+V`…) à l'application active.
///
/// Le drapeau Commande est posé explicitement sur la touche : enigo se contente
/// d'enfoncer Commande avant, et certaines applications (Electron notamment)
/// lisent les drapeaux de l'évènement plutôt que l'état du clavier.
pub fn press_command(letter: char) {
    // kCGEventSourceStateHIDSystemState, kCGHIDEventTap, kVK_Command
    const HID_SYSTEM_STATE: i32 = 1;
    const HID_EVENT_TAP: u32 = 0;
    const COMMAND_KEY: u16 = 55;
    const COMMAND_FLAG: u64 = 1 << 20;

    let keycode = keycode_for(letter);
    unsafe {
        let source = CGEventSourceCreate(HID_SYSTEM_STATE);
        let post = |key: u16, down: bool, flags: u64| {
            let event = CGEventCreateKeyboardEvent(source, key, down);
            if !event.is_null() {
                CGEventSetFlags(event, flags);
                CGEventPost(HID_EVENT_TAP, event);
                CFRelease(event);
            }
            std::thread::sleep(std::time::Duration::from_millis(8));
        };
        post(COMMAND_KEY, true, COMMAND_FLAG);
        post(keycode, true, COMMAND_FLAG);
        post(keycode, false, COMMAND_FLAG);
        post(COMMAND_KEY, false, 0);
        if !source.is_null() {
            CFRelease(source);
        }
    }
}
