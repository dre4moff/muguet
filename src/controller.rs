// Pure controller filtering, independent of touch/mouse sensitivity.
pub fn look(x: f32, y: f32, deadzone: f32) -> (f64, f64) {
    let length = x.hypot(y);
    if !length.is_finite() || length <= deadzone { return (0.0, 0.0); }
    let magnitude = ((length.min(1.0) - deadzone) / (1.0 - deadzone)).powf(1.25);
    ((x / length * magnitude) as f64, (-y / length * magnitude) as f64)
}

pub fn dpad_keys(paused: bool, in_car: bool, up: bool, down: bool, left: bool, right: bool) -> Vec<&'static str> {
    let mut keys = Vec::new();
    if up { keys.push("ArrowUp"); }
    // Retain phone navigation; game scripts gate phone and gameplay controls.
    if down { keys.push("ArrowDown"); if !paused { keys.push("AltLeft"); } }
    if left { keys.push("ArrowLeft"); if !paused && in_car { keys.push("KeyQ"); } }
    if right { keys.push("ArrowRight"); }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn radial_filter_preserves_direction_and_caps_diagonals() {
        assert_eq!(look(0.06, 0.06, 0.12), (0.0, 0.0));
        let (x, y) = look(1.0, 1.0, 0.12);
        assert!((x.hypot(y) - 1.0).abs() < 1e-6);
        assert!((x + y).abs() < 1e-6);
        assert!(look(0.5, 0.0, 0.12).0 > 0.3);
    }
    #[test] fn held_wheel_keys_do_not_become_menu_arrows() {
        assert_eq!(dpad_keys(false, true, false, true, true, false), vec!["ArrowDown", "AltLeft", "ArrowLeft", "KeyQ"]);
        assert_eq!(dpad_keys(true, true, false, true, true, false), vec!["ArrowDown", "ArrowLeft"]);
        assert!(dpad_keys(false, true, false, false, false, false).is_empty());
    }
}
