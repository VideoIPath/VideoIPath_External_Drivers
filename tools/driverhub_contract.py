"""Driver Hub contract checks shared by validate-driver.py and package-driver.py.

The JSON Schemas in Drivers/schemas hold every structural rule. This module adds the
rules JSON Schema cannot express, and verifies release assets and OCI archives.
Drivers/RELEASING.md#driver-hub-compatibility describes the contract in prose.
"""

import gzip
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SCHEMAS = ROOT / "Drivers" / "schemas"

SEMVER = (
    r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    r"(?:-(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)
TAG = re.compile(rf"^drivers/([a-z0-9]+(?:-[a-z0-9]+)*)/v({SEMVER})$")
RESERVED_VERSION = "0.0.0-local."
# Driver Hub's source-build directory rule (section 5.8 of the work order).
BUILD_DIRECTORY = re.compile(r"^(Drivers|examples)(/[A-Za-z0-9_-][A-Za-z0-9_.-]*)+$")

# Driver Hub's archive limits.
MAX_DOWNLOAD = 2 * 1024**3
MAX_EXPANDED = 8 * 1024**3
MAX_ENTRIES = 32768

OCI_MANIFEST = "application/vnd.oci.image.manifest.v1+json"
OCI_INDEX = "application/vnd.oci.image.index.v1+json"

# Variables Driver Hub supplies itself. URL and token are filled from the site unless
# the device is connected manually; the node ID is always set.
NODE_ID_ENV = "VIDEOIPATH_NODE_ID"
ROLE_ENV = {"url": "VIDEOIPATH_URL", "token": "VIDEOIPATH_TOKEN"}


class ContractError(Exception):
    """One or more contract violations, each as (JSON Pointer, message)."""

    def __init__(self, errors):
        self.errors = errors
        super().__init__("\n".join(f"{at}: {message}" for at, message in errors))


def _validator(name):
    try:
        from jsonschema import Draft202012Validator
    except ImportError as error:  # pragma: no cover - depends on the host
        raise SystemExit("install jsonschema (python3-jsonschema or pip install jsonschema)") from error
    schema = json.loads((SCHEMAS / f"{name}-v1.schema.json").read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema)


def pointer(*parts):
    return "/" + "/".join(str(part) for part in parts) if parts else "/"


def _location(error, schema, document):
    """Where an error applies. A forbidden property ("minimum": false) is reported at the
    property by older jsonschema releases and at its parent object by newer ones; report
    it at the property either way."""
    path = list(error.absolute_path)
    schema_path = list(error.absolute_schema_path)
    if schema_path and schema_path[-1] == "properties":
        properties, parent = schema, document
        for part in schema_path:
            if part == "$ref":
                continue
            # Error paths may step through a local "$ref" without naming it.
            while isinstance(properties, dict) and part not in properties and "$ref" in properties:
                target = schema
                for name in properties["$ref"].removeprefix("#/").split("/"):
                    target = target[name]
                properties = target
            properties = properties[part]
        for part in path:
            parent = parent[part]
        if isinstance(parent, dict):
            forbidden = [key for key, rule in properties.items()
                         if rule is False and key in parent and parent[key] == error.instance]
            if forbidden:
                path.append(forbidden[0])
    return pointer(*path)


def schema_errors(name, document):
    validator = _validator(name)
    errors = [(_location(error, validator.schema, document), error.message)
              for error in validator.iter_errors(document)]
    return sorted(errors)


def _is_int(value):
    return isinstance(value, int) and not isinstance(value, bool)


def config_errors(document):
    """All violations of driver-config.json, structural first."""
    errors = schema_errors("driver-config", document)
    if errors:
        # Cross-field rules assume the structure is valid; report structure first.
        return errors
    inputs = document["inputs"]
    by_name = {}
    bindings = {}
    for index, item in enumerate(inputs):
        at = ("inputs", index)
        name = item["name"]
        if name in by_name:
            errors.append((pointer(*at, "name"), f"duplicate input name {name!r}"))
        by_name.setdefault(name, item)
        kind = "env" if "env" in item else "file"
        binding = (kind, item[kind])
        if binding in bindings:
            errors.append((pointer(*at, kind), f"{kind} binding {item[kind]!r} is already used by input {bindings[binding]!r}"))
        bindings.setdefault(binding, name)

        if "minimum" in item and "maximum" in item and item["minimum"] > item["maximum"]:
            errors.append((pointer(*at), "minimum must not exceed maximum"))
        if "minLength" in item and "maxLength" in item and item["minLength"] > item["maxLength"]:
            errors.append((pointer(*at), "minLength must not exceed maxLength"))
        if "default" in item:
            default = item["default"]
            if item["type"] == "integer" and _is_int(default):
                if "minimum" in item and default < item["minimum"]:
                    errors.append((pointer(*at, "default"), "default is below minimum"))
                if "maximum" in item and default > item["maximum"]:
                    errors.append((pointer(*at, "default"), "default is above maximum"))
            if item["type"] == "string" and isinstance(default, str):
                # Python str length counts Unicode code points, as the declaration does.
                if "minLength" in item and len(default) < item["minLength"]:
                    errors.append((pointer(*at, "default"), "default is shorter than minLength"))
                if "maxLength" in item and len(default) > item["maxLength"]:
                    errors.append((pointer(*at, "default"), "default is longer than maxLength"))

    def env_of(item):
        return item.get("env")

    node_input = document.get("nodeIdInput")
    if node_input is not None:
        item = by_name.get(node_input)
        if item is None:
            errors.append(("/nodeIdInput", f"names no declared input: {node_input!r}"))
        else:
            if item["type"] != "string" or item.get("secret", False):
                errors.append(("/nodeIdInput", "must name a non-secret string input"))
            if "default" in item:
                errors.append(("/nodeIdInput", "must name an input without a default"))

    roles = document.get("videoIPathInputs", {})
    for role, canonical in ROLE_ENV.items():
        if role not in roles:
            continue
        at = f"/videoIPathInputs/{role}"
        name = roles[role]
        item = by_name.get(name)
        if item is None:
            errors.append((at, f"names no declared input: {name!r}"))
            continue
        secret = item.get("secret", False)
        if item["type"] != "string" or secret != (role == "token"):
            errors.append((at, "must name a secret string input" if role == "token" else "must name a non-secret string input"))
        if name == node_input:
            errors.append((at, "must not be the node-ID input"))
        other = ROLE_ENV["token" if role == "url" else "url"]
        if env_of(item) in (NODE_ID_ENV, other):
            errors.append((at, f"input must not be bound to {env_of(item)}"))
        for index, candidate in enumerate(inputs):
            if candidate["name"] != name and env_of(candidate) == canonical:
                errors.append((pointer("inputs", index, "env"), f"{canonical} is reserved for the input named by videoIPathInputs.{role}"))
    return errors


def check_config(document):
    errors = config_errors(document)
    if errors:
        raise ContractError(errors)


def release_errors(record, tag=None):
    """Structural and tag rules of driver-release.json (assets are checked separately)."""
    errors = schema_errors("driver-release", record)
    if tag is not None:
        match = TAG.match(tag)
        if not match:
            errors.append(("/", f"tag {tag!r} does not match drivers/<driverId>/v<SemVer>"))
        else:
            if record.get("driverId") != match.group(1):
                errors.append(("/driverId", "differs from the tag"))
            if record.get("version") != match.group(2):
                errors.append(("/version", "differs from the tag"))
    return errors


def is_prerelease(version):
    """Driver Hub treats a version with '-' before any build metadata as a prerelease."""
    return "-" in version.split("+", 1)[0]


def sha256_file(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def _safe_member(member):
    path = PurePosixPath(member.name)
    if path.is_absolute() or ".." in path.parts:
        return False
    return member.isfile() or member.isdir()


def verify_oci_archive(archive, digest, platforms):
    """Check a gzipped OCI image layout as Driver Hub does. Returns the image's platforms."""
    archive = Path(archive)
    if archive.stat().st_size > MAX_DOWNLOAD:
        raise ContractError([("/image/archive", "archive exceeds 2 GiB")])
    with tempfile.TemporaryDirectory() as scratch:
        expanded = Path(scratch) / "image.tar"
        size = 0
        with gzip.open(archive, "rb") as source, expanded.open("wb") as target:
            while chunk := source.read(1024 * 1024):
                size += len(chunk)
                if size > MAX_EXPANDED:
                    raise ContractError([("/image/archive", "archive expands beyond 8 GiB")])
                target.write(chunk)
        with tarfile.open(expanded, "r:") as layout:
            members = layout.getmembers()
            if len(members) > MAX_ENTRIES:
                raise ContractError([("/image/archive", f"archive has more than {MAX_ENTRIES} entries")])
            unsafe = [m.name for m in members if not _safe_member(m)]
            if unsafe:
                raise ContractError([("/image/archive", f"unsafe archive entry: {unsafe[0]!r}")])
            files = {PurePosixPath(m.name).as_posix().removeprefix("./"): m for m in members if m.isfile()}
            return _verify_layout(layout, files, digest, platforms)


def _verify_layout(layout, files, digest, platforms):
    def read(name):
        member = files.get(name)
        if member is None:
            raise ContractError([("/image/archive", f"missing {name}")])
        with layout.extractfile(member) as stream:
            return stream.read()

    def blob(descriptor):
        algorithm, _, value = descriptor.get("digest", "").partition(":")
        if algorithm != "sha256" or not re.fullmatch(r"[0-9a-f]{64}", value):
            raise ContractError([("/image/archive", f"unsupported blob digest {descriptor.get('digest')!r}")])
        name = f"blobs/sha256/{value}"
        member = files.get(name)
        if member is None:
            raise ContractError([("/image/archive", f"missing blob {name}")])
        if member.size != descriptor.get("size"):
            raise ContractError([("/image/archive", f"blob size differs from its descriptor: {name}")])
        with layout.extractfile(member) as stream:
            if hashlib.file_digest(stream, "sha256").hexdigest() != value:
                raise ContractError([("/image/archive", f"blob checksum mismatch: {name}")])
        return name

    def json_blob(descriptor):
        return json.loads(read(blob(descriptor)))

    def image(descriptor, expected=None):
        if descriptor.get("mediaType") != OCI_MANIFEST:
            raise ContractError([("/image/archive", f"expected an OCI image manifest, got {descriptor.get('mediaType')!r}")])
        manifest = json_blob(descriptor)
        config = json_blob(manifest["config"])
        for layer in manifest.get("layers", []):
            blob(layer)
        if not config.get("os") or not config.get("architecture"):
            raise ContractError([("/image/archive", "image config must state os and architecture")])
        platform = "/".join(filter(None, (config["os"], config["architecture"], config.get("variant"))))
        if expected is not None and platform != expected:
            raise ContractError([("/image/archive", f"image config {platform} differs from its index platform {expected}")])
        return platform

    try:
        layout_version = json.loads(read("oci-layout")).get("imageLayoutVersion")
    except json.JSONDecodeError as error:
        raise ContractError([("/image/archive", "oci-layout is not JSON")]) from error
    if layout_version != "1.0.0":
        raise ContractError([("/image/archive", "oci-layout imageLayoutVersion must be 1.0.0")])
    roots = [d for d in json.loads(read("index.json")).get("manifests", []) if d.get("digest") == digest]
    if not roots:
        raise ContractError([("/image/digest", "index.json does not reference the record's image digest")])
    root = roots[0]
    if root.get("mediaType") == OCI_INDEX:
        found = []
        for entry in json_blob(root).get("manifests", []):
            described = entry.get("platform") or {}
            if not described.get("os") or not described.get("architecture"):
                raise ContractError([("/image/archive", "every index entry needs a platform descriptor")])
            platform = "/".join(filter(None, (described["os"], described["architecture"], described.get("variant"))))
            if platform in found:
                raise ContractError([("/image/archive", f"index has more than one manifest for {platform}")])
            found.append(image(entry, platform))
    else:
        found = [image(root)]
    missing = [p for p in platforms if p not in found]
    if missing:
        raise ContractError([("/image/platforms", f"not in the image: {', '.join(missing)}")])
    return found


def verify_release_directory(directory, tag=None):
    """Validate driver-release.json and every asset it names, in one directory."""
    directory = Path(directory)
    record = json.loads((directory / "driver-release.json").read_text(encoding="utf-8"))
    errors = release_errors(record, tag)
    if errors:
        raise ContractError(errors)
    configuration = directory / record["configuration"]["file"]
    if not configuration.is_file():
        raise ContractError([("/configuration/file", "asset missing")])
    if sha256_file(configuration) != record["configuration"]["sha256"]:
        raise ContractError([("/configuration/sha256", "differs from the asset's bytes")])
    check_config(json.loads(configuration.read_text(encoding="utf-8")))
    archive = directory / record["image"]["archive"]
    if not archive.is_file():
        raise ContractError([("/image/archive", "asset missing")])
    if sha256_file(archive) != record["image"]["archiveSha256"]:
        raise ContractError([("/image/archiveSha256", "differs from the asset's bytes")])
    bundle = record.get("deploymentBundle")
    if bundle:
        if not (directory / bundle["file"]).is_file():
            raise ContractError([("/deploymentBundle/file", "asset missing")])
        if sha256_file(directory / bundle["file"]) != bundle["sha256"]:
            raise ContractError([("/deploymentBundle/sha256", "differs from the asset's bytes")])
    verify_oci_archive(archive, record["image"]["digest"], record["image"]["platforms"])
    return record


def merge_patch(target, patch):
    """RFC 7386 JSON Merge Patch, used by the schema fixtures."""
    if not isinstance(patch, dict):
        return patch
    result = dict(target) if isinstance(target, dict) else {}
    for key, value in patch.items():
        if value is None:
            result.pop(key, None)
        else:
            result[key] = merge_patch(result.get(key), value)
    return result
