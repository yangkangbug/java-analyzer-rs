// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
interface A {} interface B {} class Both implements A, B {}
public class C {
 public void /*D:invalid_a*/pick(A a) {}
 public void /*D:invalid_b*/pick(B b) {}
 public void use(C c, Both both) { c./*Q:reference_invalid_ambiguous*/pick(both); }
}
