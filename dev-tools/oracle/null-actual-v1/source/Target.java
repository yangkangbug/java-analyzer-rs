// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package nulls;
public class Target {
 public void /*D:related_parent*/related(Parent p) {}
 public void /*D:related_child*/related(Child p) {}
 public void /*D:reverse_child*/reverse(Child p) {}
 public void /*D:reverse_parent*/reverse(Parent p) {}
 public void /*D:vector_parent*/vector(Parent a, Parent b) {}
 public void /*D:vector_child*/vector(Child a, Child b) {}
 public void /*D:primitive_int*/primitive(int n) {}
 public void /*D:primitive_parent*/primitive(Parent p) {}
 public void /*D:only*/only(Parent p) {}
 public void /*D:array*/array(Parent[] p) {}
 public Product /*D:inner*/inner(Parent p) { return null; }
 public <T> void /*D:generic_any*/generic(T p) {}
 public void /*D:generic_parent*/generic(Parent p) {}
}
