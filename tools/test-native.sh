#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
mkdir -p .build/work .build/logs
xcrun swiftc -O -o .build/work/native-tests ios/SourceArchiveCleanup.swift ios/TemporalUpscaler.swift tests/native/main.swift
.build/work/native-tests
