# Unsaved-buffer preview acceptance plan

This plan fixes the vertical acceptance before the new CLI path is complete.
It is an adapter foundation, not an LSP server or Java conformance claim.

## Existing sample, two simultaneous unsaved buffers

Reuse the explicit `examples/cli_project/project.json` graph:
`app/test -> app/main -> lib/main`. Keep all disk files unchanged.

1. Supply a full-text library buffer renaming its `value` field to `editedValue`,
   and an application buffer using `editedValue`. Include CRLF and a supplementary
   character in a preceding comment. Query the application using strict zero-based
   UTF-16 line/character coordinates. Verify the exact library-buffer declaration
   bytes, source owner and caller revision/file-version metadata.
2. Omit the library overlay with the application buffer still open. The edited
   reference must not resolve to the disk field. Omit both overlays and verify the
   original disk query again. These are separate whole-state invocations, not an
   implemented persistent open/change/close session.
3. Query the same source from a dependent test scope and preserve the reverse
   main/test isolation. Wrong owner, duplicate canonical file, unknown file and
   unsupported additions must fail the entire request, without writing sources.
4. Feed malformed unsaved text to parser-report and verify its source ranges refer
   to that buffer while the disk bytes stay unchanged. Keep the classification as
   parser limitation or invalid syntax, never Java semantic diagnostics.
5. Keep every legacy offset command/output unchanged without overlays. Validate
   UTF-16 surrogate interiors, CRLF interiors, line/column overflow, blank and
   trailing lines, EOF and mixed offset/line flags. No silent clamping.
6. Test document/count caps and per-source, compilation and workspace byte budgets.
   An invalid second overlay must preserve the base snapshot and old prepared
   handle. After loading a test binary, remove it and update sources through the
   library API: the derived snapshot must not reread the binary.
7. Exercise the native supervisor with overlays under an empty environment and
   no Java search path, including cancellation/error-envelope behavior.

Revision and file versions are caller-provided correlation labels. A caller must
compare its current **whole editor/workspace generation** before accepting a
response; the CLI does not enforce monotonic versions across processes. Same-file
versions alone cannot protect a target modified in a different buffer.

## Fixed real project follow-up

Use the already pinned Gson 2.10.1 input set without upstream build execution.
Select the existing `cross_interface` query from extras/main into core/main's
`TypeAdapterFactory` declaration. Prefix the query and declaration files with a
known comment containing an astral character and CRLF in overlays only. Query
with line/character coordinates and check the independently shifted original
byte/absolute-UTF-16 declaration range. Keep the entire source inventory and all
original parser/refusal baselines; this is a position/overlay check, not a new
claim of Java semantics. Verify all disk and pinned input hashes stay unchanged.

## Mac build gate is separate

Native macOS CI may build and run the analyzer on declared hosted runner
architectures. It does not authorize accessing the user's Mac or certify a signed
Mac distribution, all OS versions or an editor plugin. Report actual CI results;
a workflow definition or a Linux build is not evidence that macOS passed.
