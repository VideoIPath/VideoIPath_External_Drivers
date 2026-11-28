"""Build the four DriverHub assets from HEAD, without tagging or publishing anything.

The declaration is validated (by its own schemaVersion) before the image is built, and
the finished assets are verified as Driver Hub would before the output directory appears.
Needs Python 3.11+, jsonschema, git and docker buildx.
"""

import argparse
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib

from distribution import add_bytes, add_distribution

SOURCE = "examples/rust_example"
ROOT = Path(__file__).resolve().parents[3]
PLATFORM = "linux/amd64"

sys.path.insert(0, str(ROOT / "tools"))
import driverhub_contract as contract  # noqa: E402


def enforce(label, check, *args):
    """Run a contract check; on failure name every problem and stop without output."""
    try:
        return check(*args)
    except contract.ContractError as error:
        for at, message in error.errors:
            print(f"{label}#{at}: {message}", file=sys.stderr)
        sys.exit(f"not packaged: {label} does not meet the Driver Hub contract")


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)


def image_digest(path):
    """Check the single-platform output, including every blob that will be shipped."""
    with tarfile.open(path, "r:") as archive:
        def read_json(name):
            return json.load(archive.extractfile(name))

        def check_blob(descriptor):
            algorithm, digest = descriptor["digest"].split(":")
            if algorithm != "sha256":
                raise ValueError("expected SHA-256 OCI blob")
            name = f"blobs/sha256/{digest}"
            entry = archive.getmember(name)
            if not entry.isfile() or entry.size != descriptor["size"]:
                raise ValueError(f"invalid OCI blob: {name}")
            with archive.extractfile(entry) as stream:
                if hashlib.file_digest(stream, "sha256").hexdigest() != digest:
                    raise ValueError(f"OCI blob checksum mismatch: {name}")
            return name

        root, = read_json("index.json")["manifests"]
        if root["mediaType"] != "application/vnd.oci.image.manifest.v1+json":
            raise ValueError("expected a single OCI image manifest")
        manifest = read_json(check_blob(root))
        config = read_json(check_blob(manifest["config"]))
        if f"{config['os']}/{config['architecture']}" != PLATFORM:
            raise ValueError(f"expected {PLATFORM}")
        for layer in manifest["layers"]:
            check_blob(layer)
        return root["digest"]


def build(output):
    commit = git("rev-parse", "HEAD").decode().strip()
    driver = f"{commit}:{SOURCE}"
    declaration = git("show", f"{driver}/driver-config.json")
    version = tomllib.loads(git("show", f"{driver}/Cargo.toml").decode())["package"]["version"]
    if output.exists():
        raise FileExistsError(f"choose a new output directory: {output}")
    # Fail before the (slow) image build. These exact bytes are shipped and hashed below.
    enforce("driver-config.json", contract.check_config, json.loads(declaration))
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=output.parent) as temp:
        scratch = Path(temp)
        assets = scratch / "assets"
        assets.mkdir()
        # Read from Git, never the worktree: local .env files, certificates and
        # edits cannot enter a package claiming to come from this revision.
        context = git("archive", driver, ".dockerignore", "Dockerfile", "src", "Cargo.toml", "Cargo.lock")
        subprocess.run(
            ["docker", "buildx", "build", "--platform", PLATFORM, "--provenance=false",
             "--sbom=false", "--output", "type=oci,dest=image.tar", "-"],
            cwd=scratch, input=context, check=True,
        )
        image = scratch / "image.tar"
        digest = image_digest(image)
        archive = assets / f"rust-example-{version}.oci.tar.gz"
        with image.open("rb") as src, gzip.open(archive, "wb") as dest:
            shutil.copyfileobj(src, dest)

        (assets / "driver-config.json").write_bytes(declaration)
        bundle = assets / "deployment.tar.gz"
        # The offline README and source-only build instructions have different audiences.
        files = {
            "README.md": f"{driver}/deployment/DRIVERHUB.md",
            "RELEASE-NOTES.md": f"{driver}/CHANGELOG.md",
            "LICENSE": f"{commit}:LICENSE",
        }
        for name in ("compose.yaml", ".env.example", "driver-config.json", "THIRD_PARTY.md"):
            files[name] = f"{driver}/{name}"
        with tarfile.open(bundle, "w:gz") as deployment:
            for name, revision in files.items():
                add_bytes(deployment, name, git("show", revision))
            add_distribution(deployment, commit)

        def checksum(path):
            with path.open("rb") as stream:
                return hashlib.file_digest(stream, "sha256").hexdigest()

        record = {
            "schemaVersion": 1, "driverId": "rust-example", "displayName": "Rust API teaching example",
            "version": version, "source": {"commit": commit, "directory": SOURCE},
            "configuration": {"file": "driver-config.json", "sha256": hashlib.sha256(declaration).hexdigest()},
            "image": {"digest": digest, "platforms": [PLATFORM], "archive": archive.name,
                      "archiveSha256": checksum(archive)},
            "deploymentBundle": {"file": bundle.name, "sha256": checksum(bundle)},
        }
        (assets / "driver-release.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
        # Every checksum, the declaration and the OCI layout, as Driver Hub checks them.
        enforce("release assets", contract.verify_release_directory, assets)
        assets.rename(output)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new output directory")
    build(parser.parse_args().output.resolve())
