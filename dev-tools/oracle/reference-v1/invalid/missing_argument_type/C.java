// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
public class C {
 public void /*D:invalid_only*/only(Object a) {}
 public void use(C c, Missing a) { c./*Q:reference_invalid_missing_argument_type*/only(a); }
}
