// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
class Flow {
    static final int /*D:code*/CODE = 3;
    int run(int /*D:selector*/selector) {
        int /*D:result*/result = 0; // λ preserves UTF-8 offsets
        switch (/*Q:selector_use*/selector) {
            case 0:
                int /*D:shared*/shared = 1;
                result = /*Q:first_shared*/shared;
            case 1:
                shared = 2;
                result = /*Q:later_shared*/shared;
                break;
            case 2: {
                int /*D:nested*/nested = /*Q:nested_selector*/selector;
                result = /*Q:nested_use*/nested;
                break;
            }
            case /*Q:constant_label*/CODE:
                result = /*Q:constant_body*/selector;
                break;
            default:
                switch (/*Q:inner_selector*/selector) {
                    case 9:
                        int /*D:inner*/inner = 4;
                        result = /*Q:inner_use*/inner;
                        break;
                    default:
                        result = /*Q:inner_default*/selector;
                }
        }
        int /*D:after_shared*/shared = 7;
        return /*Q:after_use*/shared + /*Q:after_result*/result;
    }
}
