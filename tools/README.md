# Driver tools

Repository tools for the [Driver Hub contract](../Drivers/RELEASING.md#driver-hub-compatibility). They need Python 3.11+ and `jsonschema` (`apt install python3-jsonschema` or `pip install jsonschema`). Run them from the repository root.

| Command | Checks |
| --- | --- |
| `python3 tools/validate-driver.py directory <dir>...` (or `--all`) | Whether Driver Hub could build and deploy the driver: the directory name, a `Dockerfile` at its root and a valid `driver-config.json`. Notes a missing `controlHealth`. `--report` prints GitHub annotations and a job summary and always exits 0; the Drivers workflow uses it, so the result is informational. |
| `python3 tools/validate-driver.py config <dir>/driver-config.json` | The declaration against the schema its `schemaVersion` names ([1](../Drivers/schemas/driver-config-v1.schema.json), or [2](../Drivers/schemas/driver-config-v2.schema.json)), plus the rules JSON Schema cannot express: unique names and bindings, defaults within their bounds, `nodeIdInput` and `videoIPathInputs` references, and the `VIDEOIPATH_*` roles; for version 2 also `enum` choices within the length bounds and defaults that satisfy `enum` and `url`. Any other version is refused. |
| `python3 tools/validate-driver.py release <dir> [--tag drivers/<id>/v<version>]` | `driver-release.json` against [its schema](../Drivers/schemas/driver-release-v1.schema.json) and the tag, every asset's SHA-256, the declaration, and the OCI archive: layout, size limits, root digest, every blob's size and digest, image platforms. |
| `python3 tools/validate-driver.py fixtures` | The [schema fixtures](../Drivers/schemas/tests/): at least one valid and one invalid case per rule, and the value cases (input, value, expected reason codes) to share with Driver Hub. |
| `python3 -m unittest discover -s tools/tests` | The release and OCI checks, and each driver's `tests/declared-values.json` against its declaration ([RELEASING.md](../Drivers/RELEASING.md#prove-the-declaration-against-the-parser)). |

Errors are printed as `<file>#<JSON Pointer>: <problem>`. They name fields, never input values.

`driverhub_contract.value_errors(input, value)` is the reference for the value rules: it returns the reason codes Driver Hub would report for one effective value.
