// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package strict;
class Parent {}
class Child extends Parent {}
class Target {
 public void /*D:first*/pick(Parent p) {}
 public void /*D:second*/pick(int p, Missing q) {}
}
class Use {
 void run(Target t, Child c) { t./*Q:invalid_missing*/pick(c); }
}
