---
id: TASK-40
title: curl handler does not check -sLo and -O output paths
status: Done
assignee:
  - '@dippy_dev'
created_date: '2026-10-24 14:06'
updated_date: '2026-10-24 14:33'
labels: []
dependencies: []
priority: high
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
rust/dippy-rs/src/cli/curl.rs reads only a standalone -o FILE, -oFILE or --output[=]FILE as a write target, so bundled short options (curl -sLo /tmp/x.deb URL) and every other write form are allowed without a redirect-rule check: -O/--remote-name and --remote-name-all (file named after the URL, in cwd or --output-dir), -D/--dump-header, -c/--cookie-jar, --trace, --trace-ascii, --stderr, --libcurl, --etag-save, --hsts, --alt-svc. The same token-by-token scan misses bundled data, method, config and quote options (curl -sd x URL, -sXPOST, -sK file) that ask when written alone.

Fix: parse curl arguments like curl 8.x does (short option bundles; the short options taking an argument use the rest of the word or the next word; -- ends options; --url adds a URL) and report every written file as a redirect target. Unknown names that curl takes from the server or from URL globs ask.

In scope: curl handler and its tests, docs/CHANGELOG.
Out of scope: wget (separate finding), ~/.curlrc contents, the live ~/wiki/.dippy workaround (revisit with the user after install).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Bundled and attached -o forms (-sLo FILE, -sLoFILE, -oFILE, -o=FILE) report the file as a redirect target, as -o FILE and --output FILE do
- [x] #2 -O/--remote-name and --remote-name-all report the file name curl derives from each URL (last path segment without query; curl_response when empty), so curl -O https://x/.bashrc is checked as .bashrc
- [x] #3 --output-dir DIR prefixes relative -o and -O targets
- [x] #4 curl asks for -J/--remote-header-name, for -O with a URL glob ([..] or {..}), for an -o name containing #<digit>, and for -w/--write-out containing %output{
- [x] #5 Bundled data, method, config and quote options (-sd x, -sF a=b, -sXPOST, -sK file, -sQ cmd) are classified like their standalone forms
- [x] #6 dippy --cmd with a deny-redirect rule denies curl -sLo ~/.bashrc URL and curl -O https://x/.bashrc run in HOME
- [x] #7 -D/--dump-header, -c/--cookie-jar, --trace, --trace-ascii, --stderr, --libcurl, --etag-save, --hsts and --alt-svc report their file as a redirect target; '-' (stdout) and /dev/null are not targets
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Bump 0.6.10 + CHANGELOG. 2. Failing Rust tests in cli/curl.rs. 3. One parser yielding options (name, value) and URLs; classify over it; collect write targets. 4. Verify with dippy --cmd (empty HOME, temporary config). 5. just check, just test, commit.
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
curl.rs now parses arguments like curl 8.x (short option bundles with SHORT_WITH_ARG, -- ends options, --url, --name=value) and classifies over the parsed options: -o/--output (bundled, attached, -o=), -O/--remote-name/--remote-name-all (remote_file_name, curl_response when empty), --output-dir prefix, and the WRITE_FILE_FLAGS files are redirect targets; -J, -O with a URL glob, -o with #N and -w %output{ ask; bundled -d/-F/-X/-K/-Q are classified like standalone forms. Tests: 5 new tests in cli/curl.rs. Verified: cargo test (empty HOME), clippy, rustfmt, just check, and the debug binary with deny-redirect **/.bashrc: curl -sLo ~/.bashrc URL, curl -O https://x/.bashrc and curl -sc ~/.bashrc URL deny; -sLo /tmp/x.deb allow under allow-redirect /tmp/**.
<!-- SECTION:FINAL_SUMMARY:END -->
