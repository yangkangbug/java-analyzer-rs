// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
class ReturnOther { static float foreign(float value) { return value; } }
class ReturnBoundaries {
    static float field;
    static float /*D:return_boundary_float*/pick(float a, float b) { return a; }
    static double /*D:return_boundary_double*/pick(double a, double b) { return a; }
    static void /*D:return_array_float*/array(float[] value) {}
    static void /*D:return_array_double*/array(double[] value) {}
    static float identity(float value) { return value; }
    static float fromText(String text) { return 0; }
    static Float boxed(float value) { return value; }
    static <T> float generic(float value) { return value; }
    static float spread(float... values) { return values[0]; }
    static float[] prefix(float[] values) { return values; }
    static float postfix(float[] values)[] { return values; }
    static void use(float f, float[] fs, int i, String text, boolean yes) {
        /*Q:return_literal_refusal*/pick(identity(0f), f);
        /*Q:return_field_refusal*/pick(identity(field), f);
        /*Q:return_reference_argument_refusal*/pick(fromText(text), f);
        /*Q:return_qualified_refusal*/pick(ReturnBoundaries.identity(f), f);
        /*Q:return_foreign_refusal*/pick(ReturnOther.foreign(f), f);
        /*Q:return_binary_refusal*/pick(Math.abs(f), f);
        /*Q:return_boxed_refusal*/pick(boxed(f), f);
        /*Q:return_generic_refusal*/pick(generic(f), f);
        /*Q:return_varargs_refusal*/pick(spread(f, f), f);
        /*Q:return_cast_refusal*/pick(identity((float) f), f);
        /*Q:return_conditional_refusal*/pick(identity(yes ? f : f), f);
        /*Q:return_prefix_array_refusal*/array(prefix(fs));
        /*Q:return_postfix_array_refusal*/array(postfix(fs));
        /*Q:return_indexed_array_refusal*/pick(prefix(fs)[i], f);
    }
}
