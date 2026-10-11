import Foundation

enum GameArchiveAccessError: LocalizedError {
    case unavailable
    var errorDescription: String? { "The selected ZIP could not be accessed safely." }
}

enum GameArchiveAccess {
    // Called on the import worker, never on the UI thread. Providers may need
    // to materialize the file before the native ZIP reader can access it.
    static func read<T>(_ url: URL, _ body: (URL) throws -> T) throws -> T {
        var coordinationError: NSError?
        var result: Result<T, Error>?
        NSFileCoordinator(filePresenter: nil).coordinate(readingItemAt: url, options: [], error: &coordinationError) {
            coordinatedURL in result = Result { try body(coordinatedURL) }
        }
        if let coordinationError { throw coordinationError }
        guard let result else { throw GameArchiveAccessError.unavailable }
        return try result.get()
    }

    static func delete(_ url: URL, _ body: (URL) throws -> Void) throws {
        var coordinationError: NSError?
        var result: Result<Void, Error>?
        NSFileCoordinator(filePresenter: nil).coordinate(writingItemAt: url, options: .forDeleting, error: &coordinationError) {
            coordinatedURL in result = Result { try body(coordinatedURL) }
        }
        if let coordinationError { throw coordinationError }
        guard let result else { throw GameArchiveAccessError.unavailable }
        try result.get()
    }
}
