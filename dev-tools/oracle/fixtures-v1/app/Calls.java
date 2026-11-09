// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package app;
import lib.CallBase;
import lib.Value;
// Supplementary character before namespace queries: 😀
public class Calls extends CallBase {
    int /*D:call_field*/m = 1;
    int /*D:call_method*/m() { return 2; }
    int /*D:call_consumer*/consume(int input) { return input; }
    int fieldCase() {
        return /*Q:field_name_call*/m() + /*Q:field_name_value*/m;
    }
    int parameterCase(int /*D:call_parameter*/m) {
        return /*Q:parameter_name_call*/m() + /*Q:parameter_name_value*/m
            + /*Q:argument_consumer*/consume(/*Q:parameter_name_argument*/m);
    }
    int localCase() {
        int /*D:call_local*/m = 3;
        return /*Q:local_name_call*/m() + /*Q:local_name_value*/m;
    }
    int inheritedCase(int /*D:call_inherited_parameter*/inheritedCall) {
        return /*Q:inherited_name_call*/inheritedCall() + /*Q:inherited_name_value*/inheritedCall;
    }
    int receiverCase(Value /*D:call_receiver_parameter*/m) {
        return /*Q:receiver_name_call*/m() + /*Q:receiver_name_value*/m./*Q:receiver_read_call*/read();
    }
    int blockCase() {
        int result = 0;
        {
            int /*D:call_block_local*/m = 4;
            result += /*Q:block_name_call*/m() + /*Q:block_name_value*/m;
        }
        return result + /*Q:after_block_call*/m() + /*Q:after_block_value*/m;
    }
    class Nested {
        int /*D:call_nested_field*/m = 5;
        int nestedCase(int /*D:call_nested_parameter*/inheritedCall) {
            return /*Q:nested_outer_call*/m() + /*Q:nested_field_value*/m
                + /*Q:nested_inherited_call*/inheritedCall() + /*Q:nested_parameter_value*/inheritedCall;
        }
    }
    int overloadCase(int /*D:call_overload_parameter*/choose) {
        return /*Q:overload_name_call*/choose(/*Q:overload_name_argument*/choose);
    }
}
