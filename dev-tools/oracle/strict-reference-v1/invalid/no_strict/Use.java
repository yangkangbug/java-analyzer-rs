// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package strict;
class Parent {}
class Child extends Parent {}
class Target {
 public void /*D:first*/pick(int p) {}
}
class Use {
 void run(Target t, Child c) { t./*Q:invalid_no_strict*/pick(c); }
}
