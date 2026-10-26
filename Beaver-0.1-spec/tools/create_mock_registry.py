#!/usr/bin/env python3
"""Create deterministic static Beaver v1 mock registry fixtures.

No external dependencies. This is an input for future Beaver integration tests.
It deliberately creates a few *malicious* archives, which must never be extracted
by tooling except through a security validation test harness.
"""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile

BASE = Path(__file__).resolve().parent.parent
DEFAULT_OUT = BASE / 'fixtures' / 'registry'


def manifest(name: str, version: str, dependencies: dict[str, str]) -> bytes:
    lines = [
        '[package]',
        f'name = "{name}"',
        f'version = "{version}"',
        'edition = "2026"',
        'kind = "lib"',
        'entry = "src/lib.bv"',
        '',
        '[dependencies]',
    ]
    for dependency, dep_version in sorted(dependencies.items()):
        lines.append(f'{dependency} = {{ version = "{dep_version}" }}')
    lines.append('')
    return '\n'.join(lines).encode('utf-8')


def tar_archive(name: str, version: str, files: dict[str, bytes],
                dangerous: str | None = None) -> bytes:
    root = f'{name}-{version}'
    memory = io.BytesIO()
    with tarfile.open(fileobj=memory, mode='w', format=tarfile.USTAR_FORMAT) as tar:
        for path, data in sorted(files.items()):
            ti = tarfile.TarInfo(name=f'{root}/{path}')
            ti.size = len(data)
            ti.mtime = 0
            ti.uid = ti.gid = 0
            ti.uname = ti.gname = ''
            ti.mode = 0o644
            tar.addfile(ti, io.BytesIO(data))
        if dangerous == 'traversal':
            ti = tarfile.TarInfo(name=f'{root}/../../outside.txt')
            payload = b'DO NOT WRITE OUTSIDE TARGET DIRECTORY\n'
            ti.size = len(payload)
            ti.mtime = 0
            ti.uid = ti.gid = 0
            tar.addfile(ti, io.BytesIO(payload))
        elif dangerous == 'symlink':
            ti = tarfile.TarInfo(name=f'{root}/src/link.bv')
            ti.type = tarfile.SYMTYPE
            ti.linkname = '/tmp/evil.bv'
            ti.mtime = 0
            ti.uid = ti.gid = 0
            tar.addfile(ti)
    compressed = io.BytesIO()
    with gzip.GzipFile(fileobj=compressed, mode='wb', filename='', mtime=0) as z:
        z.write(memory.getvalue())
    return compressed.getvalue()


def create_package(root: Path, name: str, version: str,
                   dependencies: dict[str, str], *,
                   source: bytes | None = None,
                   dangerous: str | None = None,
                   incorrect_hash: bool = False,
                   invalid_gzip: bool = False) -> dict[str, str | int | dict[str, str]]:
    files = {
        'beaver.toml': manifest(name, version, dependencies),
        'src/lib.bv': source if source is not None else b'pub fn identity(value: Int) -> Int:\n    return value\n',
    }
    archive = (b'this is not a valid tar.gz' if invalid_gzip else
               tar_archive(name, version, files, dangerous))
    checksum = hashlib.sha256(archive).hexdigest()
    if incorrect_hash:
        checksum = '0' * 64
    meta = {
        'schema': 1,
        'name': name,
        'version': version,
        'sha256': checksum,
        'archive_size': len(archive),
        'dependencies': dependencies,
    }
    dest = root / 'v1' / 'packages' / name / version
    dest.mkdir(parents=True, exist_ok=True)
    (dest / 'metadata.json').write_text(json.dumps(meta, indent=2, sort_keys=True) + '\n')
    (dest / 'archive.tar.gz').write_bytes(archive)
    return meta


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=DEFAULT_OUT)
    args = parser.parse_args()
    out = args.out.resolve()
    source = (BASE / 'examples' / 'mathkit' / 'src' / 'lib.bv').read_bytes()

    create_package(out, 'mathkit', '1.0.0', {}, source=source)
    create_package(out, 'mathkit', '2.0.0', {}, source=source)
    create_package(out, 'badchecksum', '1.0.0', {}, incorrect_hash=True)
    create_package(out, 'pathtrap', '1.0.0', {}, dangerous='traversal')
    create_package(out, 'linktrap', '1.0.0', {}, dangerous='symlink')
    create_package(out, 'badgzip', '1.0.0', {}, invalid_gzip=True)
    create_package(out, 'cycle_a', '1.0.0', {'cycle_b': '1.0.0'})
    create_package(out, 'cycle_b', '1.0.0', {'cycle_a': '1.0.0'})
    create_package(out, 'requires_a', '1.0.0', {'mathkit': '1.0.0'})
    create_package(out, 'requires_b', '1.0.0', {'mathkit': '2.0.0'})
    create_package(out, 'conflict_root', '1.0.0', {
        'requires_a': '1.0.0', 'requires_b': '1.0.0',
    })

    print('Mock registry written to:', out)
    print('Start server: python3 -m http.server 8765 --bind 127.0.0.1 --directory', out)
    print('Set BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY=1 and')
    print('BEAVER_REGISTRY_URL=http://127.0.0.1:8765 for local testing only.')
    print('Includes malicious fixture packages pathtrap/linktrap; DO NOT extract them manually.')


if __name__ == '__main__':
    main()
