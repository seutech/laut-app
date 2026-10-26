#!/usr/bin/env python3
"""Build the pinned CPU-only worker for the current host, including Windows without CUDA.
Requires CMake and a C++ toolchain only on the build host. End users receive the binary.
"""
import argparse, hashlib, os, pathlib, platform, shutil, subprocess, tarfile, urllib.request

VERSION = '1.9.5'
SHA256 = 'ff1a9053feb509ff9d7729703355541ae9690073a6b1c40eb692c962e0dc1720'
URL = f'https://github.com/ggml-org/whisper.cpp/archive/refs/tags/v{VERSION}.tar.gz'
ROOT = pathlib.Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--archive', type=pathlib.Path)
    parser.add_argument('--out', type=pathlib.Path, default=ROOT / 'target' / 'worker-bundle')
    args = parser.parse_args()
    cache = ROOT / 'target' / 'whisper-source'
    cache.mkdir(parents=True, exist_ok=True)
    archive = args.archive or cache / f'whisper-{VERSION}.tar.gz'
    if not archive.exists():
        temporary = archive.with_suffix('.partial')
        urllib.request.urlretrieve(URL, temporary)
        if hashlib.sha256(temporary.read_bytes()).hexdigest() != SHA256:
            raise SystemExit('Whisper source checksum mismatch; package not used')
        temporary.replace(archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
        raise SystemExit('Whisper source checksum mismatch')
    source = cache / f'whisper.cpp-{VERSION}'
    if not source.exists():
        with tarfile.open(archive) as tar:
            tar.extractall(cache, filter='data')
    build = cache / ('build-' + platform.system() + '-' + platform.machine())
    command = ['cmake', '-S', str(source), '-B', str(build),
               '-DCMAKE_BUILD_TYPE=Release', '-DBUILD_SHARED_LIBS=OFF',
               '-DWHISPER_BUILD_TESTS=OFF', '-DWHISPER_BUILD_EXAMPLES=ON',
               '-DWHISPER_BUILD_SERVER=OFF', '-DGGML_NATIVE=OFF',
               '-DGGML_CUDA=OFF', '-DGGML_METAL=OFF', '-DGGML_BLAS=OFF',
               '-DGGML_OPENMP=OFF', '-DWHISPER_COREML=OFF',
               '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded']
    remap_source = os.environ.get('LAUT_REMAP_SOURCE', str(ROOT.parent))
    if platform.system() == 'Windows':
        command += ['-A', 'x64', '-DGGML_AVX=OFF', '-DGGML_AVX2=OFF',
                    '-DGGML_FMA=OFF', '-DGGML_F16C=OFF', '-DGGML_BMI2=OFF',
                    f'-DCMAKE_C_FLAGS=/pathmap:{remap_source}=/src/laut',
                    f'-DCMAKE_CXX_FLAGS=/pathmap:{remap_source}=/src/laut']
    else:
        command += [f'-DCMAKE_C_FLAGS=-ffile-prefix-map={remap_source}=/src/laut',
                    f'-DCMAKE_CXX_FLAGS=-ffile-prefix-map={remap_source}=/src/laut']
    subprocess.run(command, check=True)
    subprocess.run(['cmake', '--build', str(build), '--config', 'Release',
                    '--target', 'whisper-cli', '--parallel', '4'], check=True)
    name = 'whisper-cli.exe' if os.name == 'nt' else 'whisper-cli'
    candidates = list((build / 'bin').rglob(name))
    if len(candidates) != 1:
        raise SystemExit(f'Ambiguous worker output: {candidates}')
    args.out.mkdir(parents=True, exist_ok=True)
    shutil.copy2(candidates[0], args.out / name)
    shutil.copy2(source / 'LICENSE', args.out / 'WHISPER-LICENSE.txt')
    (args.out / 'WHISPER-SOURCE.txt').write_text(
        f'whisper.cpp {VERSION}\nMIT license; original license included.\nSource: {URL}\n'
        f'Source SHA256: {SHA256}\nCPU only. No CUDA, Metal or external Python runtime.\n', encoding='utf-8')
    subprocess.run([str((args.out / name).resolve()), '--help'], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    print(args.out.resolve())


if __name__ == '__main__':
    main()
