"""Public `haiai.crypt` compatibility layer.

All crypto functions now live in `jacs.hai.signing`.
This shim re-exports them for backward compatibility.
"""

from jacs.hai.signing import (  # noqa: F401
    canonicalize_json,
    create_agent_document,
    verify_string,
)
