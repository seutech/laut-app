#!/usr/bin/env python3
"""Build the pinned CPU-only worker for the current host, including Windows without CUDA.
Requires CMake and a C++ toolchain only on the build host. End users receive the binary.
"""
import argparse, hashlib, os, pathlib, platform, shutil, subprocess, tarfile, urllib.request

VERSION = '0.7.0'
SHA256 = '55d67acf3ad2ad747a9bbeb75f9f0e173dbcc0d1753a471038a90c819421c5e1'
URL = f'https://github.com/ggml-org/llama.cpp/archive/refs/tags/v{VERSION}.tar.gz'
ROOT = pathlib.Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--archive', type=pathlib.Path)
    parser.add_argument('--out', type=pathlib.Path, default=ROOT / 'target' / 'worker-bundle')
    args = parser.parse_args()
    cache = ROOT / 'target' / 'llama-source'
    cache.mkdir(parents=True, exist_ok=True)
    archive = args.archive or cache / f'llama-{VERSION}.tar.gz'
    if not archive.exists():
        temporary = archive.with_suffix('.partial')
        urllib.request.urlretrieve(URL, temporary)
        if hashlib.sha256(temporary.read_bytes()).hexdigest() != SHA256:
            raise SystemExit('Llama source checksum mismatch; package not used')
        temporary.replace(archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
        raise SystemExit('Llama source checksum mismatch')
    source = cache / f'llama.cpp-{VERSION}'
    required_source_files = (source / 'CMakeLists.txt', source / 'LICENSE')
    if not all(path.is_file() for path in required_source_files):
        if source.is_dir() and not source.is_symlink():
            shutil.rmtree(source)
        elif source.exists() or source.is_symlink():
            source.unlink()
        with tarfile.open(archive) as tar:
            tar.extractall(cache, filter='data')
    if not all(path.is_file() for path in required_source_files):
        raise SystemExit('Llama source archive did not produce a complete source tree')
    build = cache / ('build-' + platform.system() + '-' + platform.machine())
    command = ['cmake', '-S', str(source), '-B', str(build),
               '-DCMAKE_BUILD_TYPE=Release', '-DBUILD_SHARED_LIBS=OFF',
               '-DLLAMA_BUILD_TESTS=OFF', '-DLLAMA_BUILD_EXAMPLES=ON', '-DLLAMA_BUILD_TOOLS=ON',
               '-DLLAMA_BUILD_SERVER=OFF', '-DLLAMA_BUILD_APP=OFF', '-DLLAMA_OPENSSL=OFF', '-DGGML_NATIVE=OFF',
               '-DGGML_CUDA=OFF', '-DGGML_METAL=OFF', '-DGGML_BLAS=OFF',
               '-DGGML_OPENMP=OFF',
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
                    '--target', 'llama-completion', 'llama-embedding', '--parallel', '4'], check=True)
    args.out.mkdir(parents=True, exist_ok=True)
    for tool in ('llama-completion', 'llama-embedding'):
        name = tool + ('.exe' if os.name == 'nt' else '')
        candidates = list((build / 'bin').rglob(name))
        if len(candidates) != 1:
            raise SystemExit(f'Ambiguous worker output: {candidates}')
        shutil.copy2(candidates[0], args.out / name)
        subprocess.run([str((args.out / name).resolve()), '--help'], check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    shutil.copy2(source / 'LICENSE', args.out / 'LLAMA-LICENSE.txt')
    (args.out / 'LLAMA-SOURCE.txt').write_text(
        f'llama.cpp {VERSION}\nMIT license; original license included.\nSource: {URL}\n'
        f'Source SHA256: {SHA256}\nCPU only. No CUDA, Metal, server or external Python runtime.\n', encoding='utf-8')
    print(args.out.resolve())


if __name__ == '__main__':
    main()
