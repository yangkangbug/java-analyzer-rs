// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package concat;
class Product { public String text; }
class Factory { public Product make() { return null; } }
class Use {
 void run(Factory f, String text) {
  (f.make() + "x")./*Q:concat_plus*/length();
  (text += f.make())./*Q:concat_assign*/length();
  (f.make().text += "x")./*Q:concat_lhs*/length();
 }
}
