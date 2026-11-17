# Explicit static field name navigation

A direct value reference can navigate through an explicit named import such as
`import static pkg.Owner.VALUE;`. This is declaration navigation, not constant
calculation, field-type inference or validation that the Java program compiles.

## Bounded selection

Existing locals, pattern guards and lexical fields take precedence. Only after
those paths leave the name unresolved does the new path inspect its matching
explicit imports. Relevant enclosing owners and import owners must have complete,
unambiguous ancestry using the existing bounded proof. Missing parents cannot
be treated as proof that no lexical field shadows the import.

The import owner must be a unique accessible top-level non-interface declaration.
Only its own uniquely declared field is selected. Staticness and import-site
access must be known: public members, or supported package/protected members in
the same package. A use-site subclass does not grant protected import access.
Source fields reuse existing supported modifier facts; unsupported annotations
or malformed field declarations remain unproved. Binary fields require supported
ordinary flags; synthetic and enum-constant flags are not admitted by this slice.

Repeated imports of the same declaration are deduplicated by identity. Different
fields, missing owners, duplicate declarations, nonstatic fields and unproved
access cannot be filtered out to manufacture a unique target. An own field may
legally hide an ancestor field. An import naming only an inherited field remains
unsupported, including the Java-legal alias combination `A.VALUE` and `B.VALUE`
where B inherits the same declaration from A.

Failure of a relevant import proof returns `StaticImportNotApplied`, not a Java
error diagnosis, and cannot escape to a same-named type. No matching explicit
import preserves the old resolver path. Wildcard-only files retain their old
behavior; mixed wildcard imports are conservatively refused when this explicit
field path is attempted.

## Namespace and expression boundaries

Only direct value references opt in. Type names, calls, method references and
qualified/indexed receivers retain their existing paths, including parenthesized
receivers. Ordinary argument values and index operands may use an imported field.
No imported-field receiver type, invocation actual type or return type is inferred.
Field initializers work without an enclosing method or DirectCaller.

## Verification scope

Native tests cover own fields, initialization, shadowing, repeated and conflicting
imports, missing/inherited/nonstatic members, access and binary flags, namespace
boundaries and snapshot/edit isolation. Optional javac controls distinguish valid
but deliberately unsupported imports from invalid programs.

Four unchanged Gson source references provide an original-project check:
JsonWriter's EMPTY_DOCUMENT and DANGLING_NAME, and GsonBuilder's
DEFAULT_SERIALIZE_NULLS and DEFAULT_PRETTY_PRINT. Exact source declaration names
and ranges are compared with independent compiler-selected declarations. These
four checks are bounded evidence, not a project-wide coverage percentage.
