// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
// Astral prefix checks javac UTF-16 to native UTF-8 range conversion: 🧪
class ReturnCalls {
    static float /*D:return_pick_float*/pick(float a, float b) { return a; }
    static double /*D:return_pick_double*/pick(double a, double b) { return a; }
    static float /*D:return_identity_float*/identity(float value) { return value; }
    static double /*D:return_identity_double*/identity(double value) { return value; }
    static double /*D:return_change_float*/change(float value) { return value; }
    static float /*D:return_change_double*/change(double value) { return 0; }
    static float zero() { return 0; }
    static byte byteValue(byte value) { return value; }
    static char charValue(char value) { return value; }
    static boolean truth(boolean value) { return value; }
    static long /*D:return_widen_long*/widen(long value) { return value; }
    static double /*D:return_widen_double*/widen(double value) { return value; }
    static int /*D:return_integer_int*/integer(int value) { return value; }
    static long /*D:return_integer_long*/integer(long value) { return value; }
    static void /*D:return_flag_boolean*/flag(boolean value) {}
    static void /*D:return_flag_int*/flag(int value) {}
    static void /*D:return_single*/single(float value) {}
    static void use(float f, double d, byte b, char c, boolean yes) {
        /*Q:return_nested_float*/pick(identity(f), f);
        /*Q:return_nested_double*/pick(identity(d), d);
        /*Q:return_swapped_type*/pick(/*Q:return_selected_inner*/change(f), f);
        /*Q:return_zero_argument*/pick(zero(), f);
        /*Q:return_primitive_widen*/widen(byteValue(b));
        /*Q:return_char_widen*/integer(charValue(c));
        /*Q:return_boolean*/flag(truth(yes));
        /*Q:return_parenthesized*/pick(((identity(f))), f);
        /*Q:return_three_levels*/pick(identity(identity(f)), f);
        /*Q:return_singleton*/single(identity(f));
        /*Q:return_sibling_calls*/pick(identity(f), identity(f));
        /*Q:return_same_name*/pick(pick(f, f), f);
        /*Q:return_direct_control*/pick(f, f);
    }
}
