"""Root conftest: ensure local src/jacs/hai overrides the installed jacs package.

The haiai SDK extends the jacs.hai namespace with additional modules (models,
config, client, etc.). When jacs is installed from PyPI, Python resolves
jacs.hai submodules from site-packages first. This conftest prepends the
local src/jacs path so the SDK's extended versions take precedence.
"""

import sys
from pathlib import Path

_src_jacs = str(Path(__file__).resolve().parent / "src" / "jacs")

# Ensure jacs.__path__ resolves our local modules first
import jacs  # noqa: E402

if _src_jacs not in jacs.__path__ or jacs.__path__[0] != _src_jacs:
    try:
        jacs.__path__.remove(_src_jacs)
    except ValueError:
        pass
    jacs.__path__.insert(0, _src_jacs)

# Also clear any cached jacs.hai submodules so they re-resolve
for mod_name in list(sys.modules):
    if mod_name.startswith("jacs.hai"):
        del sys.modules[mod_name]
