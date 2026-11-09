// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
class ReturnInvalid {
    static void /*D:invalid_pick_float*/pick(float value) {}
    static void /*D:invalid_pick_other*/pick(boolean value) {}
    static double source(float value) { return value; }
    static float source(double value) { return 0; }
    static void use(float f) { /*Q:invalid_swapped_return*/pick(source(f)); }
}
