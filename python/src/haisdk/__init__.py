"""HAI SDK public Python namespace.

Use `import haisdk` as the canonical import path.
The legacy `jacs.hai` namespace remains supported for backward compatibility.
"""

from jacs.hai import *  # noqa: F401,F403
from jacs.hai import __all__ as _LEGACY_ALL
from jacs.hai import __version__
from . import integrations

__all__ = list(_LEGACY_ALL) + ["integrations"]
