// muguet - pad.rs
// Constantin Clerc (c22dev) - 0.1

use crate::input;
use objc2_game_controller::{GCController, GCControllerButtonInput, GCExtendedGamepad};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

pub static ACTIVE: AtomicBool = AtomicBool::new(false);
pub static MENU_ACTIVE: AtomicBool = AtomicBool::new(false);
pub static FOCUSED: AtomicBool = AtomicBool::new(true);

pub static IN_VEHICLE: AtomicBool = AtomicBool::new(false);

pub static HORN_PAD: AtomicBool = AtomicBool::new(false);
pub static HORN_TOUCH: AtomicBool = AtomicBool::new(false);
pub fn horn_held() -> bool {
    HORN_PAD.load(Ordering::Relaxed) || HORN_TOUCH.load(Ordering::Relaxed)
}

struct State {
    held: HashSet<&'static str>,
    buttons: u8,
    last: Instant,
}
static STATE: Mutex<Option<State>> = Mutex::new(None);

const STICK_DEADZONE: f32 = 0.3;
const LOOK_SPEED: f64 = 1400.0;

fn gamepad() -> Option<objc2::rc::Retained<GCExtendedGamepad>> {
    unsafe { GCController::controllers().iter().find_map(|c| c.extendedGamepad()) }
}

fn down(b: &GCControllerButtonInput) -> bool {
    unsafe { b.isPressed() }
}

pub fn poll() -> bool {
    if !FOCUSED.load(Ordering::Relaxed) { release_all(); return false; }
    let Some(gp) = gamepad() else {
        if ACTIVE.swap(false, Ordering::Relaxed) {
            release_all();
            eprintln!("[pad] controller disconnected");
        }
        return false;
    };
    if !ACTIVE.swap(true, Ordering::Relaxed) {
        eprintln!("[pad] controller connected");
    }
    let mut guard = STATE.lock().unwrap();
    let st = guard.get_or_insert_with(|| State { held: HashSet::new(), buttons: 0, last: Instant::now() });
    let dt = st.last.elapsed().as_secs_f64().min(0.05);
    st.last = Instant::now();

    let in_car = IN_VEHICLE.load(Ordering::Relaxed);
    let paused = MENU_ACTIVE.load(Ordering::Relaxed);
    let gain = std::env::var("MUGUET_PAD_SENS").ok().and_then(|s| s.parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(1.5).clamp(0.25, 4.0);
    let deadzone = std::env::var("MUGUET_PAD_DEADZONE").ok().and_then(|s| s.parse::<f32>().ok()).filter(|v| v.is_finite()).unwrap_or(0.12).clamp(0.05, 0.30);
    HORN_PAD.store(false, Ordering::Relaxed);
    let mut keys: HashSet<&'static str> = HashSet::new();
    let mut buttons = 0u8;
    unsafe {
        let ls = gp.leftThumbstick();
        let (lx, ly) = (ls.xAxis().value(), ls.yAxis().value());
        if lx < -STICK_DEADZONE { keys.insert("KeyA"); }
        if lx > STICK_DEADZONE { keys.insert("KeyD"); }
        if paused {
            keys.clear();
            if lx < -STICK_DEADZONE { keys.insert("ArrowLeft"); }
            if lx > STICK_DEADZONE { keys.insert("ArrowRight"); }
            if ly > STICK_DEADZONE { keys.insert("ArrowUp"); }
            if ly < -STICK_DEADZONE { keys.insert("ArrowDown"); }
        } else if in_car {
            if ly > STICK_DEADZONE { keys.insert("Numpad8"); }
            if ly < -STICK_DEADZONE { keys.insert("Numpad2"); }
            if gp.rightTrigger().value() > 0.2 { keys.insert("KeyW"); }
            if gp.leftTrigger().value() > 0.2 { keys.insert("KeyS"); }
            if down(&gp.rightShoulder()) { keys.insert("Space"); }
            HORN_PAD.store(gp.leftThumbstickButton().is_some_and(|b| down(&b)), Ordering::Relaxed);
            if down(&gp.buttonX()) { keys.insert("Space"); }
        } else {
            if ly > STICK_DEADZONE { keys.insert("KeyW"); }
            if ly < -STICK_DEADZONE { keys.insert("KeyS"); }
            if gp.rightTrigger().value() > 0.2 { buttons |= 1; }
            if gp.leftTrigger().value() > 0.2 { buttons |= 2; }
            if down(&gp.buttonA()) { keys.insert("ShiftLeft"); }
            if down(&gp.buttonX()) { keys.insert("Space"); }
            if down(&gp.rightShoulder()) { keys.insert("KeyQ"); }
            if gp.leftThumbstickButton().is_some_and(|b| down(&b)) { keys.insert("ControlLeft"); }
        }
        // The phone and mission prompts also consume Enter/Backspace. Keep those
        // navigation keys, but never emit sprint/reload while the pause menu is active.
        if down(&gp.buttonA()) { keys.insert("Enter"); }
        if down(&gp.buttonB()) {
            keys.insert("Backspace");
            if !paused { keys.insert("KeyR"); }
        }
        if !paused {
            if down(&gp.buttonY()) { keys.insert("KeyF"); }
            if down(&gp.leftShoulder()) { keys.insert("Tab"); }
            if gp.rightThumbstickButton().is_some_and(|b| down(&b)) { keys.insert("KeyC"); }
        }
        let dp = gp.dpad();
        keys.extend(crate::controller::dpad_keys(paused, in_car, down(&dp.up()), down(&dp.down()), down(&dp.left()), down(&dp.right())));
        if down(&gp.buttonMenu()) { keys.insert("Escape"); }
        if gp.buttonOptions().is_some_and(|b| down(&b)) { keys.insert("KeyV"); }

        let rs = gp.rightThumbstick();
        let (dx, dy) = crate::controller::look(rs.xAxis().value(), rs.yAxis().value(), deadzone);
        if !paused && (dx != 0.0 || dy != 0.0) {
            input::controller_delta_f(dx * LOOK_SPEED * gain * dt, dy * LOOK_SPEED * gain * dt);
        }
    }

    if cfg!(target_os = "ios") { input::set_locked(true); }

    for k in st.held.difference(&keys) { input::key(k, false, false, None, false); }
    for k in keys.difference(&st.held) {
        crate::mp::key_pressed(k);
        input::key(k, true, false, None, false);
    }
    st.held = keys;
    for (bit, dom) in [(1u8, 0usize), (2, 2)] {
        if (buttons ^ st.buttons) & bit != 0 { input::button(dom, buttons & bit != 0); }
    }
    st.buttons = buttons;
    true
}

fn release(st: State) {
    for k in st.held { input::key(k, false, false, None, false); }
    if st.buttons & 1 != 0 { input::button(0, false); }
    if st.buttons & 2 != 0 { input::button(2, false); }
}

pub fn release_all() {
    HORN_PAD.store(false, Ordering::Relaxed);
    if let Some(st) = STATE.lock().unwrap().take() { release(st); }
}
