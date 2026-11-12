// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package mixed;
class Ref {}
class Target {
 public void /*D:narrow*/narrow(byte a, Ref r) {}
}
class Use { void run(Target t, Ref r) { t./*Q:invalid_narrow*/narrow(1, r); } }
