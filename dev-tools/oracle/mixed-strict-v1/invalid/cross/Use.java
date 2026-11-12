// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package mixed;
class Ref {}
class Target {
 public void /*D:first*/cross(int a, Object b) {}
 public void /*D:second*/cross(long a, Ref b) {}
}
class Use { void run(Target t, Ref r) { t./*Q:invalid_cross*/cross(1, r); } }
