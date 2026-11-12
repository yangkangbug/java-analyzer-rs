// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package qualified;
class Target {
 public void /*D:narrow*/pick(byte value) {}
}
public class Use {
 void run(Target t) { t./*Q:invalid_narrow*/pick(1); }
}
