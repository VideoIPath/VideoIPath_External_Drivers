"""Unit tests for the OCI archive and release-directory checks. Run from the repository root:

    python3 -m unittest discover -s tools/tests
"""

import gzip
import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import driverhub_contract as contract  # noqa: E402

MANIFEST = contract.OCI_MANIFEST
INDEX = contract.OCI_INDEX


class Layout:
    """Build a small OCI image layout, then break it on purpose."""

    def __init__(self):
        self.blobs = {}
        self.extra = {}
        self.layout = {"imageLayoutVersion": "1.0.0"}

    def blob(self, data, media_type):
        if not isinstance(data, bytes):
            data = json.dumps(data).encode()
        digest = hashlib.sha256(data).hexdigest()
        self.blobs[digest] = data
        return {"mediaType": media_type, "digest": f"sha256:{digest}", "size": len(data)}

    def image(self, os="linux", architecture="amd64", **config):
        layer = self.blob(b"layer-" + architecture.encode(), "application/vnd.oci.image.layer.v1.tar")
        config = self.blob({"os": os, "architecture": architecture, **config}, "application/vnd.oci.image.config.v1+json")
        return self.blob({"schemaVersion": 2, "mediaType": MANIFEST, "config": config, "layers": [layer]}, MANIFEST)

    def index(self, *platforms):
        manifests = []
        for platform in platforms:
            os, architecture = platform.split("/")
            manifests.append({**self.image(os, architecture), "platform": {"os": os, "architecture": architecture}})
        return self.blob({"schemaVersion": 2, "mediaType": INDEX, "manifests": manifests}, INDEX)

    def write(self, path, root):
        with tarfile.open(path, "w:gz") as archive:
            def add(name, data):
                member = tarfile.TarInfo(name)
                member.size = len(data)
                archive.addfile(member, io.BytesIO(data))
            add("oci-layout", json.dumps(self.layout).encode())
            add("index.json", json.dumps({"schemaVersion": 2, "manifests": [root]}).encode())
            for digest, data in self.blobs.items():
                add(f"blobs/sha256/{digest}", data)
            for name, data in self.extra.items():
                add(name, data)
        return path


