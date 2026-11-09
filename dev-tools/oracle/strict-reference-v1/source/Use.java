// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package strict;
// Astral query prefix: 🪴 中文.
public class Use {
 void run(Target t, Derived d, Child c, Parent p, Integer i) {
  t./*Q:strict_string*/literal("hello");
  t./*Q:strict_child*/pick(c);
  t./*Q:strict_identity*/pick(p);
  t./*Q:strict_specific*/specific(c);
  t./*Q:strict_reverse*/reverse(c);
  t./*Q:strict_mixed*/mixed(c,c);
  t./*Q:strict_wrapper*/wrapper(i);
  t./*Q:strict_loose*/loose(i);
  t./*Q:strict_array*/array(c);
  t./*Q:strict_unknown*/unknown(null);
  t./*Q:strict_static*/stat(c);
  t./*Q:strict_private*/priv(c);
  t./*Q:strict_generic*/generic(c,c);
  t./*Q:strict_varargs*/varargs(c);
  d./*Q:strict_inherited*/inherited(c);
 }
}
