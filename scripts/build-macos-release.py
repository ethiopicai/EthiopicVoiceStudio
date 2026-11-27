#!/usr/bin/env python3
"""Build a local macOS candidate, or verify a signed/notarized public release before packaging."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    'aarch64-apple-darwin': {'arch': 'aarch64', 'cpu': 'arm64', 'label': 'Apple Silicon'},
    'x86_64-apple-darwin': {'arch': 'x86_64', 'cpu': 'x86_64', 'label': 'Intel'},
}
ACCEPTANCE_CHECKS = (
    'redistribution_clearance', 'downloaded_runtime_signing', 'clean_machine_setup', 'interrupted_download_resume',
    'offline_transcription_dictation', 'real_microphone', 'corrections_playback_export',
    'history_original_preserved', 'low_memory_baseline', 'owner_ux',
)


def check_acceptance(path, version, arch='aarch64'):
    if path is None:
        raise ValueError('Public release needs --acceptance-report with recorded macOS release checks; use --candidate for local review.')
    report = json.loads(path.read_text())
    if report.get('schema_version') != 1 or report.get('app_version') != version or report.get('platform') != 'macos' or report.get('arch') != arch:
        raise ValueError('Acceptance report does not match this macOS app version.')
    for name in ACCEPTANCE_CHECKS:
        check = report.get('checks', {}).get(name, {})
        if check.get('passed') is not True or not str(check.get('evidence', '')).strip():
            raise ValueError(f'Public release check still pending: {name}')
    return report


def run(*args, **kwargs):
    kwargs.setdefault('cwd', ROOT)
    return subprocess.run(args, check=True, **kwargs)


def selected_target(target=None):
    if platform.system() != 'Darwin' or platform.machine() not in ('arm64', 'x86_64'):
        raise ValueError('Build macOS packages on an Apple Silicon or Intel Mac.')
    target = target or ('aarch64-apple-darwin' if platform.machine() == 'arm64' else 'x86_64-apple-darwin')
    if target not in TARGETS:
        raise ValueError('Unsupported macOS build target.')
    return target, TARGETS[target]


def preflight(candidate, report=None, target=None):
    target, details = selected_target(target)
    if candidate:
        return target, details
    catalog = json.loads((ROOT / 'apps/desktop/resources/asr-catalog.json').read_text())
    mac_releases = [release for release in catalog.get('releases', [])
                    if release.get('os') == 'macos' and release.get('arch') == details['arch']]
    if not mac_releases:
        raise ValueError('Public release blocked: publish and pin the approved model/runtime catalogue first.')
    config = json.loads((ROOT / 'apps/desktop/src-tauri/tauri.conf.json').read_text())
    if any(release.get('minimum_macos') != config['bundle']['macOS']['minimumSystemVersion'] for release in mac_releases):
        raise ValueError('The app and resource catalogue must declare the same minimum macOS version.')
    if platform.machine() != details['cpu']:
        raise ValueError('Public releases require a native build and acceptance checks for the target architecture.')
    check_acceptance(report, config['version'], details['arch'])
    run('cargo', 'run', '--locked', '-p', 'resource-packs', '--example', 'validate_catalog', '--',
        str(ROOT / 'apps/desktop/resources/asr-catalog.json'))
    identity = os.environ.get('APPLE_SIGNING_IDENTITY', '')
    if not identity.startswith('Developer ID Application:'):
        raise ValueError('Public release needs APPLE_SIGNING_IDENTITY for a Developer ID Application certificate.')
    identities = run('security', 'find-identity', '-v', '-p', 'codesigning', capture_output=True, text=True).stdout
    if identity not in identities:
        raise ValueError('The configured signing identity is not installed in this Mac keychain.')
    if not os.environ.get('ETHIOPIC_NOTARY_PROFILE'):
        raise ValueError('Set ETHIOPIC_NOTARY_PROFILE to a notarytool credentials profile stored securely in this Mac keychain.')
    return target, details


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--candidate', action='store_true', help='Local review only; never represents a signed public release')
    parser.add_argument('--acceptance-report', type=Path, help='Recorded results of the required release acceptance checks')
    parser.add_argument('--target', choices=TARGETS, help='Defaults to this Mac architecture; Intel currently supports interface review only')
    args = parser.parse_args()
    target, details = preflight(args.candidate, args.acceptance_report, args.target)
    config = json.loads((ROOT / 'apps/desktop/src-tauri/tauri.conf.json').read_text())
    version, name = config['version'], config['productName']
    spec = importlib.util.spec_from_file_location('build_privacy', ROOT / 'scripts/build-privacy.py')
    privacy = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(privacy)
    env = privacy.build_environment(ROOT)
    if args.candidate:
        for key in list(env):
            if key.startswith('APPLE_'):
                del env[key]
        env['APPLE_SIGNING_IDENTITY'] = '-'
    minimum = config['bundle']['macOS']['minimumSystemVersion']
    env['MACOSX_DEPLOYMENT_TARGET'] = minimum
    run('npm', 'exec', '--', 'tauri', 'build', '--bundles', 'app', '--target', target,
        env=env, cwd=ROOT / 'apps/desktop')
    bundle = ROOT / 'target' / target / 'release/bundle/macos' / f'{name}.app'
    binary = bundle / 'Contents/MacOS/ethiopic-voice-studio'
    privacy.reject_build_paths(binary, ROOT)
    architectures = run('lipo', '-archs', str(binary), capture_output=True, text=True).stdout.split()
    if architectures != [details['cpu']]:
        raise ValueError('Packaged executable does not match the requested Mac architecture.')
    with (bundle / 'Contents/Info.plist').open('rb') as stream:
        plist = plistlib.load(stream)
    if plist.get('CFBundleShortVersionString') != version.split('-', 1)[0] or not str(plist.get('CFBundleVersion', '')).isdigit():
        raise ValueError('Apple bundle version metadata must be numeric and match the release version.')
    if plist.get('LSMinimumSystemVersion') != config['bundle']['macOS']['minimumSystemVersion']:
        raise ValueError('Packaged minimum macOS version differs from the release configuration.')
    # Tauri signs the complete bundle, including entitlements and hardened-runtime flags.
    # Re-signing here would discard that metadata from a local candidate.
    run('codesign', '--verify', '--deep', '--strict', str(bundle))
    output = ROOT / '.release' / f'macos-{version}{"-intel" if details["arch"] == "x86_64" else ""}{"-candidate" if args.candidate else ""}'
    output.mkdir(parents=True, exist_ok=True)
    if not args.candidate:
        upload = output / 'notarization-app.zip'
        run('ditto', '-c', '-k', '--keepParent', str(bundle), str(upload))
        run('xcrun', 'notarytool', 'submit', str(upload), '--wait', '--keychain-profile', os.environ['ETHIOPIC_NOTARY_PROFILE'])
        run('xcrun', 'stapler', 'staple', str(bundle))
        run('xcrun', 'stapler', 'validate', str(bundle))
        run('spctl', '--assess', '--type', 'execute', '--verbose=2', str(bundle))
    staging = output / 'disk'
    if staging.exists():
        shutil.rmtree(staging)
    staging.mkdir()
    run('ditto', str(bundle), str(staging / bundle.name))
    (staging / 'Applications').symlink_to('/Applications')
    (staging / 'Read Me.txt').write_text(
        f'EthiopicAI Voice Studio {version}\n\nDrag the app into Applications, then open it.\n'
        f'{details["label"]} Mac, macOS {minimum} or later.\n'
        + ('Interface preview only. Voice-tool setup is not available for Intel Macs in this release.\n' if details['arch'] == 'x86_64' else
           'Open Settings to download voice tools. After setup, recognition works offline.\nMemory needs and speed depend on the selected voice tools.\n')
        + ('\nLOCAL REVIEW CANDIDATE. Not notarized or approved for public distribution.\n' if args.candidate else ''))
    image = output / f'EthiopicAI-Voice-Studio-{version}-macos-{"arm64" if details["arch"] == "aarch64" else "x64"}{"-candidate" if args.candidate else ""}.dmg'
    if image.exists():
        image.unlink()
    run('hdiutil', 'create', '-volname', 'EthiopicAI Voice Studio', '-srcfolder', str(staging), '-ov', '-format', 'UDZO', str(image))
    if not args.candidate:
        run('codesign', '--force', '--timestamp', '--sign', os.environ['APPLE_SIGNING_IDENTITY'], str(image))
        run('xcrun', 'notarytool', 'submit', str(image), '--wait', '--keychain-profile', os.environ['ETHIOPIC_NOTARY_PROFILE'])
        run('xcrun', 'stapler', 'staple', str(image))
        run('xcrun', 'stapler', 'validate', str(image))
        run('spctl', '--assess', '--type', 'open', '--context', 'context:primary-signature', '--verbose=2', str(image))
    with image.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    (output / (image.name + '.sha256')).write_text(f'{digest}  {image.name}\n')
    (output / 'build-status.json').write_text(json.dumps({
        'version': version, 'platform': 'macos', 'arch': details['arch'], 'target': target, 'public_release': not args.candidate,
        'support_stage': 'interface-preview' if details['arch'] == 'x86_64' else 'preview',
        'installer': image.name, 'sha256': digest,
        'acceptance_report_sha256': hashlib.sha256(args.acceptance_report.read_bytes()).hexdigest() if args.acceptance_report and not args.candidate else None,
        'resource_catalog_configured': bool(json.loads((ROOT / 'apps/desktop/resources/asr-catalog.json').read_text())['releases'])}, indent=2) + '\n')
    shutil.rmtree(staging)
    print(f'{"Local review candidate" if args.candidate else "Verified release"}: {image}')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error))
