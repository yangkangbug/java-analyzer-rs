// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package nulls;
class Parent {}
class Child extends Parent {}
class Other {}
class Target {
 public void /*D:first*/pick(int n, Parent p) {}
 public void /*D:second*/pick(long n, Child p) {}
}
class Use { void run(Target t) { t./*Q:invalid_cross*/pick(1, null); } }
