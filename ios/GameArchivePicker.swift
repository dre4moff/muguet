import UIKit
import UniformTypeIdentifiers

// Present the document picker itself, rather than embedding it as a child of
// a SwiftUI cover. Retain the delegate until selection/cancellation completes.
final class GameArchivePicker: NSObject, UIDocumentPickerDelegate {
    weak var presenter: UIViewController?
    private var picker: UIDocumentPickerViewController?
    private var completion: ((Result<[URL]?, Error>) -> Void)?

    func present(_ completion: @escaping (Result<[URL]?, Error>) -> Void) {
        guard let presenter, presenter.viewIfLoaded?.window != nil,
              presenter.presentedViewController == nil, picker == nil else {
            completion(.failure(PickerError.unavailable))
            return
        }
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.data], asCopy: false)
        picker.allowsMultipleSelection = true
        picker.shouldShowFileExtensions = true
        picker.modalPresentationStyle = .fullScreen
        picker.delegate = self
        self.picker = picker
        self.completion = completion
        NSLog("[muguet-import] presenting document picker")
        presenter.present(picker, animated: true) { [weak picker] in
            NSLog("[muguet-import] picker presented as modal: %@", picker?.presentingViewController != nil ? "yes" : "no")
        }
    }

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        NSLog("[muguet-import] document picker returned %ld URL(s)", urls.count)
        finish(controller, urls: urls)
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        NSLog("[muguet-import] document picker cancelled")
        finish(controller, urls: nil)
    }

    private func finish(_ controller: UIDocumentPickerViewController, urls: [URL]?) {
        guard controller === picker, let completion else { return }
        self.completion = nil
        picker = nil
        controller.dismiss(animated: true)
        // Deliver the selection directly. Import must not depend on a SwiftUI
        // onDismiss callback or on UIKit's dismissal animation completion.
        completion(.success(urls))
    }

    private enum PickerError: LocalizedError {
        case unavailable
        var errorDescription: String? {
            "Could not open Files. Try again, or import a ZIP from the Muguet folder."
        }
    }
}
