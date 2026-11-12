// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package qualified;
class Target {
 public void /*D:first*/pick(int value) {}
 public void /*D:second*/pick(long value) {}
}
public class Use {
 void run(Target t) { t./*Q:invalid_overflow*/pick(2147483648); }
}
