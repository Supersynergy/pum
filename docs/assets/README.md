# Assets

- `social-preview.png` — 1280x640 deterministic SVG card, generated 2026-07-02 via
  `repo-release-excellence` skill's `scripts/gen-hero.sh` (no AI image gen, no external
  service — pure SVG→PNG render). Regenerate with:

  ```bash
  bash scripts/gen-hero.sh --name "pum" \
    --tagline "Package Update Manager — for humans and AI agents" \
    --motif '$ pum doctor && pum update --all' \
    --accent "#F59E0B" --accent2 "#38BDF8" \
    --out docs/assets/social-preview.png
  ```

  Upload manually to GitHub repo Settings → Social preview (no API for this).
