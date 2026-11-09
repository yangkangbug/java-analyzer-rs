// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { static void /*D:single_primitive_mismatch*/m(int n) {} static void use(boolean b) { /*Q:single_primitive_mismatch*/m(b); } }
