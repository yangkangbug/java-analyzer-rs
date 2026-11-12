// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package nulls;
// Coordinates: 🪴 中文.
public class Use {
 Parent field;
 Parent value() { return null; }
 void run(Target t) {
  t./*Q:null_related*/related(null);
  t./*Q:null_reverse*/reverse(null);
  t./*Q:null_vector*/vector(null, null);
  t./*Q:null_primitive*/primitive(null);
  t./*Q:null_only*/only(null);
  t./*Q:null_paren*/related(((null)));
  t./*Q:null_unknown*/related(value());
  t./*Q:null_generic*/generic(null);
  t./*Q:null_array*/array(null);
  t./*Q:null_field*/only(field);
  t.inner(null)./*Q:null_chain*/next();
 }
}
