// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package literal;
// Astral query prefix: 🪴 中文. String is a source shadow, never the literal type.
public class Use {
 java.lang.String field;
 java.lang.String result() { return null; }
 void run(Target receiver, Ref ref) {
  receiver./*Q:string_plain*/exact("hello");
  receiver./*Q:string_empty*/exact("");
  receiver./*Q:string_unicode*/exact("🪴中文");
  receiver./*Q:string_escapes*/exact("\b\t\n\f\r\"\\\'\s");
  receiver./*Q:string_octal*/exact("\0\7\07\377\400");
  receiver./*Q:string_pair*/pair("a", "b");
  receiver./*Q:string_arity_one*/arity("a");
  receiver./*Q:string_arity_two*/arity("a", "b");
  receiver./*Q:string_mixed*/mixed("a", ref);
  receiver./*Q:string_object*/object("a");
  receiver./*Q:string_choose*/choose("a");
  receiver./*Q:string_other*/other("a");
  receiver./*Q:string_null*/exact(null);
  receiver./*Q:string_cast*/exact((java.lang.String) "a");
  receiver./*Q:string_concat*/exact("a" + "b");
  receiver./*Q:string_field*/exact(field);
  receiver./*Q:string_result*/exact(result());
  receiver./*Q:string_unicode_escape*/exact("\u0061");
  receiver./*Q:string_textblock*/exact("""
text
""");
 }
}
