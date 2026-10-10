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
