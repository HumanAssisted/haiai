"""Public `haiai.crypt` compatibility layer.

All crypto functions now live in `haiai.signing`.
This shim re-exports them for backward compatibility.
"""

from haiai.signing import (  # noqa: F401
    canonicalize_json,
    create_agent_document,
    verify_string,
)
