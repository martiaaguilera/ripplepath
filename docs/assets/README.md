# Screenshots

Real captures of `ripplepath serve`, not mockups. Regenerate with:

```bash
cargo build -p ripplepath-cli
ripplepath demo --dir /tmp/rp-demo
# ingest fixtures/java-banking/evidence/v1 at main~1 and v2 at main into /tmp/rp-demo.db:
#   ripplepath ingest junit <run-N.xml> --repo /tmp/rp-demo --rev <rev> --db /tmp/rp-demo.db
#   ripplepath ingest coverage <Class.xml> --format jacoco --test <Class> --repo /tmp/rp-demo --rev <rev> --db /tmp/rp-demo.db
(cd web && npm ci && npm run build)
ripplepath serve --repo /tmp/rp-demo --db /tmp/rp-demo.db --base main~1 --head main --web-dir web/dist
cd web && RIPPLEPATH_SCREENSHOTS=../docs/assets npx playwright test screenshots
```

Viewport 1600×1000, light theme.
