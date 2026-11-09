// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
class ReturnInvalid {
    static void /*D:invalid_pick_float*/pick(float value) {}
    static void /*D:invalid_pick_other*/pick(double value) {}
    static float[] source(float value) { return null; }
    static void use(float f) { /*Q:invalid_array_result*/pick(source(f)); }
}
