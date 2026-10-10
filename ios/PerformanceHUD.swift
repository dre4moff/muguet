// Muguet fork r1 — native, non-interactive performance overlay.
// GPL-3.0-or-later; derived application retains the upstream license.
import UIKit

private var performanceLabel: UILabel?

@_cdecl("muguet_hud_install")
public func muguetHUDInstall(_ controllerPointer: UnsafeMutableRawPointer) {
    let controller = Unmanaged<UIViewController>.fromOpaque(controllerPointer).takeUnretainedValue()
    let label = UILabel()
    label.font = .monospacedSystemFont(ofSize: 11, weight: .medium)
    label.textColor = .white
    label.backgroundColor = UIColor.black.withAlphaComponent(0.72)
    label.layer.cornerRadius = 7
    label.clipsToBounds = true
    label.numberOfLines = 0
    label.isUserInteractionEnabled = false
    label.accessibilityLabel = "Performance monitor"
    label.translatesAutoresizingMaskIntoConstraints = false
    controller.view.addSubview(label)
    NSLayoutConstraint.activate([
        label.topAnchor.constraint(equalTo: controller.view.safeAreaLayoutGuide.topAnchor, constant: 6),
        label.leadingAnchor.constraint(equalTo: controller.view.safeAreaLayoutGuide.leadingAnchor, constant: 6),
        label.widthAnchor.constraint(lessThanOrEqualTo: controller.view.widthAnchor, multiplier: 0.65)
    ])
    label.text = "  Muguet — waiting for frames  "
    performanceLabel?.removeFromSuperview()
    performanceLabel = label
}

@_cdecl("muguet_hud_update")
public func muguetHUDUpdate(_ text: UnsafePointer<CChar>) {
    let message = String(cString: text)
    DispatchQueue.main.async {
        let thermal: String
        switch ProcessInfo.processInfo.thermalState {
        case .nominal: thermal = "Normal"
        case .fair: thermal = "Warm"
        case .serious: thermal = "Hot"
        case .critical: thermal = "Critical"
        @unknown default: thermal = "Unknown"
        }
        performanceLabel?.text = "  \(message.replacingOccurrences(of: "\n", with: "  \n  "))  \n  Thermal: \(thermal) · Low power: \(ProcessInfo.processInfo.isLowPowerModeEnabled ? "On" : "Off")  "
    }
}
