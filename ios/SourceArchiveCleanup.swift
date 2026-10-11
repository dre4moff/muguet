// Delete only the selected, unchanged source archive after a successful import.
import Foundation

enum ArchiveCleanupError: LocalizedError {
    case identityUnavailable, changed
    var errorDescription: String? {
        switch self {
        case .identityUnavailable: return "The original archive could not be identified safely."
        case .changed: return "The selected archive changed during the import."
        }
    }
}

struct SourceArchiveIdentity: Equatable {
    let file: UInt64
    let size: UInt64
    let modified: Date
    static func read(_ url: URL) throws -> Self {
        let a = try FileManager.default.attributesOfItem(atPath: url.path)
        guard a[.type] as? FileAttributeType == .typeRegular,
              url.pathExtension.lowercased() == "zip",
              let file = a[.systemFileNumber] as? NSNumber,
              let size = a[.size] as? NSNumber,
              let modified = a[.modificationDate] as? Date else {
            throw ArchiveCleanupError.identityUnavailable
        }
        return Self(file: file.uint64Value, size: size.uint64Value, modified: modified)
    }
    func removeUnchangedArchive(at url: URL) throws {
        try GameArchiveAccess.delete(url) { coordinatedURL in
            guard try Self.read(coordinatedURL) == self else { throw ArchiveCleanupError.changed }
            try FileManager.default.removeItem(at: coordinatedURL)
        }
    }
}
