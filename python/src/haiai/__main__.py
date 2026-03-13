"""Entry point for `python -m haiai` -- delegates to the Rust CLI binary."""

from haiai._binary import main

if __name__ == "__main__":
    main()
