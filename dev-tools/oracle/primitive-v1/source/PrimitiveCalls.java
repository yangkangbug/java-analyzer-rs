// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
// Astral prefix checks javac UTF-16 to native UTF-8 range conversion: 🧪
class Carrier { void /*D:carrier_other*/other(double value) {} }
class PrimitiveCalls {
    static float /*D:pick_float*/pick(float a, float b) { return a; }
    static double /*D:pick_double*/pick(double a, double b) { return a; }
    static double /*D:pick_three*/pick(double a, double b, double c) { return a; }
    static float /*D:pick_float_varargs*/pick(float... a) { return a[0]; }
    static double /*D:pick_double_varargs*/pick(double... a) { return a[0]; }
    static long /*D:widen_long*/widen(long value) { return value; }
    static double /*D:widen_double*/widen(double value) { return value; }
    static int /*D:choose_int*/choose(int value) { return value; }
    static long /*D:choose_long*/choose(long value) { return value; }
    static int /*D:choose_two*/choose(int left, int right) { return left; }
    static Carrier /*D:chain_float*/chain(float value) { return null; }
    static Carrier /*D:chain_double*/chain(double value) { return null; }
    static void use(float[] fs, double[] ds, int i, float f, double d, byte b, char c, int n, long l) {
        /*Q:float_array*/pick(fs[i], f);
        /*Q:double_array*/pick(ds[i], d);
        /*Q:mixed_promote*/pick(f, d);
        /*Q:widen_byte*/widen(b);
        /*Q:widen_char*/widen(c);
        /*Q:identity_int*/choose(n);
        /*Q:identity_long*/choose(l);
        /*Q:arity_typed*/choose(n, n);
        /*Q:nested_outer_refusal*/pick(/*Q:nested_inner*/pick(f, f), f);
        /*Q:literal_index_refusal*/pick(fs[0], f);
        /*Q:receiver_spine_inner*/chain(f)./*Q:receiver_spine_outer_refusal*/other(d);
    }
}

class LoopScope {
    static double x;
    static float[] /*D:loop_scope_float*/scope(float value) { return null; }
    static float[] /*D:loop_scope_double*/scope(double value) { return null; }
    static void use() {
        for (float x : /*Q:enhanced_for_scope_refusal*/scope(x)) {}
    }
}
