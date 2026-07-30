# 1.0 release checklist

- [ ] Freeze runtime and orchestration features.
- [ ] Run the full automated Windows and Ubuntu checks from clean clones.
- [ ] Build signed Windows and verified Ubuntu packages from the release tag.
- [ ] Complete every required case in `acceptance-1.0.md`.
- [ ] Triage all failures and publish remaining non-blocking limitations.
- [ ] Confirm no critical/high release blocker remains.
- [ ] Confirm every supported row in `support-matrix.md` has evidence.
- [ ] Review diagnostic bundles and logs for prohibited sensitive data.
- [ ] Verify schema migration, corrupt-state fallback, backup restore, and
      offline startup from the previous stable version.
- [ ] Verify retain-data and explicit-remove-data uninstall paths.
- [ ] Complete keyboard, screen-reader, scaling, contrast, and non-expert
      comprehension runs.
- [ ] Verify docs links and package checksums.
- [ ] Tag the exact tested commit and archive the evidence reports.
