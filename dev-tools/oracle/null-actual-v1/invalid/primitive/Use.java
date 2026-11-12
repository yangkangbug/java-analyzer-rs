// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package nulls;
class Parent {}
class Child extends Parent {}
class Other {}
class Target {
 public void /*D:only*/pick(int n) {}
}
class Use { void run(Target t) { t./*Q:invalid_primitive*/pick(null); } }
