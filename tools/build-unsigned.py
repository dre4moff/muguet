#!/usr/bin/env python3
"""Build a personal unsigned IPA from your own game.wasm; output stays in .build/."""
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import time
import zipfile
import argparse
ROOT = Path(__file__).resolve().parents[1]
def check(reserve_bytes=0):
    free = shutil.disk_usage(ROOT).free
    return {'passed': free - reserve_bytes >= 15 * 1024**3, 'free_bytes': free}
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('game_wasm', type=Path, help='Your own original game.wasm (never uploaded)')
parser.add_argument('--xcodegen', default=shutil.which('xcodegen'), help='Path to XcodeGen')
args = parser.parse_args()
if not args.game_wasm.is_file() or args.game_wasm.suffix != '.wasm':
    parser.error('Provide an existing original .wasm file')
if not args.xcodegen:
    parser.error('Install XcodeGen or pass --xcodegen')
GAME_WASM = args.game_wasm.resolve()

BUILD = ROOT / '.build'
STATE = BUILD / 'logs/fork-r2-ipa-build-state.json'
TOOLCHAIN = '1.98.1'
GAME_CODE = BUILD / 'work/game.cwasm'
TARGET = BUILD / 'target-ios'
PROJECT = BUILD / 'work/ios-project'
DERIVED = BUILD / 'xcode-derived-data'
STAGE = BUILD / 'work/ipa-stage'
IPA = BUILD / 'artifacts/fork-r2/Muguet-0.1.2-fork-r2-ios-arm64-unsigned.ipa'
ENV = os.environ.copy()
ENV.update({
    'RUSTUP_HOME': str(BUILD / 'rustup-home'),
    'CARGO_HOME': str(BUILD / 'cargo-home'),
    'CARGO_BUILD_JOBS': '2', 'RAYON_NUM_THREADS': '2',
    'TMPDIR': str(BUILD / 'work'),
    'CLANG_MODULE_CACHE_PATH': str(BUILD / 'sdk-cache/clang'),
    'SWIFT_MODULE_CACHE_PATH': str(BUILD / 'sdk-cache/swift'),
    'RUSTFLAGS': f'--remap-path-prefix={Path.home()}=/home --remap-path-prefix={ROOT}=/muguet',
    'MUGUET_IOS_HEAP_GB': '5',
    'IPHONEOS_DEPLOYMENT_TARGET': '15.0',
})

def state(phase, **fields):
    temporary = STATE.with_suffix('.tmp')
    temporary.write_text(json.dumps({'updated_at_unix': time.time(), 'phase': phase, **fields}, indent=2) + '\n')
    temporary.replace(STATE)

