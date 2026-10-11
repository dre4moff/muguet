import SwiftUI
import UIKit
import UniformTypeIdentifiers

struct GameArchivePicker: UIViewControllerRepresentable {
    let onSelection: ([URL]?) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(onSelection) }

    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        // Providers may classify ZIPs as generic data. Validate the selected name
        // separately, and let the native importer validate the archive contents.
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.data], asCopy: false)
        // Use explicit selection + Open instead of relying on a single tap to
        // dismiss the file browser. The import accepts exactly one archive.
        picker.allowsMultipleSelection = true
        picker.shouldShowFileExtensions = true
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ picker: UIDocumentPickerViewController, context: Context) {}

    final class Coordinator: NSObject, UIDocumentPickerDelegate {
        private let onSelection: ([URL]?) -> Void
        private var completed = false

        init(_ onSelection: @escaping ([URL]?) -> Void) { self.onSelection = onSelection }

        func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
            finish(urls)
        }

        func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
            finish(nil)
        }

        private func finish(_ urls: [URL]?) {
            guard !completed else { return }
            completed = true
            onSelection(urls)
        }
    }
}
