# Driver tools

Repository tools for the [Driver Hub contract](../Drivers/RELEASING.md#driver-hub-compatibility). They need Python 3.11+ and `jsonschema` (`apt install python3-jsonschema` or `pip install jsonschema`). Run them from the repository root.

| Command | Checks |
| --- | --- |
| `python3 tools/validate-driver.py config <dir>/driver-config.json` | The declaration against [its schema](../Drivers/schemas/driver-config-v1.schema.json), plus the rules JSON Schema cannot express: unique names and bindings, defaults within their bounds, `nodeIdInput` and `videoIPathInputs` references, and the `VIDEOIPATH_*` roles. |
| `python3 tools/validate-driver.py release <dir> [--tag drivers/<id>/v<version>]` | `driver-release.json` against [its schema](../Drivers/schemas/driver-release-v1.schema.json) and the tag, every asset's SHA-256, the declaration, and the OCI archive: layout, size limits, root digest, every blob's size and digest, image platforms. |
| `python3 tools/validate-driver.py fixtures` | The [schema fixtures](../Drivers/schemas/tests/): at least one valid and one invalid case per rule. |
| `python3 -m unittest discover -s tools/tests` | The release and OCI checks. |

Errors are printed as `<file>#<JSON Pointer>: <problem>`. They name fields, never input values.
