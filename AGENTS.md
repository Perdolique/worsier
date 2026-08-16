# Agents instructions

## Verification

- Use `vp run <script>` for repository package scripts. Run the full check with `vp run check`.
- Treat every target in the CI and release matrices as supported. For changes that can affect OS-specific code, paths, filesystems, line endings, processes, or builds, inspect every relevant `cfg` and platform-specific code path, add or update platform-aware coverage, and verify the affected matrix when available; do not infer cross-platform correctness from the local OS alone.
- Keep `README.md` fresh and synched with all applied changes.
- Do not hard-wrap Markdown prose; this project has no line-length limit.
