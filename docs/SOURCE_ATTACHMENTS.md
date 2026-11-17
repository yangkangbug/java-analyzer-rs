# Explicit source attachments (editor preview)

Optional manifest field (existing JAR/JMOD -> already expanded local directory):

    "source_attachments": [{
      "binary_input": "/opt/jdk/jmods/java.base.jmod",
      "source_root": "/home/me/jdk-sources/java.base",
      "association": "explicit_unverified"
    }]

No download, extraction, dependency discovery or decompilation is performed.

This is a user-declared display association, NOT proof of the same build or source
version. Matching package/name and supported declaration shape only permits
displaying that declared attachment. Use sources appropriate to your binaries.
Private editor results record source_association=explicit_unverified; LSP logs
the limitation on successful attachment navigation.

## Deliberately narrow display matching

Only exact unique top-level ordinary class/interface declarations in package/Name.java
are supported. Every loaded compilation signature at the selected binary origin
must agree. That origin must belong to exactly one configured archive association;
duplicate associations are configuration errors. JAR multi-release provenance is
checked but does not prove the attachment matches the selected release.

Nested/local/anonymous types, all names containing dollar signs (including legal
literal names), enum/record/annotation source declarations, alternate filenames and fields
are unsupported. Binary kind checks use only existing interface-bit metadata;
complete source/binary declaration-kind equivalence is not verified.
A method/field never falls back to its class. Any parser error
causes refusal, even with a usable-looking header. In tested Temurin 21 sources,
Collection/List parse without errors; String has existing gaps and is refused.

Methods additionally require an existing exact binary method identity, unique
full descriptor, and agreeing loaded origin metadata. Constructors, bridge or
synthetic methods and methods with a generic Signature attribute are refused.
Only primitive parameters/returns, primitive arrays/varargs and scalar void
returns are supported. Generic owners are permitted when the method descriptor
does not depend on their type parameters. No reference-type resolution or erasure
is attempted.

The unique top-level owner is inspected only for direct Method children. Every
same-name method header must be supported; reference/generic parameters or returns,
throws clauses, unsupported modifiers, malformed headers and duplicate parameter
descriptors refuse the whole family. Only access, static, final, native and abstract
method modifiers are supported, with duplicate/conflicting forms refused; parameters
allow annotations and a single final. Full descriptors, static and varargs flags
must match, and the returned range is the exact method name. Abstract/native
declarations need no body. Old-style return dimensions after the parameter list
remain unsupported by the parser. This is not full Java semantic validation.

Actual local Temurin 21 LSP checks cover Character.isWhitespace(char/int),
Collection.size and Iterator.hasNext, attachment off/on and same-session source
refresh. String.length stays unavailable because String.java has parser diagnostics.
These display successes do not certify source/binary version equivalence.

No attachment preserves prior behavior and output. Unsupported/unavailable display
returns binary_source_unavailable, preserving binding status. Invalid attachment
configuration fails configuration validation. Ordinary declaration CLI results
remain binary targets, not attachment locations.

## Freshness and display boundaries

Each query rereads bounded source bytes and uses the same captured text for parsing
and UTF-16 ranges. Frozen configuration carries the association; manifest edits
require restart. Attachments are NOT source roots, inventory, compilation owners,
overlays or binder inputs. Opening one does not enable navigation from it.
Unsaved attachment buffer modifications are not analyzed.

The service only reads attachments; it does NOT enforce editor read-only mode.
A returned file URI may still be editable. Keep copyright/license notices.
Sources remain user-supplied local data; this feature bundles no JDK sources.

## Limits and local path checks

64 attachments maximum, 4096-byte paths, 1 MiB per source file. Only the predicted
file is read, without recursive search. Non-regular files, symlinks, parent
traversal, path escape, ambiguous origins, unknown association values and invalid
UTF-8 are refused. Canonical/component checks prevent common misconfiguration,
not a race-free hostile-filesystem sandbox. Existing timeout, generation,
cancellation and delivery guards are unchanged.
