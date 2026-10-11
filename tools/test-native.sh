#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
mkdir -p .build/work .build/logs
xcrun swiftc -O -module-cache-path .build/sdk-cache/swift-native -o .build/work/native-tests ios/SourceArchiveCleanup.swift ios/GameArchiveSelection.swift ios/GameArchiveAccess.swift tests/native/main.swift
MUGUET_NATIVE_TEST_TEMP_ROOT="$PWD/.build/work" .build/work/native-tests
