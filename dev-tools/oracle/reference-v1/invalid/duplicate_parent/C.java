// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
class A {}
public class C extends Parent {
 public void /*D:invalid_only*/only(A a) {}
 public void use(C c, A a) { c./*Q:reference_invalid_duplicate_parent*/only(a); }
}
