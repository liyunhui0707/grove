# Moment Grove website

Static, Chinese product website. No server, external font, analytics, account or photo upload.

Build: node build.mjs. Serve dist/ with a static HTTP server.

From the repository root, `npm run test:website` builds locally and checks six viewport widths using the installed Chrome. It blocks remote downloads and covers layout, rating controls, keyboard FAQ/reset, exact approved download links and disclosure. The tablet artwork overflow fix is included in the 0.5.0 website update; current publication evidence is tracked in the product release receipt. This is not Safari or physical-phone acceptance.

The comparison image is an actual browser UI screenshot using generated demo artwork. It is labelled as such and is not a native macOS screenshot. Other landscapes are original CSS illustrations. Personal QA media must never be copied here.

Download links target the exact approved GitHub prerelease. Keep version, filenames and support limits aligned with that release before deploying. The Sites project identity is retained in .openai/hosting.json.