def run(phase, command, reserve_mib=1024, env=None):
    if not check(reserve_mib * 1024**2)['passed']:
        raise RuntimeError(f'Disk guard stopped {phase}.')
    log_path = BUILD / f'logs/r2-{phase}.log'
    print(f'Starting {phase}.', flush=True)
    with log_path.open('wb') as log:
        process = subprocess.Popen(command, cwd=ROOT, env=env or ENV,
                                   stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        state(phase, process_pid=process.pid, log_path=str(log_path))
        while process.poll() is None:
            if not check()['passed']:
                os.killpg(process.pid, signal.SIGTERM)
                process.wait()
                raise RuntimeError(f'Free disk space fell below 15 GiB during {phase}.')
            time.sleep(5)
        if process.returncode:
            tail = '\n'.join(log_path.read_text(errors='replace').splitlines()[-25:])
            raise RuntimeError(f'{phase} failed ({process.returncode}):\n{tail}')
    print(f'Completed {phase}.', flush=True)

def output(command):
    return subprocess.check_output(command, cwd=ROOT, env=ENV, text=True, stderr=subprocess.STDOUT)

try:
    for folder in [BUILD / 'work', BUILD / 'logs', IPA.parent]:
        folder.mkdir(parents=True, exist_ok=True)
    if IPA.exists():
        raise RuntimeError('An IPA already exists; inspect it before rebuilding.')
    host = BUILD / 'target-host/release/muguet'
    host_env = ENV.copy()
    host_env.pop('RUSTFLAGS', None)
    run('fork-build-host', ['cargo', f'+{TOOLCHAIN}', 'build', '--locked', '--release', '--target-dir', str(BUILD / 'target-host'), '--jobs', '2'], 4096, host_env)
    run('precompile-ios-game', [str(host), '--precompile-ios', str(GAME_WASM), str(GAME_CODE)], 4096)
    ios_env = ENV.copy()
    ios_env['MUGUET_GAME_CODE'] = str(GAME_CODE)
    run('build-ios-rust', ['cargo', f'+{TOOLCHAIN}', 'build', '--locked', '--release',
                          '--no-default-features', '--target', 'aarch64-apple-ios',
                          '--target-dir', str(TARGET), '--jobs', '2'], 8192, ios_env)
    binary = TARGET / 'aarch64-apple-ios/release/muguet'
    PROJECT.mkdir(parents=True, exist_ok=True)
    specification = (ROOT / 'ios/project.yml').read_text()
    specification = specification.replace('- path: stub.c', '- path: ' + json.dumps(str(ROOT / 'ios/stub.c')))
    specification = specification.replace('- path: ../assets/muguet.icon', '- path: ' + json.dumps(str(ROOT / 'assets/muguet.icon')))
    specification = specification.replace('INFOPLIST_FILE: Info.plist', 'INFOPLIST_FILE: ' + json.dumps(str(ROOT / 'ios/Info.plist')))
    specification = specification.replace('CODE_SIGN_ENTITLEMENTS: Muguet.entitlements', 'CODE_SIGN_ENTITLEMENTS: ' + json.dumps(str(ROOT / 'ios/Muguet.entitlements')))
    specification = specification.replace('"$SRCROOT/../target/ios/aarch64-apple-ios/release/muguet"', json.dumps(str(binary)))
    spec_path = PROJECT / 'project.yml'
    spec_path.write_text(specification)
    run('generate-ios-project', [str(Path(args.xcodegen).resolve()), 'generate',
                               '--spec', str(spec_path), '--project', str(PROJECT)], 64)
    run('package-ios-app', ['xcodebuild', '-quiet', '-project', str(PROJECT / 'Muguet.xcodeproj'),
                           '-scheme', 'Muguet', '-configuration', 'Release', '-jobs', '2',
                           '-destination', 'generic/platform=iOS', '-derivedDataPath', str(DERIVED),
                           'build', 'PRODUCT_BUNDLE_IDENTIFIER=ch.cclerc.muguet.ios',
                           'CODE_SIGNING_ALLOWED=NO',
                           f'CLANG_MODULE_CACHE_PATH={BUILD / "sdk-cache/clang"}',
                           f'SWIFT_MODULE_CACHE_PATH={BUILD / "sdk-cache/swift"}'], 2048)
    built_app = DERIVED / 'Build/Products/Release-iphoneos/Muguet.app'
    payload = STAGE / 'Payload'
    payload.mkdir(parents=True, exist_ok=True)
    app = payload / 'Muguet.app'
    if app.exists():
        raise RuntimeError('The IPA staging app already exists; do not overwrite it blindly.')
    run('fork-stage-ios-app', ['/bin/cp', '-cRp', str(built_app), str(app)], 1024)
    run('remove-ios-signature', ['codesign', '--remove-signature', str(app / 'muguet')], 64)
    executable = app / 'muguet'
    architectures = output(['lipo', '-archs', str(executable)]).strip()
    if architectures != 'arm64':
        raise RuntimeError(f'Unexpected IPA architecture: {architectures}')
    commands = output(['otool', '-l', str(executable)])
    if 'segname __GAME' not in commands:
        raise RuntimeError('The iOS executable does not contain embedded game code.')
    (BUILD / 'logs/fork-r2-ios-executable-load-commands.txt').write_text(commands)
    signature = subprocess.run(['codesign', '--display', str(app)], text=True, capture_output=True)
    (BUILD / 'logs/fork-r2-ios-signature-status.txt').write_text(signature.stdout + signature.stderr)
    if signature.returncode == 0 or 'LC_CODE_SIGNATURE' in commands:
        raise RuntimeError('The unsigned executable still contains a signature.')
    signature_folder = app / '_CodeSignature'
    if signature_folder.exists():
        for entry in signature_folder.iterdir():
            if entry.name != 'CodeResources' or not entry.is_file():
                raise RuntimeError('Unexpected generated signature content.')
            entry.unlink()
        signature_folder.rmdir()
    shutil.copyfile(ROOT / 'ios/Muguet-sideload.entitlements', IPA.parent / 'Muguet-sideload.entitlements.plist')
    info = plistlib.loads((app / 'Info.plist').read_bytes())
    if info.get('CFBundleExecutable') != 'muguet':
        raise RuntimeError('The app bundle references an incorrect executable.')
    state('creating-ipa')
    partial = IPA.with_suffix('.ipa.partial')
    if partial.exists():
        raise RuntimeError('A partial IPA already exists; inspect it before overwriting it.')
    files = [p for p in STAGE.rglob('*') if p.is_file()]
    with zipfile.ZipFile(partial, 'x', compression=zipfile.ZIP_DEFLATED, compresslevel=1, allowZip64=True) as archive:
        for path in files:
            archive.write(path, str(path.relative_to(STAGE)))
    with zipfile.ZipFile(partial) as archive:
        bad = archive.testzip()
        if bad:
            raise RuntimeError(f'The IPA failed ZIP integrity verification: {bad}')
        expected = 'Payload/Muguet.app/muguet'
        if hashlib.sha256(archive.read(expected)).hexdigest() != hashlib.sha256(executable.read_bytes()).hexdigest():
            raise RuntimeError('The packaged executable differs from the unsigned staged executable.')
    digest = hashlib.sha256()
    with partial.open('rb') as inp:
        while chunk := inp.read(8 * 1024**2):
            digest.update(chunk)
    partial.replace(IPA)
    report = {'path': str(IPA), 'bytes': IPA.stat().st_size, 'sha256': digest.hexdigest(),
              'architecture': architectures, 'bundle_identifier': info['CFBundleIdentifier'],
              'version': info['CFBundleShortVersionString'], 'minimum_ios': info.get('MinimumOSVersion'),
              'rust_version': output(['cargo', f'+{TOOLCHAIN}', '--version']).strip(),
              'precompiled_game_bytes': GAME_CODE.stat().st_size,
              'installed_application_logical_bytes': sum(p.stat().st_size for p in app.rglob('*') if p.is_file()),
              'signature_verification': 'Unsigned: no embedded Mach-O signature, resource seal or provisioning profile.',
              'game_code_embedded': True, 'build_jobs': 2, 'ios_heap_ceiling_gib': 5,
              'sideload_signing_required': True, 'actual_device_tested': False, 'gameplay_tested': False}
    (IPA.parent / 'SHA256SUMS.txt').write_text(digest.hexdigest() + '  ' + IPA.name + '\n')
    (BUILD / 'logs/fork-r2-ipa-verification.json').write_text(json.dumps(report, indent=2) + '\n')
    state('complete', artifact=report)
    print(json.dumps(report), flush=True)
except Exception as error:
    state('failed', error=str(error))
    raise
