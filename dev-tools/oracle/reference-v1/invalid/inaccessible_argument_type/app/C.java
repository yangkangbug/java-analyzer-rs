// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package app;
import base.Hidden;
public class C {
 public void /*D:invalid_only*/only(Object a) {}
 public void use(C c, Hidden a) { c./*Q:reference_invalid_inaccessible_argument_type*/only(a); }
}
