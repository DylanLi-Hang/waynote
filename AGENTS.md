# Waynote development and release rules

Read `docs/development.md` before changing runtime behavior or releasing.

- Integrate and test changes on `develop` first. Do not promote feature work directly to `main`.
- Automated/headless tests are necessary but do not replace real-desktop acceptance of the production note UI.
- Before promotion, verify the actual running executable, saved geometry, smaller/larger monitors, input regions, scaling, editing and dragging on the user's desktop.
- Merge `develop` into `main` only after the user explicitly confirms that develop works and authorizes promotion. Do not update the stable consumer pin to an unaccepted build.
- Keep test notes/state separate from installed notes. No OS build, switch or activation without explicit authorization.
