import Foundation

enum GameArchiveSelectionError: LocalizedError {
    case chooseOneArchive
    case notZIP

    var errorDescription: String? {
        switch self {
        case .chooseOneArchive: return "Select exactly one game-data ZIP, then tap Open."
        case .notZIP: return "Select a .zip file containing your game data."
        }
    }
}

enum GameArchiveSelection {
    static func localArchives(in directory: URL) throws -> [URL] {
        let keys: Set<URLResourceKey> = [.isRegularFileKey, .isSymbolicLinkKey]
        return try FileManager.default.contentsOfDirectory(at: directory,
                    includingPropertiesForKeys: Array(keys), options: .skipsHiddenFiles)
            .filter {
                guard $0.pathExtension.lowercased() == "zip" else { return false }
                let values = try $0.resourceValues(forKeys: keys)
                return values.isRegularFile == true && values.isSymbolicLink != true
            }
            .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
    }

    static func archive(from urls: [URL]) throws -> URL {
        guard urls.count == 1 else { throw GameArchiveSelectionError.chooseOneArchive }
        let url = urls[0]
        guard url.isFileURL, url.pathExtension.lowercased() == "zip" else {
            throw GameArchiveSelectionError.notZIP
        }
        return url
    }
}
