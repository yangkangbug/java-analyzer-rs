// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package mixed;
// UTF-8 position prefix: 🪴 中文.
public class Use {
 int value() { return 1; }
 void run(Target t, Sub s, int i, byte b) {
  t./*Q:mixed_mixed*/mix(1, s);
  t./*Q:mixed_swapped*/swap(s, 1);
  t./*Q:mixed_multiprimitive*/multi(i, b);
  t./*Q:mixed_reverse*/reverse(i, s);
  t./*Q:mixed_three*/tri(i, s, b);
  t./*Q:mixed_widen*/mix(b, s);
  t./*Q:mixed_set*/set(0, s);
  t./*Q:mixed_unknown*/mix(value(), s);
  t./*Q:mixed_null*/set(0, null);
  t./*Q:mixed_boxed*/boxed(1, s);
  t./*Q:mixed_varargs*/varargs(s, 1);
 }
}
