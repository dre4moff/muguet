import Foundation

func require(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() { fatalError(message) }
}

let testRoot = ProcessInfo.processInfo.environment["MUGUET_NATIVE_TEST_TEMP_ROOT"]
    .map { URL(fileURLWithPath: $0, isDirectory: true) } ?? FileManager.default.temporaryDirectory
let directory = testRoot.resolvingSymlinksInPath().appendingPathComponent("muguet-native-tests-\(UUID().uuidString)")
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
defer { try? FileManager.default.removeItem(at: directory) }
let original = directory.appendingPathComponent("original.zip")
try Data([1,2,3]).write(to: original)
let identity = try SourceArchiveIdentity.read(original)
try identity.removeUnchangedArchive(at: original)
require(!FileManager.default.fileExists(atPath: original.path), "Successful import cleanup must delete selected ZIP")
try Data([1,2,3]).write(to: original)
let changed = try SourceArchiveIdentity.read(original)
try Data([1,2,3,4]).write(to: original)
do { try changed.removeUnchangedArchive(at: original); fatalError("Changed source ZIP was deleted") }
catch ArchiveCleanupError.changed { }
require(FileManager.default.fileExists(atPath: original.path), "Changed source must survive")
let nonZIP = directory.appendingPathComponent("data.txt")
try Data([1]).write(to: nonZIP)
do { _ = try SourceArchiveIdentity.read(nonZIP); fatalError("Non-ZIP accepted for deletion") }
catch ArchiveCleanupError.identityUnavailable { }
print("PASS: selected unchanged ZIP cleanup; modified ZIP and non-ZIP protection")

let selected = directory.appendingPathComponent("Game Data.ZIP")
let selectedArchive = try GameArchiveSelection.archive(from: [selected])
require(selectedArchive == selected, "Selection must preserve the original provider URL")
for urls in [[], [selected, original]] {
    do { _ = try GameArchiveSelection.archive(from: urls); fatalError("Non-single selection accepted") }
    catch GameArchiveSelectionError.chooseOneArchive { }
}
for url in [nonZIP, URL(string: "https://example.com/game.zip")!] {
    do { _ = try GameArchiveSelection.archive(from: [url]); fatalError("Non-file or non-ZIP accepted") }
    catch GameArchiveSelectionError.notZIP { }
}
require(FileManager.default.fileExists(atPath: original.path), "Rejected selection must leave archives untouched")
require(FileManager.default.fileExists(atPath: nonZIP.path), "Rejected selection must leave other files untouched")
print("PASS: single original ZIP URL, uppercase extension; empty/multiple/non-ZIP/remote rejection without deletion")

try Data([5,6,7]).write(to: selected)
let hidden = directory.appendingPathComponent(".hidden.zip")
try Data([8]).write(to: hidden)
let fakeZIP = directory.appendingPathComponent("directory.zip")
try FileManager.default.createDirectory(at: fakeZIP, withIntermediateDirectories: false)
let link = directory.appendingPathComponent("link.zip")
try FileManager.default.createSymbolicLink(at: link, withDestinationURL: selected)
let nested = directory.appendingPathComponent("nested")
try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: false)
try Data([9]).write(to: nested.appendingPathComponent("nested.zip"))
let archives = try GameArchiveSelection.localArchives(in: directory)
require(Set(archives.map { $0.path }) == Set([selected.path, original.path]), "Local import must list only visible regular ZIPs in the app folder")
let readBytes = try GameArchiveAccess.read(selected) { try Data(contentsOf: $0) }
require(readBytes == Data([5,6,7]), "Coordinated read must deliver the selected file bytes")
enum TestFailure: Error { case failedImport }
do {
    try GameArchiveAccess.read(selected) { _ in throw TestFailure.failedImport }
    fatalError("Import failure was swallowed")
} catch TestFailure.failedImport { }
require(FileManager.default.fileExists(atPath: selected.path), "Failed coordinated import must retain source")
try SourceArchiveIdentity.read(selected).removeUnchangedArchive(at: selected)
require(!FileManager.default.fileExists(atPath: selected.path), "Coordinated successful cleanup must remove unchanged source")
print("PASS: app-folder ZIP discovery excludes hidden/nested/symlink/directory entries; coordinated read, failure retention and deletion")
