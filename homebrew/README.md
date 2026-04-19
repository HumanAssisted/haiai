# Homebrew tap

This directory holds the seed `haiai.rb` formula. The canonical formula lives in a
separate tap repo at `github.com/HumanAssisted/homebrew-haiai`.

After the tap repo exists, the `bump-homebrew` job in
`.github/workflows/publish-rust.yml` regenerates `Formula/haiai.rb` on every
`rust/v*` release and pushes it to the tap repo. `haiai.rb` here is only a
reference copy of the current template and the platform sha256s for the latest
release — do not edit it by hand after bootstrap.

## One-time bootstrap

1. Create an empty GitHub repo: `HumanAssisted/homebrew-haiai` (public).
2. Push the seed formula:

   ```bash
   git clone https://github.com/HumanAssisted/homebrew-haiai.git
   cd homebrew-haiai
   mkdir -p Formula
   cp ../haisdk/homebrew/haiai.rb Formula/haiai.rb
   git add Formula/haiai.rb
   git commit -m "Add haiai formula v$(grep '^  version' Formula/haiai.rb | sed 's/.*"\(.*\)".*/\1/')"
   git push origin main
   ```

3. Create a fine-grained GitHub PAT with `contents: write` on
   `HumanAssisted/homebrew-haiai`. Store as the secret `HOMEBREW_TAP_TOKEN`
   on `HumanAssisted/haiai` (or wherever this workflow runs).
4. Verify:

   ```bash
   brew tap HumanAssisted/homebrew-haiai
   brew install haiai
   haiai --version
   ```

## How updates happen

On every `rust/v*` tag push:

1. `release` job uploads release tarballs + `sha256sums.txt` to GitHub Releases.
2. `bump-homebrew` job downloads `sha256sums.txt`, parses the 4 platform sha256s,
   regenerates `Formula/haiai.rb` from the inline template, and pushes directly
   to `homebrew-haiai:main`.

No manual formula edits required after bootstrap.
