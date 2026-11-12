// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package qualified;
// Supplementary-coordinate prefix: 🪴 中文.
public class Use {
 int value() { return 1; }
 void run(Target t, Derived d, int i, byte b, char c, boolean yes) {
  t./*Q:primitive_literal*/pick(1);
  t./*Q:primitive_int*/pick(i);
  t./*Q:primitive_byte*/pick(b);
  t./*Q:primitive_reverse*/reverse(1);
  t./*Q:primitive_widen*/widen(b);
  t./*Q:primitive_char*/chars(c);
  t./*Q:primitive_boolean*/bool(yes);
  t./*Q:primitive_unknown*/pick(value());
  t./*Q:primitive_long_suffix*/pick(1L);
  t./*Q:primitive_boxed*/boxed(1);
  t./*Q:primitive_varargs*/varargs(1);
  t./*Q:primitive_generic*/generic(1);
  d./*Q:primitive_inherited*/inherited(1);
  t./*Q:primitive_mixed*/mixed(1, null);
 }
}
