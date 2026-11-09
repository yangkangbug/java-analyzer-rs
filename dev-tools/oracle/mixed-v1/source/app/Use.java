// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package app;
import dep.Payload;
// UTF-8 offsets remain significant: 😀
public class Use extends dep.Base {
    @Override
    public int /*D:mixed_scalar*/scalar(Payload p) { return 1; }
    @Override
    public int /*D:mixed_primitive*/primitive(int p) { return 1; }
    @Override
    public int /*D:mixed_array*/array(dep.Payload[] p) { return 1; }
    @Override
    public int /*D:mixed_varargs*/varargs(Payload... p) { return 1; }
    @Override
    public int /*D:mixed_postfix*/postfix(int p[][]) { return 1; }
    public int /*D:mixed_package*/distinctPackage(Payload p) { return 1; }
    public int /*D:mixed_primitive_distinct*/distinctPrimitive(int p) { return 1; }
    public int /*D:mixed_dimensions*/distinctDimensions(Payload[] p) { return 1; }
    int probe(Payload p, Payload[] a, int[][] matrix) {
        return /*Q:binary_scalar_override*/scalar(p)
            + /*Q:binary_primitive_override*/primitive(1)
            + /*Q:binary_array_override*/array(a)
            + /*Q:binary_varargs_override*/varargs(a)
            + /*Q:binary_postfix_override*/postfix(matrix)
            + /*Q:binary_distinct_package*/distinctPackage(p)
            + /*Q:binary_distinct_primitive*/distinctPrimitive(1)
            + /*Q:binary_distinct_dimensions*/distinctDimensions(a);
    }
}
