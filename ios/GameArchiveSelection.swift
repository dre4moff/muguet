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
    static func archive(from urls: [URL]) throws -> URL {
        guard urls.count == 1 else { throw GameArchiveSelectionError.chooseOneArchive }
        let url = urls[0]
        guard url.isFileURL, url.pathExtension.lowercased() == "zip" else {
            throw GameArchiveSelectionError.notZIP
        }
        return url
    }
}
