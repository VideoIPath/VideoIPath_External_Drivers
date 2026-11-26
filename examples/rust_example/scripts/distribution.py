"""Collect source and upstream notices for the exact Rust DriverHub package."""

from concurrent.futures import ThreadPoolExecutor
import hashlib
import io
from pathlib import Path
import re
import subprocess
import tarfile
import tomllib
from urllib.request import urlopen


def add_bytes(bundle, name, data):
    member = tarfile.TarInfo(name)
    member.size = len(data)
    bundle.addfile(member, io.BytesIO(data))


def download(url, checksum):
    with urlopen(url, timeout=60) as response:
        data = response.read()
    if hashlib.sha256(data).hexdigest() != checksum:
        raise ValueError(f"checksum mismatch: {url}")
    return data


def crate_archive(package):
    if package["source"] != "registry+https://github.com/rust-lang/crates.io-index":
        raise ValueError(f"unhandled dependency source for {package['name']}")
    name = package["name"]
    archive = f"{name}-{package['version']}.crate"
    data = download(f"https://static.crates.io/crates/{name}/{archive}", package["checksum"])
    return f"sources/crates/{archive}", data


def add_distribution(bundle, commit):
    root = Path(__file__).resolve().parents[3]
    tree = f"{commit}:examples/rust_example"

    def git(*args):
        return subprocess.check_output(["git", *args], cwd=root)

    add_bytes(bundle, "sources/rust-example.tar.gz", git("archive", "--format=tar.gz", tree))
    lock = tomllib.loads(git("show", f"{tree}/Cargo.lock").decode())
    with ThreadPoolExecutor(max_workers=6) as pool:
        for name, data in pool.map(crate_archive, (p for p in lock["package"] if "source" in p)):
            add_bytes(bundle, name, data)

    dockerfile = git("show", f"{tree}/Dockerfile").decode()
    builder = re.search(r"^FROM (\S+@sha256:[0-9a-f]{64}) AS builder$", dockerfile, re.MULTILINE)
    if not builder:
        raise ValueError("expected a digest-pinned Rust builder")
    notices = subprocess.check_output([
        "docker", "run", "--rm", "--platform", "linux/amd64", "--network", "none",
        builder.group(1), "sh", "-ec",
        'cd "$(rustc --print sysroot)/share/doc/rust"; rustc -Vv > RUST-VERSION; '
        'tar -cf - COPYRIGHT-library.html licenses RUST-VERSION',
    ])
    add_bytes(bundle, "notices/rust-toolchain.tar", notices)
    for version, checksum in (
        ("1.2.5", "f9bc4423732350eb0b3f7ed7e91d530298476f8fec0c6c427a1c04ade22655af"),
        ("1.2.6", "b870108ec5e7790e9f9919064f1b9421d62d5f9b0e6c230c6adf7ea2da62e97b"),
    ):
        url = f"https://raw.githubusercontent.com/ifduyue/musl/v{version}/COPYRIGHT"
        add_bytes(bundle, f"notices/musl-{version}-COPYRIGHT", download(url, checksum))
    add_bytes(bundle, "notices/distroless-LICENSE", download(
        "https://raw.githubusercontent.com/GoogleContainerTools/distroless/main/LICENSE",
        "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30",
    ))
