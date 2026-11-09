// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package app;
import lib.Holder;
import model.Payload;
// Supplementary character before multi-declarators: 😀
public class Declarations {
    int /*D:field_first*/first = 1, /*D:field_second*/second = 2;
    int /*D:field_shadow*/shadow = 7;
    Holder /*D:holder_left*/left = new Holder(), /*D:holder_right*/right = new Holder();
    Payload /*D:field_vector*/vector[] = null, /*D:field_object*/object = new Payload();
    Payload[] /*D:field_matrix*/matrix[] = null, /*D:field_row*/row = null;
    static int combine(int a, int b) { return a + b; }
    int run() {
        int /*D:local_first*/a = combine(1, 2), /*D:local_second*/b = /*Q:earlier_initializer*/a + 1;
        int /*D:local_values*/values[] = {1, 2}, /*D:local_scalar*/scalar = 3;
        Payload /*D:local_vector*/localVector[] = null, /*D:local_object*/localObject = new Payload();
        Payload[] /*D:local_matrix*/localMatrix[] = null, /*D:local_row*/localRow = null;
        int result = /*Q:first_field*/first + /*Q:second_field*/second + /*Q:first_local*/a + /*Q:second_local*/b;
        result += /*Q:local_scalar_use*/scalar;
        result += /*Q:field_object_use*/object./*Q:field_object_member*/count;
        result += /*Q:local_object_use*/localObject./*Q:local_object_member*/count;
        result += /*Q:holder_right_use*/right./*Q:holder_scalar_use*/scalar./*Q:imported_scalar_member*/count;
        result += left./*Q:holder_second_use*/second./*Q:imported_second_member*/count;
        Payload[] copy = /*Q:field_vector_use*/vector;
        copy = /*Q:field_row_use*/row;
        copy = /*Q:local_vector_use*/localVector;
        copy = /*Q:local_row_use*/localRow;
        Payload[][] grid = /*Q:field_matrix_use*/matrix;
        grid = /*Q:local_matrix_use*/localMatrix;
        int[] ints = /*Q:local_array_use*/values;
        result += vector[0]./*Q:array_receiver*/count;
        {
            int /*D:scope_before*/before = /*Q:before_shadow_declaration*/shadow, /*D:scope_shadow*/shadow = 4;
            result += /*Q:scope_first_local*/before + /*Q:scope_second_local*/shadow;
        }
        return result + /*Q:after_shadow_scope*/shadow;
    }
}