class OciArchive(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.path = Path(self.scratch.name) / "image.oci.tar.gz"

    def tearDown(self):
        self.scratch.cleanup()

    def verify(self, layout, root, platforms):
        layout.write(self.path, root)
        return contract.verify_oci_archive(self.path, root["digest"], platforms)

    def rejected(self, layout, root, platforms, text):
        with self.assertRaises(contract.ContractError) as caught:
            self.verify(layout, root, platforms)
        self.assertIn(text, str(caught.exception))

    def test_single_manifest(self):
        layout = Layout()
        self.assertEqual(self.verify(layout, layout.image(), ["linux/amd64"]), ["linux/amd64"])

    def test_multi_platform_index(self):
        layout = Layout()
        root = layout.index("linux/amd64", "linux/arm64")
        self.assertEqual(self.verify(layout, root, ["linux/amd64", "linux/arm64"]), ["linux/amd64", "linux/arm64"])

    def test_record_platform_must_be_in_the_image(self):
        layout = Layout()
        self.rejected(layout, layout.image(), ["linux/arm64"], "not in the image: linux/arm64")

    def test_root_must_be_the_record_digest(self):
        layout = Layout()
        root = layout.image()
        layout.write(self.path, root)
        with self.assertRaises(contract.ContractError) as caught:
            contract.verify_oci_archive(self.path, "sha256:" + "0" * 64, ["linux/amd64"])
        self.assertIn("does not reference the record's image digest", str(caught.exception))

    def test_missing_blob(self):
        layout = Layout()
        root = layout.image()
        layout.blobs.pop(next(d for d, data in layout.blobs.items() if data.startswith(b"layer-")))
        self.rejected(layout, root, ["linux/amd64"], "missing blob")

    def test_blob_size_and_checksum(self):
        layout = Layout()
        root = layout.image()
        layer = next(d for d, data in layout.blobs.items() if data.startswith(b"layer-"))
        layout.blobs[layer] = b"layer-shorter"
        self.rejected(layout, root, ["linux/amd64"], "blob size differs")
        layout.blobs[layer] = b"layer-AMD64"
        self.rejected(layout, root, ["linux/amd64"], "blob checksum mismatch")

    def test_config_states_os_and_architecture(self):
        layout = Layout()
        self.rejected(layout, layout.image(os=""), ["linux/amd64"], "must state os and architecture")

    def test_index_entries_need_a_platform_once(self):
        layout = Layout()
        entry = layout.image()
        root = layout.blob({"schemaVersion": 2, "mediaType": INDEX, "manifests": [entry]}, INDEX)
        self.rejected(layout, root, ["linux/amd64"], "needs a platform descriptor")
        twice = [{**entry, "platform": {"os": "linux", "architecture": "amd64"}}] * 2
        root = layout.blob({"schemaVersion": 2, "mediaType": INDEX, "manifests": twice}, INDEX)
        self.rejected(layout, root, ["linux/amd64"], "more than one manifest for linux/amd64")

    def test_index_platform_matches_config(self):
        layout = Layout()
        entry = {**layout.image("linux", "amd64"), "platform": {"os": "linux", "architecture": "arm64"}}
        root = layout.blob({"schemaVersion": 2, "mediaType": INDEX, "manifests": [entry]}, INDEX)
        self.rejected(layout, root, ["linux/arm64"], "differs from its index platform")

    def test_layout_version(self):
        layout = Layout()
        layout.layout = {"imageLayoutVersion": "2.0.0"}
        self.rejected(layout, layout.image(), ["linux/amd64"], "imageLayoutVersion must be 1.0.0")

    def test_unsafe_entries(self):
        layout = Layout()
        layout.extra["../escape"] = b"x"
        self.rejected(layout, layout.image(), ["linux/amd64"], "unsafe archive entry")

    def test_docker_media_types_are_not_oci(self):
        layout = Layout()
        root = {**layout.image(), "mediaType": "application/vnd.docker.distribution.manifest.v2+json"}
        self.rejected(layout, root, ["linux/amd64"], "expected an OCI image manifest")


class ReleaseDirectory(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.directory = Path(self.scratch.name)
        layout = Layout()
        root = layout.image()
        archive = layout.write(self.directory / "meter-1.0.0.oci.tar.gz", root)
        config = self.directory / "driver-config.json"
        config.write_text(json.dumps({"schemaVersion": 1, "inputs": []}))
        bundle = self.directory / "deployment.tar.gz"
        bundle.write_bytes(gzip.compress(b""))
        self.record = {
            "schemaVersion": 1, "driverId": "exampleco-meter", "displayName": "Meter", "version": "1.0.0",
            "source": {"commit": "0" * 40, "directory": "Drivers/ExampleCo/Meter"},
            "configuration": {"file": config.name, "sha256": contract.sha256_file(config)},
            "image": {"digest": root["digest"], "platforms": ["linux/amd64"], "archive": archive.name,
                      "archiveSha256": contract.sha256_file(archive)},
            "deploymentBundle": {"file": bundle.name, "sha256": contract.sha256_file(bundle)},
        }

    def tearDown(self):
        self.scratch.cleanup()

    def check(self, tag=None):
        (self.directory / "driver-release.json").write_text(json.dumps(self.record))
        return contract.verify_release_directory(self.directory, tag)

    def test_valid_release(self):
        self.assertEqual(self.check("drivers/exampleco-meter/v1.0.0")["version"], "1.0.0")

    def test_asset_hashes(self):
        for section, field in (("configuration", "sha256"), ("image", "archiveSha256"), ("deploymentBundle", "sha256")):
            with self.subTest(section=section):
                good = self.record[section][field]
                self.record[section][field] = "f" * 64
                with self.assertRaises(contract.ContractError) as caught:
                    self.check()
                self.assertIn(f"/{section}/{field}", str(caught.exception))
                self.record[section][field] = good

    def test_declaration_is_validated(self):
        config = self.directory / "driver-config.json"
        config.write_text(json.dumps({"schemaVersion": 1, "inputs": [
            {"name": "a", "type": "string", "required": True, "env": "NOMAD_X"}]}))
        self.record["configuration"]["sha256"] = contract.sha256_file(config)
        with self.assertRaises(contract.ContractError) as caught:
            self.check()
        self.assertIn("/inputs/0/env", str(caught.exception))

    def test_tag(self):
        with self.assertRaises(contract.ContractError):
            self.check("drivers/exampleco-meter/v1.0.1")


class Versions(unittest.TestCase):
    def test_prerelease(self):
        self.assertTrue(contract.is_prerelease("1.0.0-rc.1"))
        self.assertTrue(contract.is_prerelease("1.0.0-rc.1+build-7"))
        self.assertFalse(contract.is_prerelease("1.0.0+build-7"))
        self.assertFalse(contract.is_prerelease("1.0.0"))


if __name__ == "__main__":
    unittest.main()
