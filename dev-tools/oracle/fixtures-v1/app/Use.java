// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package app;
import lib.Value;
import lib.Base;
// Supplementary character makes byte and UTF-16 offsets differ: 😀
public class Use extends /*Q:base_import*/Base {
    public /*Q:value_import*/Value /*D:box_field*/box;
    int /*D:shadow_field*/shadow;
    int /*D:child_inherited*/inherited;
    int run(int /*D:param*/input) {
        int /*D:local*/sum = /*Q:parameter*/input;
        {
            int /*D:shadow_local*/shadow = 3;
            sum = /*Q:local_shadow*/shadow;
        }
        sum = /*Q:field_after_scope*/shadow;
        sum = /*Q:local_use*/sum + /*Q:inherited*/inherited;
        sum = /*Q:base_only*/baseOnly;
        sum = box./*Q:field_chain*/count;
        sum = box./*Q:method_chain*/read();
        sum = /*Q:overload*/pick(1);
        return super./*Q:super_receiver*/inherited + sum;
    }
}
