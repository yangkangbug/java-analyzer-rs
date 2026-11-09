// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { static void /*D:single_primitive_narrowing*/m(float n) {} static void use(double d) { /*Q:single_primitive_narrowing*/m(d); } }
