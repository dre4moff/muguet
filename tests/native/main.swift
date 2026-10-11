import Foundation

func require(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() { fatalError(message) }
}

let directory = FileManager.default.temporaryDirectory.appendingPathComponent("muguet-native-tests-\(UUID().uuidString)")
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
