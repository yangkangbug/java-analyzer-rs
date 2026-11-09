// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
class A {} class B {}
public class C { public void /*D:invalid_only*/only(A a) {} public void use(C c, B b) { c./*Q:reference_invalid_wrong_reference*/only(b); } }
